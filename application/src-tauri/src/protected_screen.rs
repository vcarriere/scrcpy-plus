use serde::Serialize;
use std::time::{ Duration, Instant };
use crate::{ adb_server, system_command };
use crate::screen_power::AdbClient;

const CHECK_TIMEOUT: Duration = Duration::from_secs(7);
const SDK_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Serialize)]
pub struct Observation {
    state: &'static str,
    reason: &'static str,
    android_sdk: Option<u32>,
    power_commands: &'static str,
}

#[cfg(test)]
fn focus(displays: &str) -> Option<&str> {
    let mut focused = None;
    for line in displays.lines() {
        let Some(window) = line.trim().strip_prefix("mCurrentFocus=") else { continue; };
        let window = window.strip_prefix("Window{")?;
        if !window.contains('}') { return None; }
        let token = window.split_whitespace().next()?;
        if token.is_empty() || token.len() > 8 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) { return None; }
        if focused.is_some_and(|previous| previous != token) { return None; }
        focused = Some(token);
    }
    focused
}

fn flags(attributes: &str) -> Option<bool> {
    let start = attributes.match_indices("fl=").find_map(|(index, _)| {
        (index == 0 || attributes.as_bytes()[index - 1].is_ascii_whitespace()).then_some(index + 3)
    })?;
    let value = attributes[start..].trim_start();
    let first = value.split_whitespace().next()?.trim_end_matches('}');
    if let Some(hex) = first.strip_prefix("0x").or_else(|| first.strip_prefix('#')) {
        return u32::from_str_radix(hex, 16).ok().map(|bits| bits & 0x2000 != 0);
    }
    if first == "0" { return Some(false); }
    let names: Vec<_> = value.split(|character: char| character.is_whitespace() || character == '|' || character == '}')
        .take_while(|name| !name.contains('='))
        .filter(|name| !name.is_empty()).collect();
    if names.is_empty() || names.iter().any(|name| !name.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_')) { return None; }
    Some(names.contains(&"SECURE"))
}

#[cfg(test)]
fn window_flag(dump: &str, token: &str) -> Option<bool> {
    let mut selected = String::new();
    let mut matching = false;
    let mut matches = 0;
    for line in dump.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Window #") {
            matching = trimmed.split_once("Window{").is_some_and(|(_, window)| window.split_whitespace().next() == Some(token));
            if matching { matches += 1; }
        }
        if matching { selected.push_str(line); selected.push('\n'); }
    }
    if matches != 1 { return None; }
    block_flag(&selected)
}

fn block_flag(block: &str) -> Option<bool> {
    let display = block.split_once("mDisplayId=")?.1.split_whitespace().next()?;
    if display != "0" { return None; }
    let attributes = block.split_once("mAttrs=")?.1.trim_start();
    if !attributes.starts_with('{') { return None; }
    let mut depth = 0;
    for (index, character) in attributes.char_indices() {
        if character == '{' { depth += 1; }
        if character == '}' {
            depth -= 1;
            if depth == 0 { return flags(&attributes[1..index]); }
        }
    }
    None
}

fn visible_windows(dump: &str) -> Option<Vec<String>> {
    if !dump.starts_with("WINDOW MANAGER WINDOWS") { return None; }
    let mut blocks = Vec::new();
    for line in dump.lines() {
        if line.trim_start().starts_with("Window #") { blocks.push(String::new()); }
        if let Some(block) = blocks.last_mut() { block.push_str(line); block.push('\n'); }
    }
    let selected: Vec<_> = blocks.into_iter().filter(|block| {
        let display = block.split_once("mDisplayId=").and_then(|(_, value)| value.split_whitespace().next());
        let visible = block.lines().any(|line| line.trim() == "isVisible=true");
        let surface = block.lines().any(|line| line.trim_start().starts_with("mHasSurface=true "));
        display == Some("0") && visible && surface
    }).collect();
    (!selected.is_empty()).then_some(selected)
}

