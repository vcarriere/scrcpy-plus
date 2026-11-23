use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::Mutex;
use std::path::Path;
use std::io::Read;
use std::process::Stdio;
use std::time::{ Duration, Instant };
use crate::adb_server;
use crate::mdns_discovery;

static CUSTOM_SCRCPY: Mutex<Option<String>> = Mutex::new(None);
static CUSTOM_ADB: Mutex<Option<String>> = Mutex::new(None);

// An empty or non-existent path clears the override so the bundled binary is used
fn sanitize(path: Option<String>) -> Option<String> {
    path.filter(|p| Path::new(p).is_file())
}

pub fn set_custom_paths(scrcpy_path: Option<String>, adb_path: Option<String>) {
    *CUSTOM_SCRCPY.lock().unwrap() = sanitize(scrcpy_path);
    *CUSTOM_ADB.lock().unwrap() = sanitize(adb_path);
}

pub fn adb_path() -> String {
    let default = if cfg!(windows) { "scrcpy/adb.exe" } else { "adb" };
    CUSTOM_ADB.lock().unwrap().clone().unwrap_or(default.to_string())
}

fn parse_adb_version(output: &str) -> Result<serde_json::Value, String> {
    let protocol = output.lines().find_map(|line| line.strip_prefix("Android Debug Bridge version "));
    let Some(protocol) = protocol.filter(|value| !value.trim().is_empty()) else {
        return Err("The selected executable did not report an ADB version.".into());
    };
    let version = output.lines().find_map(|line| line.strip_prefix("Version ")).map(str::trim).filter(|value| !value.is_empty());
    let path = output.lines().find_map(|line| line.strip_prefix("Installed as ")).map(str::trim).filter(|value| !value.is_empty());
    Ok(serde_json::json!({ "version": version, "protocol_version": protocol.trim(), "path": path }))
}

pub fn adb_client() -> serde_json::Value {
    let command = adb_path();
    let result = execute(&command, vec!["--version"], false).and_then(|output| parse_adb_version(&output));
    let fallback = if Path::new(&command).components().count() > 1 {
        std::path::absolute(&command).ok().map(|path| path.to_string_lossy().to_string()).unwrap_or(command.clone())
    } else { command.clone() };
    match result {
        Ok(mut details) => {
            if details["path"].is_null() { details["path"] = serde_json::json!(fallback); }
            details["command"] = serde_json::json!(command);
            details["is_windows"] = serde_json::json!(cfg!(windows));
            details
        },
        Err(error) => serde_json::json!({ "command": command, "path": fallback, "error": error, "is_windows": cfg!(windows) }),
    }
}

fn execute(command: &str, args: Vec<&str>, session: bool) -> Result<String, String> {
    let mut cmd = Command::new(&command);
    cmd.args(&args);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    // scrcpy locates adb through this variable, so keep it consistent with the chosen adb
    if let Some(adb_path) = CUSTOM_ADB.lock().unwrap().clone() {
        cmd.env("ADB", adb_path);
    }

    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    cmd.stdout(if session { Stdio::null() } else { Stdio::piped() });
    let mut child = cmd.spawn().map_err(|e| format!("Could not launch the selected executable: {}", e))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut chunk = [0; 4096];
            while let Ok(count) = stdout.read(&mut chunk) {
                if count == 0 { break; }
                let keep = count.min(65536 - output.len());
                output.extend_from_slice(&chunk[..keep]);
            }
            let _ = sender.send(output);
        });
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {},
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            },
        }
        if !session && started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The operation timed out after 10 seconds.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() { return Err(format!("The selected executable exited with {}.", status)); }
    if session { return Ok(String::new()); }
    let output = receiver.recv_timeout(Duration::from_secs(1)).map_err(|_| "Could not read command output.")?;
    Ok(String::from_utf8_lossy(&output).to_string())
}

pub fn scrcpy(args: Vec<&str>) -> Result<String, String> {
    let _operation = if args == ["--version"] { None } else { Some(adb_server::begin(false)?) };
    let default = if cfg!(windows) { "scrcpy/scrcpy.exe" } else { "scrcpy" };
    let command = CUSTOM_SCRCPY.lock().unwrap().clone().unwrap_or(default.to_string());
    return execute(&command, args, false);
}

pub fn run_scrcpy(args: Vec<&str>) -> Result<String, String> {
    let _session = adb_server::begin(true)?;
    let default = if cfg!(windows) { "scrcpy/scrcpy.exe" } else { "scrcpy" };
    let command = CUSTOM_SCRCPY.lock().unwrap().clone().unwrap_or(default.to_string());
    execute(&command, args, true)
}

pub fn adb(args: Vec<&str>) -> Result<String, String> {
    if args == ["--version"] { return execute(&adb_path(), args, false); }
    if args.iter().any(|arg| ["start-server", "kill-server", "server"].contains(arg)) {
        return Err("Use the ADB Start/Stop controls to manage the server.".into());
    }
    let _operation = adb_server::begin(false)?;
    let command = adb_path();
    let listing = args == ["devices"];
    let output = execute(&command, args, false)?;
    if listing { return Ok(mdns_discovery::refresh(output, |args| execute(&command, args.to_vec(), false))); }
    Ok(output)
}

pub fn start_adb() -> Result<String, String> {
    execute(&adb_path(), vec!["start-server"], false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_executable_is_an_error() {
        assert!(execute("scrcpy-plus-no-such-executable", vec![], false).is_err());
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        #[cfg(windows)]
        let result = execute("cmd.exe", vec!["/d", "/c", "exit", "17"], false);
        #[cfg(unix)]
        let result = execute("sh", vec!["-c", "exit 17"], false);
        assert!(result.unwrap_err().contains("17"));
    }

    #[test]
    fn client_release_is_separate_from_protocol_and_keeps_paths_with_spaces() {
        let parsed = parse_adb_version("Android Debug Bridge version 1.0.41\r\nVersion 37.0.3-1234\r\nInstalled as C:\\Test Tools\\adb.exe\r\n").unwrap();
        assert_eq!(parsed["version"], "37.0.3-1234");
        assert_eq!(parsed["protocol_version"], "1.0.41");
        assert_eq!(parsed["path"], "C:\\Test Tools\\adb.exe");
        assert!(parse_adb_version("not adb").is_err());
        let old = parse_adb_version("Android Debug Bridge version 1.0.41\n").unwrap();
        assert!(old["version"].is_null());
    }
}
