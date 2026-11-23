use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::Mutex;
use std::path::Path;
use std::ffi::OsStr;
use std::io::Read;
use std::process::Stdio;
use std::time::{ Duration, Instant };
use crate::adb_server;
use crate::mdns_discovery;

#[derive(Clone)]
struct BinaryPath { requested: Option<String>, effective: Option<String> }

impl BinaryPath {
    const fn empty() -> Self { Self { requested: None, effective: None } }

    fn select(requested: Option<String>) -> Self {
        let requested = requested.filter(|path| !path.is_empty());
        let effective = sanitize(requested.clone());
        Self { requested, effective }
    }
}

#[derive(Clone)]
struct BinaryPaths { scrcpy: BinaryPath, adb: BinaryPath }

static BINARY_PATHS: Mutex<BinaryPaths> = Mutex::new(BinaryPaths { scrcpy: BinaryPath::empty(), adb: BinaryPath::empty() });

// An empty or non-existent path clears the override so the bundled binary is used
fn sanitize(path: Option<String>) -> Option<String> {
    path.filter(|p| Path::new(p).is_file())
}

pub fn set_custom_paths(scrcpy_path: Option<String>, adb_path: Option<String>) {
    let paths = BinaryPaths { scrcpy: BinaryPath::select(scrcpy_path), adb: BinaryPath::select(adb_path) };
    *BINARY_PATHS.lock().unwrap() = paths;
}

pub fn adb_path() -> String {
    let default = if cfg!(windows) { "scrcpy/adb.exe" } else { "adb" };
    BINARY_PATHS.lock().unwrap().adb.effective.clone().unwrap_or(default.to_string())
}

pub fn scrcpy_path() -> String {
    let default = if cfg!(windows) { "scrcpy/scrcpy.exe" } else { "scrcpy" };
    BINARY_PATHS.lock().unwrap().scrcpy.effective.clone().unwrap_or(default.to_string())
}

fn system_path_candidate(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else { return false; };
    if !metadata.is_file() { return false; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // execvp skips non-executable files and continues searching PATH. This
        // check reads permission bits; it never starts the candidate program.
        if metadata.permissions().mode() & 0o111 == 0 { return false; }
    }
    true
}

fn binary_details(selected: &BinaryPath, default: &str, source: &str, search_path: Option<&OsStr>) -> serde_json::Value {
    let command = selected.effective.as_deref().unwrap_or(default);
    let source = if selected.effective.is_some() { "custom" } else { source };
    let resolved = if source == "system" {
        search_path.and_then(|paths| std::env::split_paths(paths).map(|directory| directory.join(command)).find(|candidate| system_path_candidate(candidate)))
            .and_then(|path| std::path::absolute(path).ok())
    } else { std::path::absolute(command).ok() };
    let available = resolved.as_ref().is_some_and(|path| path.is_file());
    serde_json::json!({
        "command": command,
        "path": resolved.map(|path| path.to_string_lossy().to_string()).unwrap_or_else(|| command.into()),
        "source": source,
        "available": available,
        "requested": selected.requested,
        "invalid": selected.requested.is_some() && selected.effective.is_none(),
    })
}

fn describe_binary_paths(paths: &BinaryPaths, search_path: Option<&OsStr>) -> serde_json::Value {
    let source = if cfg!(windows) { "bundled" } else { "system" };
    serde_json::json!({
        "scrcpy_path": binary_details(&paths.scrcpy, if cfg!(windows) { "scrcpy/scrcpy.exe" } else { "scrcpy" }, source, search_path),
        "adb_path": binary_details(&paths.adb, if cfg!(windows) { "scrcpy/adb.exe" } else { "adb" }, source, search_path),
    })
}

