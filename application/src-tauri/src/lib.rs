use serde_json::{ json, Value };
mod system_command;
mod downloader;
mod adb_server;
mod mdns_discovery;
mod protected_screen;
mod screen_power;
mod screen_power_control;
mod device_inventory;
mod automatic_unlock;
mod unlock_detection;
mod unlock_guard;
mod unlock_store;
mod tool_catalog;
mod adb_installer;
mod launch_progress;
use tauri::Manager;
use system_command::{ scrcpy, adb };

#[tauri::command]
fn environment() -> Value {
  return json!({
    "app_version":  env!("CARGO_PKG_VERSION"),
    "releases_url": "https://api.github.com/repos/Frontesque/scrcpy-plus/releases",
    "is_windows":   cfg!(windows),
    "os":           std::env::consts::OS,
    "arch":         std::env::consts::ARCH
  });
}

#[tauri::command]
fn set_binary_paths(scrcpy_path: Option<String>, adb_path: Option<String>) -> Result<(), String> {
  let current = system_command::get_binary_paths();
  if current["scrcpy_path"]["requested"].as_str() == scrcpy_path.as_deref().filter(|path| !path.is_empty())
    && current["adb_path"]["requested"].as_str() == adb_path.as_deref().filter(|path| !path.is_empty()) { return Ok(()); }
  let _configuration = adb_server::configure()?;
  system_command::set_custom_paths(scrcpy_path, adb_path);
  Ok(())
}

fn check_selected_installation(destination: &std::path::Path, selected: &[String]) -> Result<(), String> {
  let destination = match destination.canonicalize() {
    Ok(path) => path,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(error) => return Err(error.to_string()),
  };
  for selected in selected {
    if let Ok(path) = std::path::Path::new(selected).canonicalize() {
      #[cfg(windows)]
      let inside = path.to_string_lossy().to_lowercase().starts_with(&(destination.to_string_lossy().to_lowercase() + "\\"));
      #[cfg(not(windows))]
      let inside = path.starts_with(&destination);
      if inside { return Err("This version contains a selected executable. Choose another version or change the executable selection before reinstalling it.".into()); }
    }
  }
  Ok(())
}

fn managed_tool_path(path: &std::path::Path, directory: &std::path::Path) -> Result<bool, String> {
  let root = match directory.canonicalize() {
    Ok(root) => root,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
    Err(error) => return Err(error.to_string()),
  };
  let target = path.canonicalize().map_err(|error| error.to_string())?;
  #[cfg(windows)]
  { Ok(target.to_string_lossy().to_lowercase().starts_with(&(root.to_string_lossy().to_lowercase() + "\\"))) }
  #[cfg(not(windows))]
  { Ok(target.starts_with(root)) }
}

#[tauri::command]
async fn get_binary_paths() -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(system_command::get_binary_paths).await.map_err(|e| e.to_string())
}

// Downloads a scrcpy release archive and extracts it to the app data folder,
// returning the paths of the extracted scrcpy and adb executables.
#[tauri::command]
async fn download_scrcpy(app: tauri::AppHandle, url: String, version: String) -> Result<Value, String> {
  let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
  tauri::async_runtime::spawn_blocking(move || {
    let _installation = adb_server::install()?;
    if base.join("scrcpy").join(&version).exists() {
      return downloader::kept_version(&base, &version)?.ok_or_else(|| "The kept scrcpy package could not be verified. Its files were preserved; repair or remove that copy explicitly before reinstalling it.".to_string());
    }
    let selected = system_command::get_binary_paths();
    let paths = ["scrcpy_path", "adb_path"].iter().filter_map(|key| selected[key]["path"].as_str().map(str::to_string)).collect::<Vec<_>>();
    check_selected_installation(&base.join("scrcpy").join(&version), &paths)?;
    let release = tool_catalog::scrcpy_release(&version)?;
    if release.url != url { return Err("The archive does not match the official release for this platform.".into()); }
    downloader::download_verified(&base, &release)
  })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_tool_releases(tool: String) -> Result<Vec<tool_catalog::Release>, String> {
  tauri::async_runtime::spawn_blocking(move || match tool.as_str() {
    "adb" => tool_catalog::adb_releases(),
    "scrcpy" => tool_catalog::scrcpy_releases(),
    _ => Err("Unknown tool.".into()),
  }).await.map_err(|_| "The release catalog could not finish.".to_string())?
}

#[tauri::command]
async fn get_installed_adb(app: tauri::AppHandle) -> Result<Vec<adb_installer::Installed>, String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || adb_installer::installed(&base)).await.map_err(|_| "Could not read kept ADB packages.".to_string())?
}

