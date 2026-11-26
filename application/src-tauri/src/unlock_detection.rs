use std::collections::HashMap;
use crate::{ adb_server, device_inventory::{ self, Selection }, screen_power::{ self, AdbClient, Panel }, system_command, unlock_store::Binding };

const IDENTITY_QUERY: &str = "getprop ro.serialno; getprop ro.product.manufacturer; getprop ro.product.device; cmd activity get-current-user; getprop ro.build.version.sdk";
const METHOD_QUERY: &str = r#"dumpsys lock_settings | awk '
    /^User State:$/ { active=1; print; next }
    active && /^[^[:space:]]/ { active=0 }
    active && /^[[:space:]]+(User|CredentialType:|IsLockScreenDisabled:)/ { print }
'"#;

// Keep only current state fields. In particular, never return input history,
// cached editors, surrounding text, or the complete lock-settings dump.
const SNAPSHOT_QUERY: &str = r#"
printf '@@identity\n'
getprop ro.serialno
getprop ro.product.manufacturer
getprop ro.product.device
cmd activity get-current-user
getprop ro.build.version.sdk
printf '@@boot\n'
getprop sys.boot_completed
printf '@@policy\n'
dumpsys window policy | awk '
    /^[[:space:]]*KeyguardServiceDelegate[[:space:]]*$/ { active=1; print; next }
    /^[[:space:]]*Looper state:/ { active=0 }
    active && /^[[:space:]]*(showing|inputRestricted|userId|simSecure|occluded|bootCompleted|interactiveState)=/ { print }
'
printf '@@displays\n'
dumpsys window displays | grep -E 'Display: mDisplayId=|mCurrentFocus='
printf '@@ime\n'
dumpsys input_method | sed -n -e '/^  mCurrentImeUserId=/p' -e '/^  mUserDataRepository:$/,/^  [^ ]/p' | awk '
    /^  mCurrentImeUserId=/ || /^  mUserDataRepository:$/ || /^    userId=/ || /^      curClient=/ || /^      curEditorInfo:$/ || /^        inputType=/ || /^        packageName=/ { print; next }
    /^      [^ ]/ { print "      endEditor" }
'
printf '@@method\n'
dumpsys lock_settings | awk '
    /^User State:$/ { active=1; print; next }
    active && /^[^[:space:]]/ { active=0 }
    active && /^[[:space:]]+(User|CredentialType:|IsLockScreenDisabled:)/ { print }
'
printf '@@power\n'
# Display-group diagnostics also contain a numeric mWakefulness. Only the main
# symbolic field establishes the device's logical wakefulness.
dumpsys power | grep '^  mWakefulness=[A-Za-z]*$'
printf '@@panel\n'
dumpsys SurfaceFlinger --displays | grep -E '^[[:space:]]*((Virtual )?Display [0-9]|powerMode=)'
printf '@@identity_end\n'
getprop ro.serialno
getprop ro.product.manufacturer
getprop ro.product.device
cmd activity get-current-user
getprop ro.build.version.sdk
printf '@@end\n'
"#;

#[derive(Clone)]
pub struct Target {
    pub selection: Selection,
    pub client: AdbClient,
    pub binding: Binding,
    pub hardware_serial: String,
    pub sdk: u32,
}

pub struct Snapshot {
    pub method: String,
    pub unlocked: bool,
    pub safe_locked: bool,
    pub awake: bool,
    pub password_editor: bool,
    pub focus: Option<String>,
    pub alternate: bool,
    pub panel: Option<Panel>,
}

fn identity(output: &str) -> Option<(String, String, u32, u32)> {
    let values: Vec<_> = output.lines().map(str::trim).collect();
    if values.len() != 5 || values[..3].iter().any(|value| value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)) { return None; }
    Some((serde_json::to_string(&values[..3]).ok()?, values[0].into(), values[3].parse().ok()?, values[4].parse().ok()?))
}

