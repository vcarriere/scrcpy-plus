use std::collections::HashMap;
use std::sync::{ Arc, Mutex };
use crate::{ adb_server, device_inventory::{ self, Selection }, screen_power::{ self, AdbClient, Panel }, system_command, unlock_store::Binding };

const IDENTITY_QUERY: &str = "getprop ro.serialno; getprop ro.product.manufacturer; getprop ro.product.device; cmd activity get-current-user; getprop ro.build.version.sdk";
const METHOD_QUERY: &str = r#"dumpsys lock_settings | awk '
    /^User State:$/ { active=1; print; next }
    active && /^[^[:space:]]/ { active=0 }
    active && /^[[:space:]]+(User|CredentialType:|IsLockScreenDisabled:)/ { print }
'"#;

// Keep only current state fields. In particular, never return input history,
// cached editors, surrounding text, or the complete lock-settings dump.
// Fixed framing labels contain no options or escapes. The shell's builtin echo
// avoids starting external formatter processes for every snapshot.
const SNAPSHOT_QUERY: &str = r#"
echo '@@identity'
getprop ro.serialno
getprop ro.product.manufacturer
getprop ro.product.device
cmd activity get-current-user
getprop ro.build.version.sdk
echo '@@boot'
getprop sys.boot_completed
echo '@@policy'
dumpsys window policy | awk '
    /^[[:space:]]*KeyguardServiceDelegate[[:space:]]*$/ { active=1; print; next }
    /^[[:space:]]*Looper state:/ { active=0 }
    active && /^[[:space:]]*(showing|inputRestricted|userId|simSecure|occluded|bootCompleted|interactiveState)=/ { print }
'
echo '@@displays'
dumpsys window displays | grep -E 'Display: mDisplayId=|mCurrentFocus=|^[[:space:]]*mRotation='
echo '@@focus_display'
dumpsys input | awk '
    /^Input Dispatcher State:$/ { if (++sections > 1) { print "invalid"; active=0; next }; active=1; next }
    active && /^[^[:space:]]/ { active=0 }
    active && /^  FocusedDisplayId:/ { if ($0 ~ /^  FocusedDisplayId: [0-9]+$/) { sub(/^  FocusedDisplayId: /, ""); print } else { print "invalid" } }
'
echo '@@ime'
dumpsys input_method | sed -n -e '/^  mCurrentImeUserId=/p' -e '/^  mUserDataRepository:$/,/^  [^ ]/p' | awk '
    /^  mCurrentImeUserId=/ || /^  mUserDataRepository:$/ || /^    userId=/ || /^      curClient=/ || /^      curEditorInfo:$/ || /^        inputType=/ || /^        packageName=/ { print; next }
    /^      [^ ]/ { print "      endEditor" }
'
echo '@@method'
dumpsys lock_settings | awk '
    /^User State:$/ { active=1; print; next }
    active && /^[^[:space:]]/ { active=0 }
    active && /^[[:space:]]+(User|CredentialType:|IsLockScreenDisabled:)/ { print }
'
echo '@@power'
# Display-group diagnostics also contain a numeric mWakefulness. Only the main
# symbolic field establishes the device's logical wakefulness.
dumpsys power | grep '^  mWakefulness=[A-Za-z]*$'
echo '@@panel'
dumpsys SurfaceFlinger --displays | grep -E '^[[:space:]]*((Virtual )?Display [0-9]|powerMode=)'
echo '@@identity_end'
getprop ro.serialno
getprop ro.product.manufacturer
getprop ro.product.device
cmd activity get-current-user
getprop ro.build.version.sdk
echo '@@end'
"#;

#[derive(Clone)]
pub struct Target {
    pub selection: Selection,
    pub client: AdbClient,
    pub binding: Binding,
    pub hardware_serial: String,
    pub sdk: u32,
    diagnostics: Option<Arc<Mutex<crate::unlock_guard::Session>>>,
}