pub fn get_binary_paths() -> serde_json::Value {
    let paths = BINARY_PATHS.lock().unwrap().clone();
    let search_path = std::env::var_os("PATH");
    // Resolve the effective selection without executing ADB, scrcpy, or a custom
    // wrapper. A selected custom file remains selected if it later disappears.
    describe_binary_paths(&paths, search_path.as_deref())
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

fn execute_output(command: &str, args: Vec<&str>, session: bool) -> Result<(std::process::ExitStatus, String), String> {
    let mut cmd = Command::new(&command);
    cmd.args(&args);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    // scrcpy locates adb through this variable, so keep it consistent with the chosen adb
    if let Some(adb_path) = BINARY_PATHS.lock().unwrap().adb.effective.clone() {
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
    if session { return Ok((status, String::new())); }
    let output = receiver.recv_timeout(Duration::from_secs(1)).map_err(|_| "Could not read command output.")?;
    Ok((status, String::from_utf8_lossy(&output).to_string()))
}

fn execute(command: &str, args: Vec<&str>, session: bool) -> Result<String, String> {
    let (status, output) = execute_output(command, args, session)?;
    if !status.success() { return Err(format!("The selected executable exited with {}.", status)); }
    Ok(output)
}

pub fn scrcpy(args: Vec<&str>) -> Result<String, String> {
    let _operation = if args == ["--version"] { None } else { Some(adb_server::begin(false)?) };
    let command = scrcpy_path();
    return execute(&command, args, false);
}

pub fn run_scrcpy(args: Vec<&str>, automatic_screen: bool) -> Result<String, String> {
    let _session = adb_server::begin(true)?;
    let command = scrcpy_path();
    if automatic_screen { return crate::screen_power::run(&command, args); }
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

    struct TestDirectory { path: std::path::PathBuf }

    impl TestDirectory {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("scrcpy-plus-binary-paths-{}-{}", std::process::id(), nonce));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let Ok(entries) = std::fs::read_dir(&self.path) {
                for entry in entries.flatten() { let _ = std::fs::remove_file(entry.path()); }
            }
            let _ = std::fs::remove_dir(&self.path);
        }
    }

    #[test]
    fn default_binary_status_uses_the_same_commands_as_runtime_defaults() {
        let paths = BinaryPaths { scrcpy: BinaryPath::empty(), adb: BinaryPath::select(Some(String::new())) };
        let status = describe_binary_paths(&paths, None);
        assert_eq!(status["scrcpy_path"]["command"], if cfg!(windows) { "scrcpy/scrcpy.exe" } else { "scrcpy" });
        assert_eq!(status["adb_path"]["command"], if cfg!(windows) { "scrcpy/adb.exe" } else { "adb" });
        for key in ["scrcpy_path", "adb_path"] {
            assert_eq!(status[key]["source"], if cfg!(windows) { "bundled" } else { "system" });
            assert!(status[key]["requested"].is_null());
            assert_eq!(status[key]["invalid"], false);
            if cfg!(windows) { assert!(Path::new(status[key]["path"].as_str().unwrap()).is_absolute()); }
        }
    }

    #[test]
    fn missing_saved_override_falls_back_but_deleted_effective_override_does_not() {
        let directory = TestDirectory::new();
        let custom = directory.path.join("selected-helper.cmd");
        let requested = custom.to_string_lossy().to_string();
        let initial = BinaryPath::select(Some(requested.clone()));
        let initial_status = binary_details(&initial, "scrcpy/adb.exe", "bundled", None);
        assert_eq!(initial_status["source"], "bundled");
        assert_eq!(initial_status["command"], "scrcpy/adb.exe");
        assert_eq!(initial_status["requested"], requested);
        assert_eq!(initial_status["invalid"], true);

        std::fs::write(&custom, "@echo off\r\nexit /b 0\r\n").unwrap();
        let selected = BinaryPath::select(Some(requested.clone()));
        let present = binary_details(&selected, "scrcpy/adb.exe", "bundled", None);
        assert_eq!(present["source"], "custom");
        assert_eq!(present["available"], true);
        assert_eq!(present["invalid"], false);
        std::fs::remove_file(&custom).unwrap();
        let deleted = binary_details(&selected, "scrcpy/adb.exe", "bundled", None);
        assert_eq!(deleted["source"], "custom");
        assert_eq!(deleted["command"], requested);
        assert_eq!(deleted["path"], requested);
        assert_eq!(deleted["available"], false);
        assert_eq!(deleted["invalid"], false);
    }

    #[test]
    fn system_path_lookup_is_read_only_and_reports_missing_commands_honestly() {
        let directory = TestDirectory::new();
        let tool = directory.path.join("synthetic-adb");
        std::fs::write(&tool, "This fixture is not an executable.").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let search_path = std::env::join_paths([directory.path.as_path()]).unwrap();
        let found = binary_details(&BinaryPath::empty(), "synthetic-adb", "system", Some(&search_path));
        assert_eq!(found["source"], "system");
        assert_eq!(found["path"], tool.to_string_lossy().as_ref());
        assert_eq!(found["available"], true);
        let missing = binary_details(&BinaryPath::empty(), "synthetic-no-such-tool", "system", Some(&search_path));
        assert_eq!(missing["path"], "synthetic-no-such-tool");
        assert_eq!(missing["available"], false);
    }

    #[test]
    #[cfg(unix)]
    fn system_path_lookup_skips_non_executable_files_before_a_valid_candidate() {
        use std::os::unix::fs::PermissionsExt;
        let first = TestDirectory::new();
        let second = TestDirectory::new();
        let skipped = first.path.join("synthetic-adb");
        let executable = second.path.join("synthetic-adb");
        std::fs::write(&skipped, "This non-executable fixture must be skipped.").unwrap();
        std::fs::write(&executable, "This fixture has execute bits but is never launched.").unwrap();
        std::fs::set_permissions(&skipped, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let search_path = std::env::join_paths([first.path.as_path(), second.path.as_path()]).unwrap();
        let status = binary_details(&BinaryPath::empty(), "synthetic-adb", "system", Some(&search_path));
        assert_eq!(status["path"], executable.to_string_lossy().as_ref());
        assert_eq!(status["available"], true);
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        let missing = binary_details(&BinaryPath::empty(), "synthetic-adb", "system", Some(&search_path));
        assert_eq!(missing["path"], "synthetic-adb");
        assert_eq!(missing["available"], false);
    }

    #[test]
    fn custom_wrapper_status_checks_presence_without_running_the_wrapper() {
        let directory = TestDirectory::new();
        let wrapper = directory.path.join("password wrapper.cmd");
        std::fs::write(&wrapper, "@echo off\r\n>\"%~dp0executed.txt\" echo executed\r\n").unwrap();
        let selected = BinaryPath::select(Some(wrapper.to_string_lossy().to_string()));
        let status = binary_details(&selected, "scrcpy/scrcpy.exe", "bundled", None);
        assert_eq!(status["source"], "custom");
        assert_eq!(status["path"], wrapper.to_string_lossy().as_ref());
        assert_eq!(status["available"], true);
        assert!(!directory.path.join("executed.txt").exists());
    }

    #[test]
    fn relative_custom_commands_report_the_effective_absolute_path() {
        let command = "relative-tools/saved wrapper.cmd";
        let selected = BinaryPath { requested: Some(command.into()), effective: Some(command.into()) };
        let status = binary_details(&selected, "scrcpy/scrcpy.exe", "bundled", None);
        assert_eq!(status["command"], command);
        assert_eq!(status["source"], "custom");
        assert_eq!(status["path"], std::path::absolute(command).unwrap().to_string_lossy().as_ref());
        assert_eq!(status["available"], Path::new(command).is_file());
        assert_eq!(status["invalid"], false);
    }
}