pub fn target(selection: &Selection) -> Result<Target, String> {
    let _operation = adb_server::begin(false)?;
    device_inventory::validate(selection, &["-s", &selection.serial])?;
    let expected = selection.identity.as_ref().ok_or("Wait for the device's identity to be verified before configuring automatic unlock.")?;
    let transport = selection.transport_id.filter(|id| *id != 0).ok_or("The device transport could not be verified.")?.to_string();
    let client = AdbClient { path: system_command::adb_path(), socket: adb_server::endpoint()? };
    if client.path != selection.adb_path || client.socket != selection.server_socket { return Err("The selected ADB connection changed. Select the device again.".into()); }
    let output = client.query(&["-t", &transport, "shell", IDENTITY_QUERY], &mut || false, false)?;
    let (current, hardware_serial, user, sdk) = identity(&output).ok_or("The phone's identity and current Android user could not be verified.")?;
    if &current != expected { return Err("The selected device identity changed. No credential was retrieved.".into()); }
    Ok(Target { selection: selection.clone(), client, binding: Binding { identity: current, user }, hardware_serial, sdk })
}

pub fn query(target: &Target, script: &str, cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
    let transport = target.selection.transport_id.filter(|id| *id != 0).ok_or("The device transport could not be verified.")?.to_string();
    target.client.query(&["-t", &transport, "shell", script], cancelled, false)
}

fn method(output: &str, user: u32) -> &'static str {
    let mut active = false;
    let mut sections = 0;
    let mut users = 0;
    let mut current = None;
    let mut values = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "User State:" { sections += 1; active = true; current = None; continue; }
        if !active { continue; }
        if !line.is_empty() && !line.starts_with(char::is_whitespace) { active = false; current = None; continue; }
        if trimmed.starts_with("User") {
            current = trimmed.strip_prefix("User ").and_then(|value| value.parse::<u32>().ok());
            if current == Some(user) { users += 1; }
        } else if current == Some(user) {
            if let Some(value) = trimmed.strip_prefix("CredentialType:") { values.push(value.trim()); }
        }
    }
    if sections != 1 || users != 1 || values.len() != 1 { return "unknown"; }
    match values[0] { "PASSWORD" => "password", "PIN" => "pin", "PATTERN" => "pattern", "NONE" => "none", _ => "unknown" }
}

pub fn configured(target: &Target, cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
    let before = query(target, IDENTITY_QUERY, cancelled)?;
    let expected = identity(&before).is_some_and(|(id, _, user, sdk)| id == target.binding.identity && user == target.binding.user && sdk == target.sdk);
    if !expected { return Err("The phone identity or Android user changed. No credential was retrieved.".into()); }
    let output = query(target, METHOD_QUERY, cancelled)?;
    let after = query(target, IDENTITY_QUERY, cancelled)?;
    if before != after { return Err("The phone identity or Android user changed during detection.".into()); }
    Ok(method(&output, target.binding.user).into())
}

fn sections(output: &str) -> Option<HashMap<&str, String>> {
    let expected = ["identity", "boot", "policy", "displays", "ime", "method", "power", "panel", "identity_end", "end"];
    let mut values = HashMap::new();
    let mut current = None;
    let mut next = 0;
    for line in output.lines() {
        if let Some(name) = line.strip_prefix("@@") {
            if expected.get(next) != Some(&name) { return None; }
            next += 1;
            current = Some(name);
            values.insert(name, String::new());
        } else {
            let value = values.get_mut(current?).unwrap();
            value.push_str(line);
            value.push('\n');
        }
    }
    if next != expected.len() || !values.get("end")?.is_empty() { return None; }
    Some(values)
}

fn policy(output: &str) -> Option<HashMap<&str, &str>> {
    let mut sections = 0;
    let mut values = HashMap::new();
    for line in output.lines().map(str::trim) {
        if line == "KeyguardServiceDelegate" { sections += 1; continue; }
        let (key, value) = line.split_once('=')?;
        if sections != 1 || values.insert(key.trim(), value.trim()).is_some() { return None; }
    }
    let expected = ["showing", "inputRestricted", "userId", "simSecure", "occluded", "bootCompleted", "interactiveState"];
    (sections == 1 && values.len() == expected.len() && expected.iter().all(|key| values.contains_key(key))).then_some(values)
}