pub struct Snapshot {
    pub method: String,
    pub unlocked: bool,
    pub safe_locked: bool,
    pub awake: bool,
    pub main_display_focused: bool,
    pub rotation: Option<u32>,
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
    Ok(Target { selection: selection.clone(), client, binding: Binding { identity: current, user }, hardware_serial, sdk, diagnostics: None })
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
    let expected = ["identity", "boot", "policy", "displays", "focus_display", "ime", "method", "power", "panel", "identity_end", "end"];
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

fn main_rotation(output: &str) -> Option<u32> {
    let mut current = false;
    let mut displays = 0;
    let mut rotations = Vec::new();
    for line in output.lines() {
        if let Some((_, value)) = line.split_once("Display: mDisplayId=") {
            current = value.chars().take_while(char::is_ascii_digit).eq("0".chars());
            if current { displays += 1; }
        } else if current {
            if let Some(value) = line.trim().strip_prefix("mRotation=") {
                rotations.push(value.split_whitespace().next()?.parse::<u32>().ok()?);
            }
        }
    }
    (displays == 1 && rotations.len() == 1).then(|| rotations[0]).filter(|rotation| *rotation <= 3)
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
    let main_display_focused = sections["focus_display"].lines().count() == 1 && sections["focus_display"].trim() == "0";
    let safe_locked = ready && target.binding.user == 0 && value("showing", "true") && value("inputRestricted", "true");
    let unlocked = ready && awake && value("showing", "false") && value("inputRestricted", "false");
    let password_editor = safe_locked && awake && main_display_focused && password_focus && method == "password" && current_password_editor(&sections["ime"], target.binding.user);
    Ok(Snapshot { method, unlocked, safe_locked, awake, main_display_focused, rotation: main_rotation(&sections["displays"]), password_editor, focus, alternate, panel: screen_power::panel(&sections["panel"]) })
}

pub fn snapshot(target: &Target, cancelled: &mut impl FnMut() -> bool) -> Result<Snapshot, String> {
    let output = match &target.diagnostics {
        Some(session) => session.lock().map_err(|_| "The private unlock-state channel stopped.")?.snapshot(cancelled)?,
        None => query(target, SNAPSHOT_QUERY, cancelled)?,
    };
    parse_snapshot(target, &output)
}

pub fn start_diagnostics(target: &mut Target, cancelled: &mut impl FnMut() -> bool) -> Result<(), String> {
    let mut session = crate::unlock_guard::Session::start(target, cancelled).map_err(|_| "The private unlock-state helper could not be prepared.")?;
    // Probe before selecting this backend or retrieving any credential. Every
    // subsequent observation uses this same channel and the same native parser;
    // a failed selected channel never falls back during password replay.
    let output = session.snapshot(cancelled)?;
    parse_snapshot(target, &output)?;
    target.diagnostics = Some(Arc::new(Mutex::new(session)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_only_target() -> Target {
        let path = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Select the existing ADB executable explicitly.");
        let transport = std::env::var("SCRCPY_PLUS_TEST_TRANSPORT").expect("Select the connected Pixel transport explicitly.");
        let socket = "tcp:localhost:5037".to_string();
        assert!(adb_server::version(&socket).unwrap().is_some(), "The existing server must already be running.");
        let client = AdbClient { path: path.clone(), socket: socket.clone() };
        let output = client.query(&["-t", &transport, "shell", IDENTITY_QUERY], &mut || false, false).unwrap();
        let (id, hardware_serial, user, sdk) = identity(&output).expect("The selected transport must return a valid identity.");
        Target {
            selection: Selection { serial: "read-only-benchmark".into(), transport_id: Some(transport.parse().unwrap()), identity: Some(id.clone()), adb_path: path, server_socket: socket },
            client, binding: Binding { identity: id, user }, hardware_serial, sdk, diagnostics: None,
        }
    }

    #[test]
    #[ignore = "Read-only Pixel diagnostic benchmark; requires explicitly selected existing ADB server and transport, never reads credentials or sends input"]
    fn live_snapshot_timing_without_input() {
        use std::time::Instant;
        let mut target = read_only_target();
        let mut external_query = SNAPSHOT_QUERY.to_string();
        for name in ["identity", "boot", "policy", "displays", "focus_display", "ime", "method", "power", "panel", "identity_end", "end"] {
            let marker = format!("echo '@@{}'", name);
            assert_eq!(external_query.matches(&marker).count(), 1, "Each benchmark marker must be present exactly once.");
            external_query = external_query.replace(&marker, &format!("printf '@@{}\\n'", name));
        }
        let mut external = Vec::new();
        let mut builtin = Vec::new();
        for trial in 0..5 {
            for fast in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                let started = Instant::now();
                let output = query(&target, if fast { SNAPSHOT_QUERY } else { &external_query }, &mut || false).unwrap();
                let state = parse_snapshot(&target, &output).expect("Every timed response must pass the native identity and state parser.");
                assert!(state.method == "password" && state.panel.is_some() && (state.safe_locked || state.unlocked), "Every measured response must contain a recognized complete device state; no input was sent.");
                if fast { builtin.push(started.elapsed().as_millis()); } else { external.push(started.elapsed().as_millis()); }
            }
        }
        let mut external_sorted = external.clone(); external_sorted.sort();
        let mut builtin_sorted = builtin.clone(); builtin_sorted.sort();
        println!("Validated native diagnostic comparison: external_markers_ms={:?}, external_markers_median_ms={}, builtin_markers_ms={:?}, builtin_markers_median_ms={}, credential_access=false, input_sent=false", external, external_sorted[2], builtin, builtin_sorted[2]);
        let started = Instant::now();
        start_diagnostics(&mut target, &mut || false).expect("The new backend must activate and pass native identity/state parsing.");
        let startup = started.elapsed().as_millis();
        let mut helper = Vec::new();
        let mut legacy = Vec::new();
        for trial in 0..5 {
            for fast in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                let started = Instant::now();
                let state = if fast { snapshot(&target, &mut || false).unwrap() } else { parse_snapshot(&target, &query(&target, SNAPSHOT_QUERY, &mut || false).unwrap()).unwrap() };
                assert!(state.method == "password" && state.panel.is_some() && (state.safe_locked || state.unlocked));
                if fast { helper.push(started.elapsed().as_millis()); } else { legacy.push(started.elapsed().as_millis()); }
            }
        }
        let mut helper_sorted = helper.clone(); helper_sorted.sort();
        let mut legacy_sorted = legacy.clone(); legacy_sorted.sort();
        println!("Validated persistent diagnostic comparison: helper_startup_ms={}, helper_ms={:?}, helper_median_ms={}, legacy_ms={:?}, legacy_median_ms={}, credential_access=false, input_sent=false", startup, helper, helper_sorted[2], legacy, legacy_sorted[2]);
    }

    fn run_read_only_profile(entry: &str, verify: impl FnOnce(&Target, &str)) {
        use sha2::{ Digest, Sha256 };
        use std::io::Read;
        #[cfg(windows)]
        use std::os::windows::fs::OpenOptionsExt;
        let target = read_only_target();
        let path = std::env::var("SCRCPY_PLUS_TEST_PROFILE_JAR").unwrap();
        let expected_hash = std::env::var("SCRCPY_PLUS_TEST_PROFILE_SHA256").unwrap();
        let mut options = std::fs::OpenOptions::new(); options.read(true);
        #[cfg(windows)]
        options.share_mode(1);
        let mut file = options.open(&path).unwrap();
        let mut bytes = Vec::new(); (&mut file).take(262145).read_to_end(&mut bytes).unwrap();
        assert!(!bytes.is_empty() && bytes.len() < 262144);
        let actual_hash: String = Sha256::digest(&bytes).iter().map(|byte| format!("{:02x}", byte)).collect();
        assert_eq!(actual_hash, expected_hash);
        struct RemoteFile<'a> { target: &'a Target, path: String }
        impl Drop for RemoteFile<'_> {
            fn drop(&mut self) {
                if !adb_server::version(&self.target.client.socket).is_ok_and(|version| version.is_some()) { return; }
                if query(self.target, IDENTITY_QUERY, &mut || false).ok().and_then(|output| identity(&output)).is_some_and(|(id, _, user, sdk)| id == self.target.binding.identity && user == self.target.binding.user && sdk == self.target.sdk) {
                    let _ = query(self.target, &format!("rm -f {}", self.path), &mut || false);
                }
            }
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let remote = RemoteFile { target: &target, path: format!("/data/local/tmp/scrcpy-plus-unlock-profile-{}-{}.jar", std::process::id(), nonce) };
        let transport = target.selection.transport_id.unwrap().to_string();
        target.client.query(&["-t", &transport, "push", &path, &remote.path], &mut || false, false).unwrap();
        let hash = query(&target, &format!("sha256sum {}", remote.path), &mut || false).unwrap();
        assert!(hash.split_whitespace().eq([actual_hash.as_str(), remote.path.as_str()]));
        assert!(!entry.is_empty() && entry.chars().all(|character| character.is_ascii_alphanumeric() || character == '.'));
        let output = query(&target, &format!("CLASSPATH={} app_process / {}", remote.path, entry), &mut || false).unwrap().replace("\r\n", "\n");
        verify(&target, &output);
    }

