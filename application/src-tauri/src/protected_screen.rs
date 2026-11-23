use serde::Serialize;
use crate::{ adb_server, system_command };

#[derive(Debug, Serialize)]
pub struct Observation {
    state: &'static str,
    reason: &'static str,
    android_sdk: Option<u32>,
    power_commands: &'static str,
}

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
    let display = selected.split_once("mDisplayId=")?.1.split_whitespace().next()?;
    if display != "0" { return None; }
    let attributes = selected.split_once("mAttrs=")?.1.trim_start();
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

fn power_commands(help: &str) -> &'static str {
    let advertised = |command: &str| help.lines().any(|line| line.trim().split_whitespace().next() == Some(command));
    if !advertised("power-off") { return "unavailable"; }
    if advertised("power-on") { return "on_off"; }
    if advertised("power-reset") { return "reset_off"; }
    "unavailable"
}

fn read(device: &str, command: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<Observation, String> {
    if device.is_empty() || device.len() > 1024 || device.starts_with('-') || device.chars().any(|character| character.is_control() || character.is_whitespace()) {
        return Err("Select a valid connected device first.".into());
    }
    let mut observation = Observation { state: "unknown", reason: "focus_unavailable", android_sdk: None, power_commands: "unavailable" };
    let before = command(&["-s", device, "shell", "dumpsys", "window", "displays"])?;
    let Some(token) = focus(&before) else { return Ok(observation); };
    // The only variable shell argument is a validated hexadecimal window token.
    // Filter the dump to that window instead of collecting all app windows.
    let window = command(&["-s", device, "shell", "dumpsys", "window", token])?;
    let after = command(&["-s", device, "shell", "dumpsys", "window", "displays"])?;
    if focus(&after) != Some(token) {
        observation.reason = "focus_changed";
        return Ok(observation);
    }
    observation.reason = "window_unavailable";
    if let Some(secure) = window_flag(&window, token) {
        observation.state = if secure { "secure" } else { "unflagged" };
        observation.reason = "focused_window_flag";
    }
    observation.android_sdk = command(&["-s", device, "shell", "getprop", "ro.build.version.sdk"]).ok().and_then(|value| value.trim().parse().ok());
    if let Ok(help) = command(&["-s", device, "shell", "cmd", "display", "help"]) {
        observation.power_commands = power_commands(&help);
    }
    Ok(observation)
}

pub fn inspect(device: &str) -> Result<Observation, String> {
    // Hold the operation guard for the whole read, so Stop cannot split it.
    let _operation = adb_server::begin(false)?;
    read(device, &mut |args| {
        if args.ends_with(&["cmd", "display", "help"]) { return system_command::adb_display_help(device); }
        system_command::adb(args.to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(flags: &str, display: u32) -> String {
        format!("WINDOW MANAGER WINDOWS\n  Window #0 Window{{abc123 u0 example.app/Screen}}:\n    mDisplayId={} mSession=Session{{sample}}\n    mAttrs={{(0,0)(fillxfill) sim={{adjust=resize}} ty=BASE_APPLICATION\n      fl={}\n      pfl=HIDE_NON_SYSTEM_OVERLAY_WINDOWS\n    }}\n    isVisible=true\n", display, flags)
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
    fn changing_focus_never_produces_a_stale_secure_result() {
        let mut calls = 0;
        let result = read("synthetic-device", &mut |_| {
            calls += 1;
            Ok(match calls {
                1 => "mCurrentFocus=Window{abc123 u0 example.app/Screen}".into(),
                2 => window("SECURE", 0),
                _ => "mCurrentFocus=Window{def456 u0 example.app/Screen}".into(),
            })
        }).unwrap();
        assert_eq!(result.state, "unknown");
        assert_eq!(result.reason, "focus_changed");
        assert_eq!(calls, 3);
    }

    #[test]
    fn probe_uses_read_only_targeted_commands_and_reports_capabilities_honestly() {
        let mut calls = Vec::new();
        let observation = read("synthetic-device", &mut |args| {
            assert_eq!(&args[..3], ["-s", "synthetic-device", "shell"]);
            calls.push(args[3..].join(" "));
            Ok(match args[3..] {
                ["dumpsys", "window", "displays"] => "mCurrentFocus=Window{abc123 u0 example.app/Screen}".into(),
                ["dumpsys", "window", "abc123"] => window("SECURE", 0),
                ["getprop", "ro.build.version.sdk"] => "37\n".into(),
                ["cmd", "display", "help"] => "  power-reset DISPLAY_ID\n  power-off DISPLAY_ID\n".into(),
                _ => panic!("Unexpected or mutating command"),
            })
        }).unwrap();
        assert_eq!(observation.state, "secure");
        assert_eq!(observation.power_commands, "reset_off");
        assert_eq!(calls.len(), 5);
        assert_eq!(power_commands("  power-on DISPLAY_ID\n  power-off DISPLAY_ID"), "on_off");
        assert_eq!(power_commands("unrecognized command"), "unavailable");
        for device in ["", "-s", "bad\nselector"] { assert!(read(device, &mut |_| panic!("invalid selector must not run")).is_err()); }
        let serialized = serde_json::to_string(&observation).unwrap();
        assert!(!serialized.contains("example.app"));
        assert!(!serialized.contains("abc123"));
    }

    #[test]
    #[ignore = "Requires an explicitly selected connected phone; reads state without changing power"]
    fn inspect_live_device_without_changing_it() {
        let executable = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Set SCRCPY_PLUS_TEST_ADB");
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").expect("Set SCRCPY_PLUS_TEST_DEVICE");
        system_command::set_custom_paths(None, Some(executable));
        let observation = inspect(&device).unwrap();
        println!("{}", serde_json::to_string(&observation).unwrap());
        assert!(observation.android_sdk.is_some());
    }
}