fn main_focus(output: &str, user: u32) -> Option<String> {
    let mut displays = 0;
    let mut current = false;
    let mut focus = Vec::new();
    for line in output.lines() {
        if let Some((_, value)) = line.split_once("Display: mDisplayId=") {
            let id: String = value.chars().take_while(char::is_ascii_digit).collect();
            current = id == "0";
            if current { displays += 1; }
        } else if current && line.contains("mCurrentFocus=") {
            focus.push(line.trim().to_string());
        }
    }
    if displays != 1 || focus.len() != 1 { return None; }
    let focus = focus.pop()?;
    if !focus.starts_with("mCurrentFocus=Window{") || !focus.split_whitespace().any(|field| field == format!("u{}", user)) { return None; }
    Some(focus)
}

fn current_password_editor(output: &str, user: u32) -> bool {
    let mut current_users = 0;
    let mut repositories = 0;
    let mut owners = 0;
    let mut clients = 0;
    let mut editors = 0;
    let mut types = Vec::new();
    let mut packages = Vec::new();
    let mut display = false;
    let mut repository = false;
    let mut owner = false;
    let mut editor = false;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("  mCurrentImeUserId=") {
            if value.trim().parse::<u32>().ok() != Some(user) { return false; }
            current_users += 1;
        } else if line == "  mUserDataRepository:" {
            repositories += 1; repository = true;
        } else if line.starts_with("  ") && !line.starts_with("   ") {
            repository = false; owner = false; editor = false;
        } else if repository {
            if let Some(value) = line.strip_prefix("    userId=") {
                owner = value.trim().parse::<u32>().ok() == Some(user);
                editor = false;
                if owner { owners += 1; }
            } else if owner {
                if line.starts_with("      curClient=") {
                    clients += 1;
                    display = line.trim_end().ends_with(" mSelfReportedDisplayId=0}");
                    editor = false;
                } else if line == "      curEditorInfo:" {
                    editors += 1; editor = true;
                } else if line.starts_with("      ") && !line.starts_with("       ") {
                    editor = false;
                } else if editor {
                    if let Some(value) = line.strip_prefix("        inputType=") { types.push(value.split_whitespace().next().unwrap_or_default()); }
                    if let Some(value) = line.strip_prefix("        packageName=") { packages.push(value.split_whitespace().next().unwrap_or_default()); }
                }
            }
        }
    }
    current_users == 1 && repositories == 1 && owners == 1 && clients == 1 && editors == 1 && display
        && types.len() == 1 && packages == ["com.android.systemui"]
        && types[0].strip_prefix("0x").and_then(|value| u32::from_str_radix(value, 16).ok()).is_some_and(|value| value & 255 == 129)
}

fn parse_snapshot(target: &Target, output: &str) -> Result<Snapshot, String> {
    let sections = sections(output).ok_or("The phone's unlock state was incomplete or ambiguous.")?;
    let matches = |section: &str| identity(section).is_some_and(|(id, serial, user, sdk)| {
        id == target.binding.identity && serial == target.hardware_serial && user == target.binding.user && sdk == target.sdk
    });
    if !matches(&sections["identity"]) || !matches(&sections["identity_end"]) { return Err("The phone identity or Android user changed. Automatic input stopped.".into()); }
    let values = policy(&sections["policy"]);
    let value = |key, expected| values.as_ref().and_then(|values| values.get(key)).copied() == Some(expected);
    let ready = sections["boot"].trim() == "1" && value("bootCompleted", "true") && value("simSecure", "false")
        && value("occluded", "false") && values.as_ref().and_then(|values| values.get("userId")).and_then(|user| user.parse::<u32>().ok()) == Some(target.binding.user);
    let awake = value("interactiveState", "INTERACTIVE_STATE_AWAKE")
        && sections["power"].lines().map(str::trim).filter(|line| *line == "mWakefulness=Awake").count() == 1
        && sections["power"].lines().count() == 1;
    let focus = main_focus(&sections["displays"], target.binding.user);
    let alternate = focus.as_ref().is_some_and(|focus| focus.contains("AlternateBouncer"));
    let password_focus = focus.as_ref().is_some_and(|focus| !alternate && ["NotificationShade", "Keyguard", "Bouncer"].iter().any(|name| focus.contains(name)));
    let method = method(&sections["method"], target.binding.user).to_string();
    let safe_locked = ready && target.binding.user == 0 && value("showing", "true") && value("inputRestricted", "true");
    let unlocked = ready && awake && value("showing", "false") && value("inputRestricted", "false");
    let password_editor = safe_locked && awake && password_focus && method == "password" && current_password_editor(&sections["ime"], target.binding.user);
    Ok(Snapshot { method, unlocked, safe_locked, awake, password_editor, focus, alternate, panel: screen_power::panel(&sections["panel"]) })
}