fn biometric(block: &str) -> bool {
    // Standard Android biometric prompts can overlay a still-focused app.
    // Match the exact SystemUI title/owner, not app text mentioning a sensor.
    block.lines().next().is_some_and(|line| line.trim_end_matches(':').trim_end().ends_with(" BiometricPrompt}"))
        && block.split_whitespace().any(|word| word == "package=com.android.systemui")
        && ["ty=APPLICATION_OVERLAY", "ty=KEYGUARD_DIALOG", "ty=STATUS_BAR_SUB_PANEL", "ty=STATUS_BAR_PANEL"]
            .iter().any(|kind| block.split_whitespace().any(|word| word == *kind))
}

#[cfg(test)]
pub fn visible_biometric(dump: &str) -> bool {
    visible_windows(dump).is_some_and(|blocks| blocks.iter().any(|block| biometric(block)))
}

pub fn visible_protection(dump: &str) -> Option<bool> {
    let blocks = visible_windows(dump)?;
    let mut unknown = false;
    for block in blocks {
        match block_flag(&block) {
            Some(true) => return Some(true),
            Some(false) if biometric(&block) => return Some(true),
            Some(false) => {},
            None => unknown = true,
        }
    }
    (!unknown).then_some(false)
}

#[derive(PartialEq)]
struct VisibleState {
    protected: bool,
    reason: &'static str,
    windows: Vec<(String, Option<bool>, bool)>,
}

fn visible_state(dump: &str) -> Option<VisibleState> {
    let protected = visible_protection(dump)?;
    let mut windows = Vec::new();
    for block in visible_windows(dump)? {
        let heading = block.lines().next()?;
        let token = heading.split_once("Window{")?.1.split_whitespace().next()?;
        if token.is_empty() || token.len() > 8 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) { return None; }
        windows.push((token.to_string(), block_flag(&block), biometric(&block)));
    }
    windows.sort_by(|first, second| first.0.cmp(&second.0));
    if windows.windows(2).any(|pair| pair[0].0 == pair[1].0) { return None; }
    let reason = if windows.iter().any(|(_, secure, _)| *secure == Some(true)) { "visible_secure_window" }
        else if windows.iter().any(|(_, _, biometric)| *biometric) { "visible_biometric_prompt" }
        else { "visible_windows_clear" };
    Some(VisibleState { protected, reason, windows })
}