#[tauri::command]
async fn restore_adb(app: tauri::AppHandle, version: String) -> Result<Value, String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || {
    let _restoration = adb_server::install()?;
    adb_installer::restore(&base, &version)
  }).await.map_err(|_| "The kept ADB package could not be restored.".to_string())?
}

#[tauri::command]
async fn compact_adb_cache(app: tauri::AppHandle) -> Result<Value, String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || {
    let _compaction = adb_server::install()?;
    let selected = system_command::get_binary_paths();
    let mut protected = Vec::new();
    for tool in ["scrcpy_path", "adb_path"] {
      for field in ["path", "requested"] {
        if let Some(path) = selected[tool][field].as_str().filter(|path| !path.is_empty()) { protected.push(path.to_string()); }
      }
    }
    let socket = adb_server::endpoint()?;
    if adb_server::version(&socket)?.is_some() {
      let Some(path) = adb_server::details(&socket).ok().flatten().and_then(|details| details.path).filter(|path| !path.is_empty()) else {
        return Ok(json!({ "compressed": 0, "skipped": "running_server_path_unavailable" }));
      };
      protected.push(path);
    }
    adb_installer::compact_inactive(&base, &protected)
  }).await.map_err(|_| "Old ADB packages could not be compacted; their existing files were kept.".to_string())?
}

#[tauri::command]
async fn get_installed_scrcpy(app: tauri::AppHandle) -> Result<Vec<Value>, String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || downloader::kept_versions(&base)).await.map_err(|_| "Could not read kept scrcpy packages.".to_string())?
}

#[tauri::command]
async fn download_adb(app: tauri::AppHandle, version: String) -> Result<Value, String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || {
    let _installation = adb_server::install()?;
    let release = tool_catalog::adb_releases()?.into_iter().find(|release| release.version == version).ok_or("This ADB version is not in the official catalog. Use a verified kept package for an older version.")?;
    adb_installer::download(&base, &release)
  }).await.map_err(|_| "ADB installation could not finish.".to_string())?
}

#[tauri::command]
async fn inspect_tool_version(tool: String, path: Option<String>) -> Result<Option<String>, String> {
  tauri::async_runtime::spawn_blocking(move || system_command::inspect_tool_version(&tool, path.as_deref())).await.map_err(|_| "Could not inspect the executable version.".to_string())?
}

#[tauri::command]
async fn activate_tool_paths(app: tauri::AppHandle, scrcpy_path: Option<String>, adb_path: Option<String>) -> Result<(), String> {
  let base = app.path().app_data_dir().map_err(|error| error.to_string())?;
  tauri::async_runtime::spawn_blocking(move || {
    let _activation = adb_server::install()?;
    let current = system_command::get_binary_paths();
    for (name, requested) in [("scrcpy_path", &scrcpy_path), ("adb_path", &adb_path)] {
      if requested.as_deref().filter(|path| !path.is_empty()) == current[name]["requested"].as_str() { continue; }
      if let Some(path) = requested.as_deref().filter(|path| !path.is_empty()) {
        if !std::path::Path::new(path).is_file() { return Err("The selected tool is unavailable. Keep the current version and try again.".into()); }
        if name == "adb_path" && managed_tool_path(std::path::Path::new(path), &base.join("adb"))? { adb_installer::verify_selected(&base, path)?; }
        if name == "scrcpy_path" && managed_tool_path(std::path::Path::new(path), &base.join("scrcpy"))? { downloader::verify_kept(&base, path)?; }
      }
    }
    if adb_path.as_deref().filter(|path| !path.is_empty()) != current["adb_path"]["requested"].as_str() || current["adb_path"]["invalid"] == true {
      let candidate = adb_path.clone().filter(|path| !path.is_empty()).unwrap_or_else(system_command::default_adb_path);
      if let Some(running) = adb_server::version(&adb_server::endpoint()?)? {
        let protocol = system_command::inspect_adb_protocol(&candidate)?;
        if running != protocol {
          return Err("This ADB client uses a different server protocol. Stop the shared ADB server explicitly before changing versions; it will not be restarted automatically.".into());
        }
      }
    }
    system_command::set_custom_paths(scrcpy_path, adb_path);
    Ok(())
  }).await.map_err(|_| "Tool activation could not finish.".to_string())?
}

