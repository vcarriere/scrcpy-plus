use serde_json::{ json, Value };
mod system_command;
mod downloader;
mod adb_server;
mod mdns_discovery;
mod protected_screen;
mod screen_power;
mod screen_power_control;
mod device_inventory;
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
fn set_binary_paths(scrcpy_path: Option<String>, adb_path: Option<String>) {
  system_command::set_custom_paths(scrcpy_path, adb_path);
}

// Downloads a scrcpy release archive and extracts it to the app data folder,
// returning the paths of the extracted scrcpy and adb executables.
#[tauri::command]
async fn download_scrcpy(app: tauri::AppHandle, url: String, version: String) -> Result<Value, String> {
  let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
  tauri::async_runtime::spawn_blocking(move || downloader::download_and_extract(&base, &url, &version))
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_dependency_versions() -> Result<Value, String> {
  tauri::async_runtime::spawn_blocking(|| {
    let scrcpy_version: String = scrcpy(vec!["--version"]).unwrap_or_default().split_whitespace().nth(1).unwrap_or("Unable to find SCRCPY").to_string();
    let adb_version:    String = adb(vec!["--version"]).unwrap_or_default().split_whitespace().nth(6).unwrap_or("Unable to find ADB").to_string();
    return json!({
      "scrcpy_plus":  env!("CARGO_PKG_VERSION"),
      "scrcpy":       scrcpy_version,
      "adb":          adb_version
    });
  }).await.map_err(|e| e.to_string())
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
    if let Some(selection) = selection { device_inventory::validate(&selection, &args_ref)?; }
    system_command::run_scrcpy(args_ref, automatic_screen.unwrap_or(false)).map(|_| ())
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
    "screen_sessions": screen_power::statuses(),
    "error": observed.err()
  })
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

// #[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if let Ok(directory) = app.path().app_local_data_dir() { mdns_discovery::configure(directory); }
      Ok(())
    })
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![get_dependency_versions, exec_adb, exec_scrcpy, run_scrcpy, environment, set_binary_paths, download_scrcpy, get_adb_status, get_adb_client, control_adb, inspect_protected_screen, control_screen_power, get_device_inventory, disconnect_device_route])
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