fn read(device: &str, command: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<Observation, String> {
    if device.is_empty() || device.len() > 1024 || device.starts_with('-') || device.chars().any(|character| character.is_control() || character.is_whitespace()) {
        return Err("Select a valid connected device first.".into());
    }
    let mut observation = Observation { state: "unknown", reason: "visible_state_unavailable", android_sdk: None, power_commands: "not_checked" };
    // Use the same visible main-display signal as the automatic controller,
    // including biometric overlays above a still-focused clear app. Recheck the
    // relevant windows/flags instead of returning a result during a transition.
    let before = command(&["-s", device, "shell", "dumpsys", "window", "visible"])?;
    let after = command(&["-s", device, "shell", "dumpsys", "window", "visible"])?;
    if let (Some(before), Some(after)) = (visible_state(&before), visible_state(&after)) {
        if before == after {
            observation.state = if after.protected { "secure" } else { "unflagged" };
            observation.reason = after.reason;
        } else { observation.reason = "visible_state_changed"; }
    }
    observation.android_sdk = command(&["-s", device, "shell", "getprop", "ro.build.version.sdk"]).ok().and_then(|value| value.trim().parse().ok());
    Ok(observation)
}

fn read_captured(device: &str, client: &AdbClient, query: &mut impl FnMut(&AdbClient, &[&str], Instant) -> Result<String, String>) -> Result<Observation, String> {
    let deadline = Instant::now() + CHECK_TIMEOUT;
    read(device, &mut |args| {
        let timeout = if args.ends_with(&["getprop", "ro.build.version.sdk"]) { deadline.min(Instant::now() + SDK_TIMEOUT) } else { deadline };
        query(client, args, timeout)
    })
}

pub fn inspect(device: &str) -> Result<Observation, String> {
    // Hold the operation guard for the whole read, so Stop cannot split it.
    let _operation = adb_server::begin(false)?;
    let client = AdbClient { path: system_command::adb_path(), socket: adb_server::endpoint()? };
    read_captured(device, &client, &mut |client, args, deadline| client.query(args, &mut || Instant::now() >= deadline, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(flags: &str, display: u32) -> String {
        format!("WINDOW MANAGER WINDOWS\n  Window #0 Window{{abc123 u0 example.app/Screen}}:\n    mDisplayId={} mSession=Session{{sample}}\n    mAttrs={{(0,0)(fillxfill) sim={{adjust=resize}} ty=BASE_APPLICATION\n      fl={}\n      pfl=HIDE_NON_SYSTEM_OVERLAY_WINDOWS\n    }}\n    isVisible=true\n", display, flags)
    }

    fn visible(flags: &str) -> String {
        window(flags, 0).replace("    isVisible=true", "    mHasSurface=true isReadyForDisplay()=true\n    isVisible=true")
    }

    #[test]
    fn distinguishes_secure_flags_from_other_fields_and_other_windows() {
        assert_eq!(window_flag(&window("SECURE HARDWARE_ACCELERATED", 0), "abc123"), Some(true));
        assert_eq!(window_flag(&window("HARDWARE_ACCELERATED", 0), "abc123"), Some(false));
        assert_eq!(window_flag(&window("0x80002000", 0), "abc123"), Some(true));
        assert_eq!(window_flag(&window("#80000000", 0), "abc123"), Some(false));
        let mut dump = window("HARDWARE_ACCELERATED", 0);
        dump.push_str(&window("SECURE", 0).replace("Window #0", "Window #1").replace("abc123", "def456"));
        assert_eq!(window_flag(&dump, "abc123"), Some(false));
        assert_eq!(flags("pfl=SECURE fl=HARDWARE_ACCELERATED"), Some(false));
        assert_eq!(flags("fl=SECURELY_NAMED_FLAG"), Some(false));
    }

    #[test]
    fn missing_or_ambiguous_information_is_unknown() {
        for dump in [window("SECURE", 1), window("0xnothex", 0), window("unknown", 0), "Permission Denial".into(), window("SECURE", 0).replace("mAttrs=", "other=")] {
            assert_eq!(window_flag(&dump, "abc123"), None);
        }
        let truncated = window("SECURE", 0);
        let truncated = truncated.split("    }\n").next().unwrap();
        assert_eq!(window_flag(truncated, "abc123"), None);
        assert_eq!(focus("mCurrentFocus=null"), None);
        assert_eq!(focus("mCurrentFocus=Window{abc123 u0 example.app/Screen}\nmCurrentFocus=Window{def456 u0 example.app/Screen}"), None);
        assert_eq!(focus("mCurrentFocus=Window{not-a-token u0 example.app/Screen}"), None);
    }

    #[test]
    fn visible_secure_apps_and_biometric_overlays_trigger_protection() {
        let visible = |flags: &str| window(flags, 0).replace("    isVisible=true", "    mHasSurface=true isReadyForDisplay()=true\n    isVisible=true");
        assert_eq!(visible_protection(&visible("SECURE HARDWARE_ACCELERATED")), Some(true));
        assert_eq!(visible_protection(&visible("HARDWARE_ACCELERATED")), Some(false));
        let biometric = visible("HARDWARE_ACCELERATED").replace("example.app/Screen", "BiometricPrompt")
            .replace("ty=BASE_APPLICATION", "ty=APPLICATION_OVERLAY").replace("mAttrs=", "package=com.android.systemui\n    mAttrs=");
        assert_eq!(visible_protection(&biometric), Some(true));
        assert!(visible_biometric(&biometric));
        assert!(!visible_biometric(&biometric.replace("com.android.systemui", "example.app")));
        assert_eq!(visible_protection(&biometric.replace("com.android.systemui", "example.app")), Some(false));
        assert_eq!(visible_protection(&biometric.replace("isVisible=true", "isVisible=false")), None);
        assert_eq!(visible_protection(&visible("SECURE").replace("mDisplayId=0", "mDisplayId=1")), None);
        assert_eq!(visible_protection("Permission denied"), None);
        let mut overlay = visible("HARDWARE_ACCELERATED");
        overlay.push_str(&visible("SECURE").replace("Window #0", "Window #1").replace("abc123", "def456"));
        assert_eq!(visible_protection(&overlay), Some(true));
        assert_eq!(visible_protection(&visible("unknown")), None);
    }

    #[test]
    fn changed_or_unreadable_visible_state_never_produces_a_stale_result() {
        for (after, reason) in [
            (visible("HARDWARE_ACCELERATED"), "visible_state_changed"),
            (visible("SECURE").replace("abc123", "def456"), "visible_state_changed"),
            ("Permission denied".into(), "visible_state_unavailable"),
            (visible("unknown"), "visible_state_unavailable"),
            (visible("SECURE") + &visible("SECURE"), "visible_state_unavailable"),
        ] {
            let mut reads = 0;
            let result = read("synthetic-device", &mut |args| {
                if args.ends_with(&["getprop", "ro.build.version.sdk"]) { return Ok("37".into()); }
                reads += 1;
                Ok(if reads == 1 { visible("SECURE") } else { after.clone() })
            }).unwrap();
            assert_eq!(result.state, "unknown");
            assert_eq!(result.reason, reason);
            assert_eq!(reads, 2);
        }
    }

    #[test]
    fn probe_uses_only_read_only_visible_queries_and_optional_sdk_metadata() {
        let mut calls = Vec::new();
        let observation = read("synthetic-device", &mut |args| {
            assert_eq!(&args[..3], ["-s", "synthetic-device", "shell"]);
            calls.push(args[3..].join(" "));
            Ok(match args[3..] {
                ["dumpsys", "window", "visible"] => visible("SECURE"),
                ["getprop", "ro.build.version.sdk"] => "37\n".into(),
                _ => panic!("Unexpected or mutating command"),
            })
        }).unwrap();
        assert_eq!(observation.state, "secure");
        assert_eq!(observation.power_commands, "not_checked");
        assert_eq!(calls.len(), 3);
        for device in ["", "-s", "bad\nselector"] { assert!(read(device, &mut |_| panic!("invalid selector must not run")).is_err()); }
        let serialized = serde_json::to_string(&observation).unwrap();
        assert!(!serialized.contains("example.app"));
        assert!(!serialized.contains("abc123"));
    }

    #[test]
    fn manual_check_matches_unfocused_secure_windows_and_biometric_overlays() {
        let clear = visible("HARDWARE_ACCELERATED");
        let secure = visible("SECURE").replace("abc123", "def456");
        let biometric = visible("HARDWARE_ACCELERATED").replace("abc123", "def456").replace("example.app/Screen", "BiometricPrompt")
            .replace("ty=BASE_APPLICATION", "ty=APPLICATION_OVERLAY").replace("mAttrs=", "package=com.android.systemui\n    mAttrs=");
        for (dump, expected, reason) in [
            (clear.clone(), "unflagged", "visible_windows_clear"),
            (clear.clone() + &secure, "secure", "visible_secure_window"),
            (clear.clone() + &biometric, "secure", "visible_biometric_prompt"),
            (clear.clone() + &biometric.replace("com.android.systemui", "example.app"), "unflagged", "visible_windows_clear"),
        ] {
            let result = read("synthetic-device", &mut |args| Ok(if args.ends_with(&["getprop", "ro.build.version.sdk"]) { "37".into() } else { dump.clone() })).unwrap();
            assert_eq!(result.state, expected);
            assert_eq!(result.reason, reason);
            assert_eq!(visible_protection(&dump), Some(expected == "secure"));
        }
    }

    #[test]
    fn manual_queries_share_a_captured_client_and_bounded_deadline() {
        let client = AdbClient { path: "selected-adb".into(), socket: "tcp:127.0.0.1:1".into() };
        let started = Instant::now();
        let mut visible_deadline = None;
        let result = read_captured("synthetic-device", &client, &mut |captured, args, deadline| {
            assert_eq!(captured.path, "selected-adb");
            assert_eq!(captured.socket, "tcp:127.0.0.1:1");
            assert!(deadline <= started + CHECK_TIMEOUT + Duration::from_millis(20));
            if args.ends_with(&["getprop", "ro.build.version.sdk"]) {
                assert!(deadline <= Instant::now() + SDK_TIMEOUT);
                return Err("Metadata unavailable".into());
            }
            if let Some(previous) = visible_deadline { assert_eq!(deadline, previous); }
            visible_deadline = Some(deadline);
            Ok(visible("SECURE"))
        }).unwrap();
        assert_eq!(result.state, "secure");
        assert_eq!(result.android_sdk, None);
    }

    #[test]
    #[ignore = "Internal subprocess fixture; sleeps only when its explicit marker is supplied"]
    fn slow_metadata_fixture() {
        if std::env::args().any(|argument| argument == "SCRCPY_PLUS_METADATA_FIXTURE") {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn stalled_optional_metadata_cannot_hold_a_determined_manual_result() {
        let client = AdbClient { path: std::env::current_exe().unwrap().to_string_lossy().to_string(), socket: "tcp:127.0.0.1:1".into() };
        let started = Instant::now();
        let result = read_captured("synthetic-device", &client, &mut |captured, args, deadline| {
            if args.ends_with(&["getprop", "ro.build.version.sdk"]) {
                return captured.query(&["--ignored", "--exact", "protected_screen::tests::slow_metadata_fixture", "--skip", "SCRCPY_PLUS_METADATA_FIXTURE"], &mut || Instant::now() >= deadline, false);
            }
            Ok(visible("SECURE"))
        }).unwrap();
        assert_eq!(result.state, "secure");
        assert_eq!(result.android_sdk, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[ignore = "Requires an explicitly selected connected phone; reads state without changing power"]
    fn inspect_live_device_without_changing_it() {
        let executable = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Set SCRCPY_PLUS_TEST_ADB");
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").expect("Set SCRCPY_PLUS_TEST_DEVICE");
        system_command::set_custom_paths(None, Some(executable));
        let observation = inspect(&device).unwrap();
        println!("{}", serde_json::to_string(&observation).unwrap());
    }

    #[test]
    #[ignore = "Requires an explicitly selected phone; compares focused and visible protection without changing device state"]
    fn compare_live_focused_and_visible_protection() {
        let executable = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Set SCRCPY_PLUS_TEST_ADB");
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").expect("Set SCRCPY_PLUS_TEST_DEVICE");
        system_command::set_custom_paths(None, Some(executable.clone()));
        let client = AdbClient { path: executable, socket: adb_server::endpoint().unwrap() };
        let displays = client.query(&["-s", &device, "shell", "dumpsys", "window", "displays"], &mut || false, false).unwrap();
        let focused = focus(&displays).and_then(|token| {
            let window = client.query(&["-s", &device, "shell", "dumpsys", "window", token], &mut || false, false).ok()?;
            let after = client.query(&["-s", &device, "shell", "dumpsys", "window", "displays"], &mut || false, false).ok()?;
            if focus(&after) != Some(token) { return None; }
            window_flag(&window, token)
        });
        let observation = inspect(&device).unwrap();
        let dump = client.query(&["-s", &device, "shell", "dumpsys", "window", "visible"], &mut || false, false).unwrap();
        let blocks = visible_windows(&dump).unwrap_or_default();
        println!("{}", serde_json::json!({
            "focused_state": focused.map(|secure| if secure { "secure" } else { "unflagged" }).unwrap_or("unknown"),
            "manual_state": observation.state, "manual_reason": observation.reason, "android_sdk": observation.android_sdk,
            "visible_protection": visible_protection(&dump),
            "visible_secure_flag": blocks.iter().any(|block| block_flag(block) == Some(true)),
            "visible_biometric": blocks.iter().any(|block| biometric(block)),
            "selected_app_focused": displays.lines().any(|line| line.trim_start().starts_with("mCurrentFocus=") && line.contains("com.google.android.apps.authenticator2/")),
            "selected_app_visible": blocks.iter().any(|block| block.lines().next().is_some_and(|line| line.contains("com.google.android.apps.authenticator2/"))),
            "all_visible_flags_readable": !blocks.is_empty() && blocks.iter().all(|block| block_flag(block).is_some()),
        }));
    }
}