fn tool_update_result(tool: &str, current: Option<&str>, catalog: Result<Vec<tool_catalog::Release>, String>) -> Value {
  match catalog {
    Err(error) => json!({ "tool": tool, "state": "error", "current_version": current, "error": error }),
    Ok(releases) => match releases.first() {
      None => json!({ "tool": tool, "state": "unavailable", "current_version": current }),
      Some(release) => {
        let state = match current.and_then(|current| tool_catalog::compare_versions(&release.version, current)) {
          Some(std::cmp::Ordering::Greater) => "available",
          Some(_) => "current",
          None => "unknown",
        };
        json!({ "tool": tool, "state": state, "current_version": current, "latest_version": release.version, "release_url": release.release_url, "url": release.url })
      },
    },
  }
}

#[tauri::command]
async fn get_tool_updates() -> Result<Vec<Value>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let versions = system_command::dependency_versions();
    std::thread::scope(|scope| {
      let adb = scope.spawn(tool_catalog::adb_releases);
      let scrcpy = scope.spawn(tool_catalog::scrcpy_releases);
      vec![
        tool_update_result("adb", versions["adb"].as_str(), adb.join().unwrap_or_else(|_| Err("ADB release lookup failed.".into()))),
        tool_update_result("scrcpy", versions["scrcpy"].as_str(), scrcpy.join().unwrap_or_else(|_| Err("scrcpy release lookup failed.".into()))),
      ]
    })
  }).await.map_err(|_| "The tool update checks could not finish.".to_string())
}