    fn profile_frames<'a>(target: &Target, output: &'a str) -> &'a str {
        let output = output.strip_prefix("PROFILE_BEFORE\n").expect("The profiler must return a framed native snapshot.");
        let (before, rest) = output.split_once("PROFILE_TIMINGS\n").unwrap();
        let (timings, after) = rest.split_once("PROFILE_AFTER\n").unwrap();
        for snapshot in [before, after] {
            let state = parse_snapshot(target, snapshot).expect("Both profiling snapshots must pass the complete native parser.");
            assert!(state.method == "password" && state.panel.is_some() && (state.safe_locked || state.unlocked));
        }
        timings
    }

    #[test]
    #[ignore = "Read-only phone-side diagnostic profiling with an explicitly hash-pinned test JAR; no credential retrieval, input, or power change"]
    fn live_guard_profile_without_input() {
        run_read_only_profile("com.scrcpyplus.unlockguard.MainProfile", |target, output| {
            let timings = profile_frames(target, output);
            let labels = ["policy", "displays", "focus_display", "input_method", "lock_settings", "power", "SurfaceFlinger"];
            let mut rows: HashMap<&str, Vec<u64>> = labels.iter().map(|label| (*label, Vec::new())).collect();
            for (index, line) in timings.lines().enumerate() {
                let (label, micros) = line.split_once('=').unwrap();
                assert_eq!(label, labels[index % labels.len()]);
                let micros = micros.parse::<u64>().unwrap();
                assert!(micros <= 2500000);
                rows.get_mut(label).unwrap().push(micros);
            }
            assert!(rows.values().all(|values| (3..=5).contains(&values.len()) && values.len() == rows["policy"].len()));
            for label in labels {
                let values = &rows[label];
                let mut sorted = values.clone(); sorted.sort();
                println!("Phone-side diagnostic profile: service={}, micros={:?}, median_micros={}, credential_access=false, input_sent=false", label, values, sorted[sorted.len() / 2]);
            }
        });
    }

    #[test]
    #[ignore = "Read-only exact power-state text/proto comparison with a hash-pinned profiler; no credential retrieval, input, or power change"]
    fn live_power_profile_without_input() {
        run_read_only_profile("com.scrcpyplus.unlockguard.MainPowerProfile", |target, output| {
            let lines: Vec<_> = profile_frames(target, output).lines().collect();
            assert!(lines.len() % 4 == 0 && (3..=5).contains(&(lines.len() / 4)));
            let labels = ["power_text", "power_text_wakefulness", "power_proto", "power_proto_wakefulness"];
            let mut text = Vec::new();
            let mut proto = Vec::new();
            for trial in lines.chunks_exact(4) {
                let values: Vec<i64> = trial.iter().zip(labels).map(|(line, expected)| {
                    let (label, value) = line.split_once('=').unwrap(); assert_eq!(label, expected); value.parse().unwrap()
                }).collect();
                assert!((0..=2500000).contains(&values[0]) && (0..=2500000).contains(&values[2]));
                assert!((0..=3).contains(&values[1]) && (-1..=3).contains(&values[3]));
                assert!(values[1] == values[3] || (values[1] == 0 && values[3] == -1), "Only the documented omitted Asleep default may differ.");
                text.push(values[0]); proto.push(values[2]);
            }
            let mut text_sorted = text.clone(); text_sorted.sort();
            let mut proto_sorted = proto.clone(); proto_sorted.sort();
            println!("Validated phone power comparison: text_micros={:?}, text_median_micros={}, proto_micros={:?}, proto_median_micros={}, exact_or_documented_asleep_default=true, credential_access=false, input_sent=false", text, text_sorted[text_sorted.len() / 2], proto, proto_sorted[proto_sorted.len() / 2]);
        });
    }

    fn target_fixture() -> Target {
        let id = serde_json::to_string(&["synthetic-serial", "Example", "synthetic-product"]).unwrap();
        Target {
            selection: Selection { serial: "synthetic-route".into(), transport_id: Some(7), identity: Some(id.clone()), adb_path: "synthetic-adb".into(), server_socket: "tcp:localhost:5037".into() },
            client: AdbClient { path: "synthetic-adb".into(), socket: "tcp:localhost:5037".into() },
            binding: Binding { identity: id, user: 0 }, hardware_serial: "synthetic-serial".into(), sdk: 37, diagnostics: None,
        }
    }

    fn fixture() -> String {
        [
            "@@identity", "synthetic-serial", "Example", "synthetic-product", "0", "37",
            "@@boot", "1", "@@policy", "  KeyguardServiceDelegate", "    showing=true", "    inputRestricted=true", "    userId=0", "    simSecure=false", "    occluded=false", "    bootCompleted=true", "    interactiveState=INTERACTIVE_STATE_AWAKE",
            "@@displays", "Display: mDisplayId=1", "  mCurrentFocus=Window{virtual u0 Example}", "Display: mDisplayId=0", "  mCurrentFocus=Window{keyguard u0 NotificationShade}",
            "    mRotation=0 mDeferredRotationPauseCount=0",
            "@@focus_display", "0",
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
            ("@@focus_display\n0\n", "@@focus_display\n1\n"),
            ("@@focus_display\n0\n", "@@focus_display\n0\n0\n"),
            ("@@focus_display\n0\n", "@@focus_display\n"),
            ("@@focus_display\n0\n", "@@focus_display\ninvalid\n"),
            ("      curEditorInfo:", "      curEditorInfo:\n      curEditorInfo:"),
            ("        inputType=0x81 imeOptions=0x12000006", "        inputType=0x81\n        inputType=0x81"),
            ("    userId=0\n      curClient=", "    userId=0\n    userId=0\n      curClient="),
        ] { assert!(!parse_snapshot(&target_fixture(), &fixture().replace(from, to)).unwrap().password_editor); }
        assert!(parse_snapshot(&target_fixture(), &fixture().replace("@@ime", "@@policy")).is_err());
        assert!(parse_snapshot(&target_fixture(), fixture().trim_end_matches("@@end\n")).is_err());
    }

    #[test]
    fn native_tap_rotation_metadata_must_belong_only_to_the_current_main_display() {
        let dump = fixture();
        assert_eq!(parse_snapshot(&target_fixture(), &dump).unwrap().rotation, Some(0));
        assert_eq!(parse_snapshot(&target_fixture(), &dump.replace("mRotation=0", "mRotation=1")).unwrap().rotation, Some(1));
        for malformed in [dump.replace("    mRotation=0 mDeferredRotationPauseCount=0\n", ""), dump.replace("mRotation=0", "mRotation=unknown"), dump.replace("mRotation=0", "mRotation=0\n    mRotation=0"), dump.replace("mRotation=0", "mRotation=4")] {
            assert_eq!(parse_snapshot(&target_fixture(), &malformed).unwrap().rotation, None);
        }
        let secondary = dump.replace("Display: mDisplayId=1\n", "Display: mDisplayId=1\n    mRotation=3\n");
        assert_eq!(parse_snapshot(&target_fixture(), &secondary).unwrap().rotation, Some(0));
    }
}
