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
mod unlock_store;
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
    let selected = system_command::get_binary_paths();
    let paths = ["scrcpy_path", "adb_path"].iter().filter_map(|key| selected[key]["path"].as_str().map(str::to_string)).collect::<Vec<_>>();
    check_selected_installation(&base.join("scrcpy").join(&version), &paths)?;
    downloader::download_and_extract(&base, &url, &version)
  })
    .await
    .map_err(|e| e.to_string())?
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
async fn run_scrcpy(args: Vec<String>, automatic_screen: Option<bool>, selection: Option<device_inventory::Selection>) -> Result<(), String> {
  // Runs on a dedicated blocking thread and resolves once the scrcpy window is closed
  tauri::async_runtime::spawn_blocking(move || {
    let args_ref: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
    if let Some(selection) = &selection { device_inventory::validate(selection, &args_ref)?; }
    system_command::run_scrcpy(args_ref, automatic_screen.unwrap_or(false), selection.as_ref()).map(|_| ())
  })
  .await
  .map_err(|e| e.to_string())?
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
    .invoke_handler(tauri::generate_handler![get_dependency_versions, exec_adb, exec_scrcpy, run_scrcpy, environment, set_binary_paths, get_binary_paths, download_scrcpy, get_adb_status, get_adb_client, control_adb, inspect_protected_screen, control_screen_power, get_device_inventory, disconnect_device_route, inspect_unlock, enroll_unlock, set_unlock_enabled, remove_unlock, stop_unlock_session])
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