#[tauri::command]
async fn get_dependency_versions() -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(system_command::dependency_versions).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn exec_adb(args: Vec<String>) -> Result<String, String> {
  tauri::async_runtime::spawn_blocking(move || adb(args.iter().map(|a| a.as_str()).collect()))
    .await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn exec_scrcpy(args: Vec<&str>) -> Result<String, String> {
  return scrcpy(args);
}

#[tauri::command]
async fn get_device_inventory() -> Result<Vec<device_inventory::Device>, String> {
  tauri::async_runtime::spawn_blocking(device_inventory::refresh).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn disconnect_device_route(selection: device_inventory::Selection) -> Result<(), String> {
  tauri::async_runtime::spawn_blocking(move || device_inventory::disconnect(&selection)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn run_scrcpy(args: Vec<String>, automatic_screen: Option<bool>, selection: Option<device_inventory::Selection>, launch_id: Option<String>) -> Result<(), String> {
  // Runs on a dedicated blocking thread and resolves once the scrcpy window is closed
  tauri::async_runtime::spawn_blocking(move || {
    launch_progress::run(launch_id.as_deref(), || {
      let args_ref: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
      if let Some(selection) = &selection { device_inventory::validate(selection, &args_ref)?; }
      system_command::run_scrcpy(args_ref, automatic_screen.unwrap_or(false), selection.as_ref()).map(|_| ())
    })
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_scrcpy_progress(launch_id: String) -> Result<Option<launch_progress::Snapshot>, String> {
  launch_progress::get(&launch_id)
}

fn adb_status() -> Value {
  let revision = adb_server::revision();
  let endpoint = adb_server::endpoint();
  let observed = endpoint.as_ref().map_err(|e| e.clone()).and_then(|socket| adb_server::version(socket));
  let running = observed.as_ref().ok().and_then(|v| *v).is_some();
  let server_details = if running { endpoint.as_ref().ok().and_then(|socket| adb_server::details(socket).ok().flatten()) } else { None };
  let (paused, changing, sessions) = adb_server::observe(running, revision);
  let state = match changing {
    Some(true) => "starting",
    Some(false) => "stopping",
    None => match &observed { Ok(Some(_)) => "running", Ok(None) => "stopped", Err(_) => "error" },
  };
  json!({
    "state": state, "paused": paused, "active_sessions": sessions,
    "endpoint": endpoint.unwrap_or_default(), "path": system_command::adb_path(),
    "protocol_version": observed.as_ref().ok().and_then(|v| *v),
    "server_details": server_details,
    "screen_sessions": screen_power::statuses().into_iter().filter(|session| session.automatic_screen).collect::<Vec<_>>(),
    "error": observed.err()
  })
}

#[tauri::command]
async fn inspect_unlock(selection: device_inventory::Selection) -> Result<automatic_unlock::Observation, String> {
  tauri::async_runtime::spawn_blocking(move || automatic_unlock::inspect(&selection)).await.map_err(|_| "Unlock check could not finish.".to_string())?
}

#[tauri::command]
async fn enroll_unlock(selection: device_inventory::Selection, password: String, confirmation: String) -> Result<automatic_unlock::Observation, String> {
  tauri::async_runtime::spawn_blocking(move || automatic_unlock::enroll(&selection, password, confirmation)).await.map_err(|_| "Password enrollment could not finish.".to_string())?
}

#[tauri::command]
async fn set_unlock_enabled(selection: device_inventory::Selection, enabled: bool) -> Result<automatic_unlock::Observation, String> {
  tauri::async_runtime::spawn_blocking(move || automatic_unlock::set_enabled(&selection, enabled)).await.map_err(|_| "Unlock settings could not be saved.".to_string())?
}

#[tauri::command]
async fn remove_unlock(selection: device_inventory::Selection) -> Result<automatic_unlock::Observation, String> {
  tauri::async_runtime::spawn_blocking(move || automatic_unlock::remove(&selection)).await.map_err(|_| "Password removal could not finish.".to_string())?
}

#[tauri::command]
fn stop_unlock_session(selection: device_inventory::Selection) -> Result<automatic_unlock::Observation, String> {
  automatic_unlock::stop(&selection)
}

#[tauri::command]
async fn get_adb_status() -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(adb_status).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_adb_client() -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(system_command::adb_client).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn inspect_protected_screen(device: String) -> Result<protected_screen::Observation, String> {
  tauri::async_runtime::spawn_blocking(move || protected_screen::inspect(&device)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn control_screen_power(id: u64, paused: bool) -> Result<(), String> {
  screen_power::control(id, paused)
}

#[tauri::command]
async fn control_adb(start: bool) -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(move || {
    let mut change = adb_server::change(start)?;
    let socket = adb_server::endpoint()?;
    let running = adb_server::version(&socket)?.is_some();
    if start && !running { system_command::start_adb()?; }
    if !start && running { adb_server::query(&socket, "host:kill")?; }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
      if adb_server::version(&socket)?.is_some() == start { break; }
      if std::time::Instant::now() >= deadline { return Err("ADB did not reach the requested state.".to_string()); }
      std::thread::sleep(std::time::Duration::from_millis(100));
    }
    change.complete(start);
    drop(change);
    Ok(adb_status())
  }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
  use super::*;

  fn release(version: &str) -> tool_catalog::Release {
    tool_catalog::Release { version: version.into(), url: "https://example.invalid/unused.zip".into(), release_url: "https://example.invalid/unused".into(), checksum: "0".repeat(64), checksum_type: "sha256".into(), size: 1 }
  }

  #[test]
  fn update_results_compare_package_releases_and_do_not_claim_unknown_versions_are_current() {
    assert_eq!(tool_update_result("adb", Some("37.0.3-16887417"), Ok(vec![release("37.0.3")]))["state"], "current");
    assert_eq!(tool_update_result("adb", Some("37.0.2-1234"), Ok(vec![release("37.0.3")]))["state"], "available");
    assert_eq!(tool_update_result("adb", None, Ok(vec![release("37.0.3")]))["state"], "unknown");
    assert_eq!(tool_update_result("scrcpy", Some("custom wrapper"), Ok(vec![release("v5.0.1")]))["state"], "unknown");
    let error = tool_update_result("adb", Some("37.0.3"), Err("offline".into()));
    assert_eq!(error["state"], "error");
    assert_eq!(error["current_version"], "37.0.3");
  }

  #[test]
  fn frontend_reload_can_reapply_unchanged_paths_during_installation() {
    let current = system_command::get_binary_paths();
    let scrcpy = current["scrcpy_path"]["requested"].as_str().map(str::to_string);
    let adb = current["adb_path"]["requested"].as_str().map(str::to_string);
    let _installation = adb_server::install().unwrap();
    assert!(set_binary_paths(scrcpy, adb).is_ok());
    assert!(set_binary_paths(Some("different selected executable".into()), None).is_err());
  }

  #[test]
  fn reinstall_cannot_remove_a_selected_custom_launcher() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("scrcpy-plus-selected-install-{}-{}", std::process::id(), nonce));
    let destination = root.join("v5.0.1");
    std::fs::create_dir_all(&destination).unwrap();
    let wrapper = destination.join("password launcher.cmd");
    let unrelated = root.join("other-tool");
    std::fs::write(&wrapper, b"keep custom launcher").unwrap();
    std::fs::write(&unrelated, b"another executable").unwrap();
    assert!(check_selected_installation(&destination, &[wrapper.to_string_lossy().to_string()]).is_err());
    assert!(check_selected_installation(&destination, &[unrelated.to_string_lossy().to_string()]).is_ok());
    assert!(managed_tool_path(&destination.join("..").join("v5.0.1").join("password launcher.cmd"), &destination).unwrap());
    assert!(!managed_tool_path(&unrelated, &destination).unwrap());
    #[cfg(windows)]
    assert!(managed_tool_path(std::path::Path::new(&wrapper.to_string_lossy().to_uppercase()), &destination).unwrap());
    assert_eq!(std::fs::read(&wrapper).unwrap(), b"keep custom launcher");
    std::fs::remove_file(wrapper).unwrap();
    std::fs::remove_file(unrelated).unwrap();
    std::fs::remove_dir(destination).unwrap();
    std::fs::remove_dir(root).unwrap();
  }
}

// #[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if let Ok(directory) = app.path().app_local_data_dir() { mdns_discovery::configure(directory); }
      Ok(())
    })
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![get_dependency_versions, exec_adb, exec_scrcpy, run_scrcpy, get_scrcpy_progress, environment, set_binary_paths, get_binary_paths, download_scrcpy, get_adb_status, get_adb_client, control_adb, inspect_protected_screen, control_screen_power, get_device_inventory, disconnect_device_route, inspect_unlock, enroll_unlock, set_unlock_enabled, remove_unlock, stop_unlock_session, get_tool_releases, get_installed_adb, get_installed_scrcpy, download_adb, inspect_tool_version, activate_tool_paths, get_tool_updates, restore_adb, compact_adb_cache])
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app, event| {
      if let tauri::RunEvent::ExitRequested { api, .. } = event {
        let first = screen_power::shutdown();
        if !screen_power::idle() && !screen_power::exit_ready() {
          api.prevent_exit();
          if first {
            let app = app.clone();
            std::thread::spawn(move || {
              let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
              while !screen_power::idle() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
              }
              screen_power::finish_shutdown();
              app.exit(0);
            });
          }
        }
      }
    });
}
