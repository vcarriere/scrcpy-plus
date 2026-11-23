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
}