pub fn snapshot(target: &Target, cancelled: &mut impl FnMut() -> bool) -> Result<Snapshot, String> {
    parse_snapshot(target, &query(target, SNAPSHOT_QUERY, cancelled)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_fixture() -> Target {
        let id = serde_json::to_string(&["synthetic-serial", "Example", "synthetic-product"]).unwrap();
        Target {
            selection: Selection { serial: "synthetic-route".into(), transport_id: Some(7), identity: Some(id.clone()), adb_path: "synthetic-adb".into(), server_socket: "tcp:localhost:5037".into() },
            client: AdbClient { path: "synthetic-adb".into(), socket: "tcp:localhost:5037".into() },
            binding: Binding { identity: id, user: 0 }, hardware_serial: "synthetic-serial".into(), sdk: 37,
        }
    }

    fn fixture() -> String {
        [
            "@@identity", "synthetic-serial", "Example", "synthetic-product", "0", "37",
            "@@boot", "1", "@@policy", "  KeyguardServiceDelegate", "    showing=true", "    inputRestricted=true", "    userId=0", "    simSecure=false", "    occluded=false", "    bootCompleted=true", "    interactiveState=INTERACTIVE_STATE_AWAKE",
            "@@displays", "Display: mDisplayId=1", "  mCurrentFocus=Window{virtual u0 Example}", "Display: mDisplayId=0", "  mCurrentFocus=Window{keyguard u0 NotificationShade}",
            "@@ime", "  mCurrentImeUserId=0", "  mUserDataRepository:", "    userId=0", "      curClient=ClientState{synthetic mSelfReportedDisplayId=0}", "      curEditorInfo:", "        inputType=0x81 imeOptions=0x12000006", "        packageName=com.android.systemui autofillId=null",
            "@@method", "User State:", "  User 0", "    CredentialType: PASSWORD", "  User 10", "    CredentialType: PIN",
            "@@power", "  mWakefulness=Awake", "@@panel", "Display 123 (HWC display 0):", "  powerMode=Off",
            "@@identity_end", "synthetic-serial", "Example", "synthetic-product", "0", "37", "@@end", "",
        ].join("\n")
    }

    #[test]
    fn identifies_configured_method_only_for_the_requested_user() {
        for (reported, expected) in [("PASSWORD", "password"), ("PIN", "pin"), ("PATTERN", "pattern"), ("NONE", "none"), ("UNKNOWN_2", "unknown"), ("", "unknown")] {
            let dump = format!("User State:\n  User 10\n    CredentialType: PIN\n  User 0\n    CredentialType: {}\nKeys in namespace:\nCredentialType: PASSWORD\n", reported);
            assert_eq!(method(&dump, 0), expected);
            assert_eq!(method(&dump, 10), "pin");
            assert_eq!(method(&dump, 20), "unknown");
        }
    }

    #[test]
    fn rejects_missing_duplicate_and_ambiguous_method_dumps() {
        for dump in ["Permission Denial", "", "  User 0\n    CredentialType: PASSWORD", "User State:\n  User 0\n    CredentialType: PASSWORD\n    CredentialType: PASSWORD", "User State:\n  User 0\n    CredentialType: PASSWORD\n  User 0\n    CredentialType: PASSWORD", "User State:\n  User 0\n    CredentialType: PASSWORD\nUser State:\n  User 10\n    CredentialType: PIN"] { assert_eq!(method(dump, 0), "unknown"); }
        assert_eq!(method("User State:\n  User 0\n  User unknown\n    CredentialType: PASSWORD", 0), "unknown");
        assert_eq!(method("User State:\n  User 0\n    CredentialType: PASSWORD trailing-data", 0), "unknown");
    }

    #[test]
    fn verifies_main_display_current_owner_and_current_password_editor() {
        let state = parse_snapshot(&target_fixture(), &fixture()).unwrap();
        assert_eq!(state.method, "password");
        assert!(state.safe_locked && state.awake && state.password_editor && !state.unlocked && !state.alternate);
        assert_eq!(state.panel, Some(Panel::Off));
        assert!(state.focus.unwrap().contains("keyguard"));
    }

    #[test]
    fn never_accepts_biometrics_other_editors_or_stale_history() {
        for (from, to) in [
            ("NotificationShade", "AlternateBouncerView"),
            ("packageName=com.android.systemui", "packageName=com.example.app"),
            ("inputType=0x81", "inputType=0x12"),
            ("mSelfReportedDisplayId=0}", "mSelfReportedDisplayId=1}"),
            ("  mCurrentImeUserId=0", "  mCurrentImeUserId=10"),
            ("      curEditorInfo:", "      cachedEditorInfo:"),
            ("  mUserDataRepository:", "  Input method history:"),
            ("    CredentialType: PASSWORD", "    CredentialType: PATTERN"),
        ] {
            assert!(!parse_snapshot(&target_fixture(), &fixture().replace(from, to)).unwrap().password_editor, "accepted {}", to);
        }
    }

    #[test]
    fn policy_power_and_boot_guards_fail_closed() {
        for (from, to) in [
            ("simSecure=false", "simSecure=true"), ("occluded=false", "occluded=true"),
            ("bootCompleted=true", "bootCompleted=false"), ("userId=0", "userId=10"),
            ("@@boot\n1", "@@boot\n0"), ("inputRestricted=true", "inputRestricted=false"),
            ("    showing=true", "    showing=true\n    showing=true"),
        ] { assert!(!parse_snapshot(&target_fixture(), &fixture().replace(from, to)).unwrap().safe_locked, "accepted {}", to); }
        for (from, to) in [("INTERACTIVE_STATE_AWAKE", "INTERACTIVE_STATE_ASLEEP"), ("mWakefulness=Awake", "mWakefulness=Asleep")] {
            let state = parse_snapshot(&target_fixture(), &fixture().replace(from, to)).unwrap();
            assert!(!state.awake && !state.password_editor && !state.unlocked);
        }
    }

    #[test]
    fn requires_current_identity_before_and_after_the_snapshot() {
        for marker in ["@@identity\nsynthetic-serial", "@@identity_end\nsynthetic-serial"] {
            assert!(parse_snapshot(&target_fixture(), &fixture().replace(marker, &marker.replace("synthetic-serial", "replacement-phone"))).is_err());
        }
        assert!(parse_snapshot(&target_fixture(), &fixture().replace("synthetic-product", "replacement-product")).is_err());
        assert!(parse_snapshot(&target_fixture(), &fixture().replace("@@identity_end\nsynthetic-serial\nExample\nsynthetic-product\n0", "@@identity_end\nsynthetic-serial\nExample\nsynthetic-product\n10")).is_err());
    }

    #[test]
    fn only_verified_awake_unlocked_state_permits_manual_recovery() {
        let dump = fixture().replace("showing=true", "showing=false").replace("inputRestricted=true", "inputRestricted=false");
        let state = parse_snapshot(&target_fixture(), &dump).unwrap();
        assert!(state.unlocked && !state.safe_locked && !state.password_editor);
        assert!(!parse_snapshot(&target_fixture(), &dump.replace("occluded=false", "occluded=true")).unwrap().unlocked);
        assert!(!parse_snapshot(&target_fixture(), &dump.replace("mWakefulness=Awake", "mWakefulness=Asleep")).unwrap().unlocked);
    }

    #[test]
    fn rejects_duplicate_current_editors_and_main_displays() {
        for (from, to) in [
            ("Display: mDisplayId=0", "Display: mDisplayId=0\nDisplay: mDisplayId=0"),
            ("      curEditorInfo:", "      curEditorInfo:\n      curEditorInfo:"),
            ("        inputType=0x81 imeOptions=0x12000006", "        inputType=0x81\n        inputType=0x81"),
            ("    userId=0\n      curClient=", "    userId=0\n    userId=0\n      curClient="),
        ] { assert!(!parse_snapshot(&target_fixture(), &fixture().replace(from, to)).unwrap().password_editor); }
        assert!(parse_snapshot(&target_fixture(), &fixture().replace("@@ime", "@@policy")).is_err());
        assert!(parse_snapshot(&target_fixture(), fixture().trim_end_matches("@@end\n")).is_err());
    }
}
