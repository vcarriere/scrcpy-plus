use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{ Child, Command, Stdio };
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::{ Mutex, atomic::{ AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering } };
use std::sync::Arc;
use std::time::{ Duration, Instant };
use crate::{ adb_server, protected_screen, screen_power_control, system_command };

const POWER_QUERY: &str = "dumpsys power | grep '^[[:space:]]*mWakefulness='";
const DISPLAY_QUERY: &str = "dumpsys SurfaceFlinger --displays | grep -E '^[[:space:]]*((Virtual )?Display [0-9]|powerMode=)'";

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static EXIT_READY: AtomicBool = AtomicBool::new(false);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static PREPARING: AtomicUsize = AtomicUsize::new(0);
static SESSIONS: Mutex<Option<HashMap<u64, Session>>> = Mutex::new(None);
#[cfg(test)]
static VIDEO_FRAMES: Mutex<Vec<(Instant, u32)>> = Mutex::new(Vec::new());

#[derive(Clone, Serialize)]
pub struct Status {
    id: u64,
    device: String,
    state: &'static str,
    reason: &'static str,
}

struct Session {
    identity: String,
    status: Status,
    control: Arc<AtomicU8>,
}

struct Registration { id: u64, control: Arc<AtomicU8> }

struct Preparation;

impl Preparation {
    fn begin() -> Result<Self, String> {
        let _sessions = SESSIONS.lock().unwrap();
        if SHUTTING_DOWN.load(Ordering::SeqCst) { return Err("SCRCPY+ is closing.".into()); }
        PREPARING.fetch_add(1, Ordering::SeqCst);
        Ok(Self)
    }
}

impl Drop for Preparation {
    fn drop(&mut self) { PREPARING.fetch_sub(1, Ordering::SeqCst); }
}

impl Registration {
    fn new(device: &str, identity: &str) -> Result<Self, String> {
        let mut sessions = SESSIONS.lock().unwrap();
        if SHUTTING_DOWN.load(Ordering::SeqCst) { return Err("SCRCPY+ is closing.".into()); }
        let sessions = sessions.get_or_insert_with(HashMap::new);
        if sessions.values().any(|session| session.identity == identity) {
            return Err("Automatic screen control is already running for this phone. Close that session first.".into());
        }
        let id = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let control = Arc::new(AtomicU8::new(0));
        sessions.insert(id, Session {
            identity: identity.into(), control: control.clone(),
            status: Status { id, device: device.into(), state: "starting", reason: "waiting_for_screen_off" },
        });
        Ok(Self { id, control })
    }

    fn update(&self, state: &'static str, reason: &'static str) {
        if let Some(session) = SESSIONS.lock().unwrap().as_mut().and_then(|sessions| sessions.get_mut(&self.id)) {
            session.status.state = state;
            session.status.reason = reason;
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(sessions) = SESSIONS.lock().unwrap().as_mut() { sessions.remove(&self.id); }
    }
}

pub fn statuses() -> Vec<Status> {
    SESSIONS.lock().unwrap().as_ref().map(|sessions| sessions.values().map(|session| session.status.clone()).collect()).unwrap_or_default()
}

pub fn control(id: u64, paused: bool) -> Result<(), String> {
    let sessions = SESSIONS.lock().unwrap();
    let session = sessions.as_ref().and_then(|sessions| sessions.get(&id)).ok_or("This mirroring session has ended.")?;
    session.control.store(if paused { 1 } else { 2 }, Ordering::SeqCst);
    Ok(())
}

pub fn shutdown() -> bool {
    let _sessions = SESSIONS.lock().unwrap();
    !SHUTTING_DOWN.swap(true, Ordering::SeqCst)
}
pub fn exit_ready() -> bool { EXIT_READY.load(Ordering::SeqCst) }
pub fn finish_shutdown() { EXIT_READY.store(true, Ordering::SeqCst); }
pub fn idle() -> bool { statuses().is_empty() && PREPARING.load(Ordering::SeqCst) == 0 }

#[derive(Clone)]
pub struct AdbClient { pub path: String, pub socket: String }

impl AdbClient {
    fn capture() -> Result<Self, String> {
        Ok(Self { path: system_command::adb_path(), socket: adb_server::endpoint()? })
    }

    pub fn query(&self, args: &[&str], cancelled: &mut impl FnMut() -> bool, help: bool) -> Result<String, String> {
        if cancelled() { return Err("Screen-control operation cancelled.".into()); }
        let mut command = Command::new(&self.path);
        command.args(args).env("ADB_SERVER_SOCKET", &self.socket).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|_| "Could not launch the selected executable.")?;
        let stdout = child.stdout.take().ok_or("Could not read ADB output.")?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut stdout = stdout;
            let mut chunk = [0; 4096];
            let mut truncated = false;
            while let Ok(count) = stdout.read(&mut chunk) {
                if count == 0 { break; }
                let keep = count.min(262144 - output.len());
                truncated |= keep != count;
                output.extend_from_slice(&chunk[..keep]);
            }
            let _ = sender.send((output, truncated));
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Err(_) => { let _ = child.kill(); let _ = child.wait(); return Err("Could not inspect the ADB operation.".into()); },
                _ => {},
            }
            if cancelled() || started.elapsed() >= Duration::from_secs(3) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Screen-control operation cancelled or timed out.".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if !status.success() && !help { return Err("The phone rejected a screen-control operation.".into()); }
        let (output, truncated) = receiver.recv_timeout(Duration::from_millis(300)).map_err(|_| "Could not finish reading ADB output.")?;
        if truncated { return Err("The phone's screen-state response was too large.".into()); }
        String::from_utf8(output).map_err(|_| "The phone returned an unreadable screen state.".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel { On, Off, Asleep }

pub fn panel(dump: &str) -> Option<Panel> {
    let mut physical = false;
    let mut displays = 0;
    let mut modes = Vec::new();
    for line in dump.lines().map(str::trim) {
        if line.starts_with("Virtual Display ") { physical = false; }
        if line.strip_prefix("Display ").is_some_and(|value| value.split_whitespace().next().is_some_and(|id| id.trim_end_matches(':').parse::<u64>().is_ok())) {
            physical = true; displays += 1;
        }
        if physical { if let Some(mode) = line.strip_prefix("powerMode=") { modes.push(mode); } }
    }
    if displays != 1 || modes.len() != 1 { return None; }
    match modes[0].trim() {
        "On" => Some(Panel::On), "Off" => Some(Panel::Off),
        "Doze" | "DozeSuspend" | "OnSuspend" => Some(Panel::Asleep), _ => None,
    }
}

fn awake(dump: &str) -> bool { dump.lines().any(|line| line.trim() == "mWakefulness=Awake") }

struct Prepared {
    device: String,
    identity: String,
    user: String,
    client: AdbClient,
}

fn selector(args: &[&str]) -> Result<String, String> {
    let mut device = None;
    let mut screen_off = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        if arg == "-s" || arg == "--serial" {
            index += 1;
            if device.is_some() || index >= args.len() { return Err("Select exactly one phone for automatic screen control.".into()); }
            device = Some(args[index]);
        } else if let Some(value) = arg.strip_prefix("--serial=") {
            if device.replace(value).is_some() { return Err("Select exactly one phone for automatic screen control.".into()); }
        } else if arg == "--turn-screen-off" || arg == "-S" { screen_off = true; }
        else if arg == "--display-id" {
            index += 1;
            if args.get(index) != Some(&"0") { return Err("Automatic screen control supports only the main phone display.".into()); }
        } else if arg.starts_with("--display-id=") && arg != "--display-id=0" {
            return Err("Automatic screen control supports only the main phone display.".into());
        } else if ["--no-control", "-n", "--otg", "--no-cleanup", "--no-power-on", "--no-video", "--select-usb", "--select-tcpip", "-d", "-e"].contains(&arg)
            || arg.starts_with("--new-display") || arg.starts_with("--video-source") || arg.starts_with("--tcpip") {
            return Err("Automatic screen control needs a normal main-display session with control, video, and cleanup enabled.".into());
        } else if arg.starts_with('-') && !arg.starts_with("--") && !["-w", "-t", "-f", "-b", "-m", "-r", "-p", "-V"].contains(&arg) {
            return Err("Use separate long scrcpy options, rather than combined short options, with automatic screen control.".into());
        }
        index += 1;
    }
    let device = device.filter(|device| !device.is_empty() && device.len() <= 1024 && !device.starts_with('-') && !device.chars().any(|character| character.is_control() || character.is_whitespace()))
        .ok_or("Select a connected phone for automatic screen control.")?;
    if !screen_off { return Err("Enable Turn off phone screen before automatic screen control.".into()); }
    Ok(device.into())
}

fn prepare(executable: &str, args: &[&str]) -> Result<Prepared, String> {
    let device = selector(args)?;
    let name = Path::new(executable).file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if !name.eq_ignore_ascii_case("scrcpy.exe") && name != "scrcpy" {
        return Err("Automatic screen control requires the direct scrcpy executable, rather than a launcher or password wrapper.".into());
    }
    let client = AdbClient::capture()?;
    let mut cancelled = || SHUTTING_DOWN.load(Ordering::SeqCst);
    let sdk = client.query(&["-s", &device, "shell", "getprop", "ro.build.version.sdk"], &mut cancelled, false)?;
    if sdk.trim().parse::<u32>().unwrap_or_default() < 35 { return Err("Automatic screen control currently requires Android 15 or later.".into()); }
    let identity = client.query(&["-s", &device, "shell", "getprop", "ro.serialno"], &mut cancelled, false)?.trim().to_string();
    let user = client.query(&["-s", &device, "shell", "am", "get-current-user"], &mut cancelled, false)?.trim().to_string();
    if identity.is_empty() || identity.eq_ignore_ascii_case("unknown") || user.parse::<u32>().is_err() {
        return Err("The phone's identity and current Android user could not be verified.".into());
    }
    let display = client.query(&["-s", &device, "shell", DISPLAY_QUERY], &mut cancelled, false)?;
    if panel(&display).is_none() { return Err("The main physical display's power state could not be verified.".into()); }
    Ok(Prepared { device, identity, user, client })
}

#[derive(Default)]
struct Policy {
    ready: bool,
    paused: bool,
    expected: Option<Panel>,
    clear_since: Option<Instant>,
    mismatch: usize,
    owned_on: bool,
}

impl Policy {
    fn pause(&mut self) { self.paused = true; self.owned_on = false; self.clear_since = None; }

    fn decide(&mut self, protected: Option<bool>, current: Panel, is_awake: bool, now: Instant) -> Option<Panel> {
        if self.paused { return None; }
        if !self.ready {
            if !is_awake || current != Panel::Off { return None; }
            self.ready = true;
            self.expected = Some(Panel::Off);
        }
        if !is_awake || current == Panel::Asleep { self.pause(); return None; }
        if self.expected.is_none() {
            self.expected = Some(current);
            self.owned_on = current == Panel::On;
        }
        if self.expected.is_some_and(|expected| expected != current) {
            self.mismatch += 1;
            if self.mismatch >= 2 { self.pause(); }
            return None;
        }
        self.mismatch = 0;
        match protected {
            Some(true) => { self.clear_since = None; if current == Panel::Off { return Some(Panel::On); } },
            Some(false) if current == Panel::On && self.owned_on => {
                let since = *self.clear_since.get_or_insert(now);
                if now.duration_since(since) >= Duration::from_millis(1000) { return Some(Panel::Off); }
            },
            _ => { self.clear_since = None; },
        }
        None
    }

    fn applied(&mut self, target: Panel) {
        self.expected = Some(target);
        self.owned_on = target == Panel::On;
        self.mismatch = 0;
        self.clear_since = None;
    }
}

fn verify_identity(prepared: &Prepared, query: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<(), String> {
    let serial = query(&["-s", &prepared.device, "shell", "getprop", "ro.serialno"])?;
    let user = query(&["-s", &prepared.device, "shell", "am", "get-current-user"])?;
    if serial.trim() != prepared.identity || user.trim() != prepared.user { return Err("The phone identity or Android user changed.".into()); }
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Change { Applied, Deferred, Manual }

fn set_power(prepared: &Prepared, control: &mut screen_power_control::Control, target: Panel, cleanup: bool, cancelled: &mut impl FnMut() -> bool) -> Result<Change, String> {
    change_power(prepared, target, cleanup, &mut |args| prepared.client.query(args, cancelled, false), &mut |on| control.power(on))
}

fn change_power(prepared: &Prepared, target: Panel, cleanup: bool, query: &mut impl FnMut(&[&str]) -> Result<String, String>, write: &mut impl FnMut(bool) -> Result<(), String>) -> Result<Change, String> {
    verify_identity(prepared, query)?;
    if !cleanup {
        let windows = query(&["-s", &prepared.device, "shell", "dumpsys", "window", "visible"])?;
        if protected_screen::visible_protection(&windows) != Some(target == Panel::On) { return Ok(Change::Deferred); }
    }
    let power = query(&["-s", &prepared.device, "shell", POWER_QUERY])?;
    let display = query(&["-s", &prepared.device, "shell", DISPLAY_QUERY])?;
    let expected = if target == Panel::On { Panel::Off } else { Panel::On };
    if !awake(&power) || panel(&display) != Some(expected) { return Ok(Change::Manual); }
    write(target == Panel::On)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let dump = query(&["-s", &prepared.device, "shell", DISPLAY_QUERY])?;
        if panel(&dump) == Some(target) { return Ok(Change::Applied); }
        if Instant::now() >= deadline { return Err("The phone did not reach the requested display power state.".into()); }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn spawn(executable: &str, args: &[&str], prepared: &Prepared) -> Result<Child, String> {
    let mut command = Command::new(executable);
    command.args(args).env("ADB", &prepared.client.path).env("ADB_SERVER_SOCKET", &prepared.client.socket)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(test)]
    if std::env::var("SCRCPY_PLUS_TEST_VIDEO").as_deref() == Ok("1") {
        // SDL's dummy driver decodes and renders without opening a desktop window.
        // --no-window alone disables video, so it cannot detect a frozen stream.
        command.args(["--render-driver=software", "--max-fps=30", "--print-fps"])
            .env("SDL_VIDEODRIVER", "dummy").env("SDL_AUDIODRIVER", "dummy").stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let child = command.spawn().map_err(|_| "Could not launch the selected scrcpy executable.")?;
    #[cfg(test)]
    let mut child = child;
    #[cfg(test)]
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::{ BufRead, BufReader };
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.contains("fps") || line.contains("ERROR") || line.contains("Renderer:") || line.contains("Texture:") { println!("Offscreen video: {}", line); }
                if let Some(fps) = line.strip_prefix("INFO: ").and_then(|line| line.split_once(" fps")).and_then(|(fps, _)| fps.parse::<u32>().ok()) {
                    VIDEO_FRAMES.lock().unwrap().push((Instant::now(), fps));
                }
            }
        });
    }
    #[cfg(test)]
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::{ BufRead, BufReader };
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.contains("ERROR") { println!("Offscreen video: {}", line); }
            }
        });
    }
    Ok(child)
}

pub fn run(executable: &str, args: Vec<&str>) -> Result<String, String> {
    run_session(executable, args, None)
}

fn run_session(executable: &str, args: Vec<&str>, limit: Option<Duration>) -> Result<String, String> {
    let preparing = Preparation::begin()?;
    let prepared = prepare(executable, &args)?;
    let registration = Registration::new(&prepared.device, &prepared.identity)?;
    let mut control = screen_power_control::Control::start(executable, &prepared.client, &prepared.device, &prepared.identity, &mut || SHUTTING_DOWN.load(Ordering::SeqCst))?;
    let mut child = spawn(executable, &args, &prepared)?;
    drop(preparing);
    let mut policy = Policy::default();
    let started = Instant::now();
    let mut next = Instant::now() + Duration::from_millis(300);
    let mut failures = 0;
    loop {
        if limit.is_some_and(|limit| started.elapsed() >= limit) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(String::new());
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(_) => { let _ = child.kill(); let _ = child.wait(); return Err("Could not inspect the mirroring session.".into()); },
        };
        if let Some(status) = status {
            return if status.success() { Ok(String::new()) } else { Err("The mirroring session ended unsuccessfully.".into()) };
        }
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            // The GUI can close while the original mirror remains open. Relinquish
            // only a temporary On state we still own; never stop a shared server.
            if policy.owned_on && !policy.paused {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut cancelled = || Instant::now() >= deadline || child.try_wait().ok().flatten().is_some();
                let _ = set_power(&prepared, &mut control, Panel::Off, true, &mut cancelled);
            }
            return Ok(String::new());
        }
        match registration.control.swap(0, Ordering::SeqCst) {
            1 => { policy.pause(); registration.update("paused", "requested"); },
            2 => { policy = Policy { ready: true, ..Policy::default() }; registration.update("monitoring", "resumed"); },
            _ => {},
        }
        if policy.paused || Instant::now() < next {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        next = Instant::now() + Duration::from_millis(450);
        let mut cancelled = || SHUTTING_DOWN.load(Ordering::SeqCst) || registration.control.load(Ordering::SeqCst) == 1
            || child.try_wait().map_or(true, |status| status.is_some());
        let observed = (|| -> Result<_, String> {
            let windows = prepared.client.query(&["-s", &prepared.device, "shell", "dumpsys", "window", "visible"], &mut cancelled, false)?;
            let power = prepared.client.query(&["-s", &prepared.device, "shell", POWER_QUERY], &mut cancelled, false)?;
            let display = prepared.client.query(&["-s", &prepared.device, "shell", DISPLAY_QUERY], &mut cancelled, false)?;
            Ok((protected_screen::visible_protection(&windows), awake(&power), panel(&display)))
        })();
        if cancelled() { continue; }
        match observed {
            Ok((protected, is_awake, Some(current))) => {
                let target = policy.decide(protected, current, is_awake, Instant::now());
                if policy.paused { registration.update("paused", "manual_override"); continue; }
                if !policy.ready {
                    if started.elapsed() > Duration::from_secs(10) {
                        policy.pause(); registration.update("paused", "screen_off_unavailable");
                    }
                    continue;
                }
                if protected.is_none() {
                    failures += 1;
                    if failures >= 3 { policy.pause(); registration.update("error", "state_unavailable"); }
                    continue;
                }
                failures = 0;
                if let Some(target) = target {
                    if registration.control.load(Ordering::SeqCst) != 0 { continue; }
                    match set_power(&prepared, &mut control, target, false, &mut cancelled) {
                        Ok(Change::Applied) => policy.applied(target),
                        Ok(Change::Deferred) => { policy.clear_since = None; continue; },
                        Ok(Change::Manual) => { policy.pause(); registration.update("paused", "manual_override"); continue; },
                        Err(_) if !cancelled() => { policy.pause(); registration.update("error", "power_failed"); continue; },
                        Err(_) => continue,
                    }
                }
                if policy.ready { registration.update(if policy.owned_on { "screen_on" } else { "monitoring" }, if protected == Some(true) { "protected" } else { "watching" }); }
            },
            _ => {
                policy.clear_since = None;
                failures += 1;
                if failures >= 3 { policy.pause(); registration.update("error", "state_unavailable"); }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enters_immediately_and_leaves_after_stable_clear_observations() {
        let now = Instant::now();
        let mut policy = Policy::default();
        assert_eq!(policy.decide(Some(true), Panel::Asleep, false, now), None);
        assert!(!policy.paused);
        assert_eq!(policy.decide(Some(true), Panel::On, true, now), None);
        assert!(!policy.ready);
        assert_eq!(policy.decide(Some(true), Panel::Off, true, now), Some(Panel::On));
        policy.applied(Panel::On);
        assert_eq!(policy.decide(Some(false), Panel::On, true, now), None);
        assert_eq!(policy.decide(None, Panel::On, true, now + Duration::from_secs(1)), None);
        assert_eq!(policy.decide(Some(false), Panel::On, true, now + Duration::from_secs(2)), None);
        assert_eq!(policy.decide(Some(true), Panel::On, true, now + Duration::from_millis(2500)), None);
        assert_eq!(policy.decide(Some(false), Panel::On, true, now + Duration::from_secs(3)), None);
        assert_eq!(policy.decide(Some(false), Panel::On, true, now + Duration::from_secs(4)), Some(Panel::Off));
        policy.applied(Panel::Off);
        assert_eq!(policy.decide(None, Panel::Off, true, now + Duration::from_secs(5)), None);
    }

    #[test]
    fn manual_changes_and_sleep_suspend_control_until_explicit_resume() {
        let now = Instant::now();
        let mut policy = Policy::default();
        policy.decide(Some(false), Panel::Off, true, now);
        assert_eq!(policy.decide(Some(true), Panel::On, true, now), None);
        assert_eq!(policy.decide(Some(true), Panel::On, true, now), None);
        assert!(policy.paused);
        assert_eq!(policy.decide(Some(true), Panel::Off, true, now), None);
        let mut resumed = Policy { ready: true, ..Policy::default() };
        assert_eq!(resumed.decide(Some(false), Panel::On, true, now), None);
        assert_eq!(resumed.decide(Some(false), Panel::On, true, now + Duration::from_secs(1)), Some(Panel::Off));
        resumed.applied(Panel::Off);
        resumed.decide(Some(true), Panel::Off, false, now);
        assert!(resumed.paused);
        assert!(!resumed.owned_on);
    }

    #[test]
    fn validates_the_selected_main_display_session_without_changing_flags() {
        let valid = ["-s", "synthetic-device", "--turn-screen-off", "--display-id=0"];
        assert_eq!(selector(&valid).unwrap(), "synthetic-device");
        for forbidden in ["--no-control", "--otg", "--no-cleanup", "--no-power-on", "--new-display=800x600", "--display-id=1", "--video-source=camera", "--no-video", "--tcpip=example", "--select-usb", "-e", "-sother-phone", "-Sn"] {
            let mut args = valid.to_vec(); args.push(forbidden);
            assert!(selector(&args).is_err());
        }
        assert!(selector(&["-s", "synthetic-device"]).is_err());
        assert!(selector(&["-s", "a", "-s", "b", "-S"]).is_err());
        assert!(selector(&["--serial=", "-S"]).is_err());
    }

    #[test]
    fn reads_physical_panel_state_without_confusing_logical_power() {
        let physical = "Display 123 (HWC display 0):\n    powerMode=Off\n  mScreenState=ON\n";
        assert_eq!(panel(physical), Some(Panel::Off));
        assert_eq!(panel(&physical.replace("powerMode=Off", "powerMode=On")), Some(Panel::On));
        assert_eq!(panel(&physical.replace("powerMode=Off", "powerMode=Doze")), Some(Panel::Asleep));
        assert_eq!(panel(&(physical.to_string() + physical)), None);
        assert_eq!(panel(&(physical.to_string() + "Virtual Display 456:\n  powerMode=On\n")), Some(Panel::Off));
        assert_eq!(panel("mScreenState=ON"), None);
        assert!(awake("  mWakefulness=Awake\n"));
        assert!(!awake("  mWakefulness=Dozing\n"));
    }

    #[test]
    fn one_controller_owns_each_physical_phone_across_connection_routes() {
        let first = Registration::new("synthetic-ip-route", "synthetic-physical-phone").unwrap();
        assert!(Registration::new("synthetic-mdns-route", "synthetic-physical-phone").is_err());
        control(first.id, true).unwrap();
        assert_eq!(first.control.load(Ordering::SeqCst), 1);
        control(first.id, false).unwrap();
        assert_eq!(first.control.load(Ordering::SeqCst), 2);
        let id = first.id;
        drop(first);
        assert!(control(id, false).is_err());
        let second = Registration::new("synthetic-mdns-route", "synthetic-physical-phone").unwrap();
        drop(second);
    }

    fn fixture() -> Prepared {
        Prepared { device: "synthetic-route".into(), identity: "synthetic-phone".into(), user: "0".into(),
            client: AdbClient { path: "unused".into(), socket: "unused".into() } }
    }

    fn windows(secure: bool) -> String {
        format!("WINDOW MANAGER WINDOWS\n Window #0 Window{{abc u0 example.app/Screen}}:\n mDisplayId=0 mSession=example\n mAttrs={{ty=BASE_APPLICATION fl={}}}\n mHasSurface=true isReadyForDisplay()=true\n isVisible=true\n", if secure { "SECURE" } else { "HARDWARE_ACCELERATED" })
    }

    #[test]
    fn rechecks_identity_fresh_protection_and_manual_power_before_writing() {
        let prepared = fixture();
        for (serial, secure, physical, expected) in [
            ("wrong-phone", false, "On", None),
            ("synthetic-phone", true, "On", Some(Change::Deferred)),
            ("synthetic-phone", false, "Off", Some(Change::Manual)),
        ] {
            let mut writes = 0;
            let result = change_power(&prepared, Panel::Off, false, &mut |args| {
                Ok(match &args[3..] {
                    ["getprop", "ro.serialno"] => serial.into(),
                    ["am", "get-current-user"] => "0".into(),
                    ["dumpsys", "window", "visible"] => windows(secure),
                    [POWER_QUERY] => "mWakefulness=Awake".into(),
                    [DISPLAY_QUERY] => format!("Display 123:\n powerMode={}", physical),
                    _ => panic!("Unexpected query"),
                })
            }, &mut |_| { writes += 1; Ok(()) });
            assert_eq!(result.ok(), expected);
            assert_eq!(writes, 0);
        }
        assert!(change_power(&prepared, Panel::On, false, &mut |_| Err("cancelled".into()), &mut |_| panic!("Must not write after cancellation")).is_err());
    }

    #[test]
    fn uses_one_scrcpy_power_command_and_verifies_the_actual_panel() {
        let prepared = fixture();
        let mut writes = Vec::new();
        let physical = std::cell::Cell::new("Off");
        let changed = change_power(&prepared, Panel::On, false, &mut |args| {
            Ok(match &args[3..] {
                ["getprop", "ro.serialno"] => "synthetic-phone".into(),
                ["am", "get-current-user"] => "0".into(),
                ["dumpsys", "window", "visible"] => windows(true),
                [POWER_QUERY] => "mWakefulness=Awake".into(),
                [DISPLAY_QUERY] => format!("Display 123:\n powerMode={}", physical.get()),
                _ => panic!("Unexpected command"),
            })
        }, &mut |on| { writes.push(on); physical.set(if on { "On" } else { "Off" }); Ok(()) }).unwrap();
        assert_eq!(changed, Change::Applied);
        assert_eq!(writes, vec![true]);
    }

    #[test]
    #[ignore = "Internal subprocess fixture; does nothing without the fixture marker"]
    fn sleeping_subprocess_fixture() {
        if std::env::args().any(|argument| argument == "SCRCPY_PLUS_SLEEP_FIXTURE") {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn cancellation_terminates_only_the_owned_query_process_promptly() {
        let client = AdbClient { path: std::env::current_exe().unwrap().to_string_lossy().to_string(), socket: "tcp:127.0.0.1:1".into() };
        let start = Instant::now();
        let result = client.query(&["--ignored", "--exact", "screen_power::tests::sleeping_subprocess_fixture", "--skip", "SCRCPY_PLUS_SLEEP_FIXTURE"],
            &mut || start.elapsed() > Duration::from_millis(150), false);
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[ignore = "Requires an explicitly selected phone; performs a bounded physical display power cycle"]
    fn live_display_power_cycle() {
        let adb = std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap();
        let scrcpy = std::env::var("SCRCPY_PLUS_TEST_SCRCPY").unwrap();
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").unwrap();
        system_command::set_custom_paths(Some(scrcpy.clone()), Some(adb));
        let _session = adb_server::begin(true).unwrap();
        let video = std::env::var("SCRCPY_PLUS_TEST_VIDEO").as_deref() == Ok("1");
        let mut args = vec!["-s", device.as_str(), "--turn-screen-off", "--no-audio", "--no-clipboard-autosync"];
        if !video { args.push("--no-window"); }
        let prepared = prepare(&scrcpy, &args).unwrap();
        let mut never_cancel = || false;
        let mut control = screen_power_control::Control::start(&scrcpy, &prepared.client, &device, &prepared.identity, &mut never_cancel).unwrap();
        let original = prepared.client.query(&["-s", &device, "shell", POWER_QUERY], &mut never_cancel, false).unwrap();
        let originally_awake = awake(&original);
        let mut child = spawn(&scrcpy, &args, &prepared).unwrap();
        let result = (|| -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(12);
            loop {
                if child.try_wait().unwrap().is_some() { return Err("The test mirror exited early.".into()); }
                let power = prepared.client.query(&["-s", &device, "shell", POWER_QUERY], &mut never_cancel, false)?;
                let display = prepared.client.query(&["-s", &device, "shell", DISPLAY_QUERY], &mut never_cancel, false)?;
                if awake(&power) && panel(&display) == Some(Panel::Off) { break; }
                if Instant::now() >= deadline { return Err("The headless mirror did not establish the screen-off baseline.".into()); }
                std::thread::sleep(Duration::from_millis(100));
            }
            for cycle in 0..3 {
                for target in [Panel::On, Panel::Off] {
                    if set_power(&prepared, &mut control, target, true, &mut never_cancel)? != Change::Applied {
                        return Err("The test panel state changed outside the controller.".into());
                    }
                    let changed = Instant::now();
                    if video {
                        // Only normal Settings/Home transitions: no protected content,
                        // credentials, screenshots, recordings, or desktop capture.
                        for _ in 0..3 {
                            prepared.client.query(&["-s", &device, "shell", "am", "start", "-a", "android.settings.SETTINGS"], &mut never_cancel, false)?;
                            std::thread::sleep(Duration::from_millis(650));
                            prepared.client.query(&["-s", &device, "shell", "input", "keyevent", "3"], &mut never_cancel, false)?;
                            std::thread::sleep(Duration::from_millis(650));
                        }
                        if child.try_wait().unwrap().is_some() { return Err("The video mirror exited during a display switch.".into()); }
                        let fps = VIDEO_FRAMES.lock().unwrap().iter().filter(|(time, _)| *time > changed + Duration::from_secs(2)).map(|(_, fps)| *fps).max().unwrap_or_default();
                        if fps == 0 { return Err(format!("No decoded/rendered frames after {:?} in cycle {}", target, cycle + 1)); }
                        println!("Cycle {} {:?}: video continued (peak {} fps).", cycle + 1, target, fps);
                    }
                }
            }
            Ok(())
        })();
        let _ = child.kill(); let _ = child.wait();
        std::thread::sleep(Duration::from_millis(700));
        if !originally_awake {
            let _ = prepared.client.query(&["-s", &device, "shell", "input", "keyevent", "223"], &mut never_cancel, false);
        }
        result.unwrap();
    }

    #[test]
    #[ignore = "Requires an explicitly selected phone; runs the real automatic controller in a headless mirror"]
    fn live_automatic_session() {
        let adb = std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap();
        let scrcpy = std::env::var("SCRCPY_PLUS_TEST_SCRCPY").unwrap();
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").unwrap();
        let seconds = std::env::var("SCRCPY_PLUS_TEST_SCREEN_SECONDS").unwrap_or("60".into()).parse::<u64>().unwrap().clamp(10, 180);
        system_command::set_custom_paths(Some(scrcpy.clone()), Some(adb));
        let _session = adb_server::begin(true).unwrap();
        let return_home = std::env::var("SCRCPY_PLUS_TEST_RETURN_HOME").as_deref() == Ok("1");
        let cancel_biometric = std::env::var("SCRCPY_PLUS_TEST_CANCEL_BIOMETRIC").as_deref() == Ok("1");
        let video = std::env::var("SCRCPY_PLUS_TEST_VIDEO").as_deref() == Ok("1");
        let test_device = device.clone();
        let done = Arc::new(AtomicBool::new(false));
        let watching = done.clone();
        let watcher = std::thread::spawn(move || {
            let mut previous = String::new();
            let mut turned_on = false;
            let mut turned_off = false;
            let mut sent_home = false;
            let mut sent_back = false;
            let mut video_after_exit = None;
            while !watching.load(Ordering::SeqCst) {
                for status in statuses() {
                    let state = format!("{}:{}", status.state, status.reason);
                    if state != previous { println!("Screen controller: {}", state); previous = state; }
                    if status.state == "screen_on" {
                        turned_on = true;
                        if return_home && !sent_home {
                            let client = AdbClient::capture().unwrap();
                            let mut cancelled = || false;
                            let focused = client.query(&["-s", &test_device, "shell", "dumpsys", "window", "displays"], &mut cancelled, false).unwrap();
                            let authenticator = focused.lines().any(|line| line.trim_start().starts_with("mCurrentFocus=") && line.contains("com.google.android.apps.authenticator2/"));
                            assert!(authenticator, "The opt-in Home transition requires Authenticator to be focused");
                            client.query(&["-s", &test_device, "shell", "input", "keyevent", "3"], &mut cancelled, false).unwrap();
                            println!("Test returned Authenticator to the Home screen.");
                            sent_home = true;
                        }
                        if cancel_biometric && !sent_back {
                            let client = AdbClient::capture().unwrap();
                            let mut cancelled = || false;
                            let windows = client.query(&["-s", &test_device, "shell", "dumpsys", "window", "visible"], &mut cancelled, false).unwrap();
                            assert!(protected_screen::visible_biometric(&windows), "The opt-in Back transition requires a visible SystemUI biometric prompt");
                            client.query(&["-s", &test_device, "shell", "input", "keyevent", "4"], &mut cancelled, false).unwrap();
                            println!("Test cancelled the visible fingerprint prompt without authenticating.");
                            sent_back = true;
                        }
                    }
                    if turned_on && status.state == "monitoring" {
                        turned_off = true;
                        if video && video_after_exit.is_none() {
                            video_after_exit = Some(Instant::now());
                            let client = AdbClient::capture().unwrap();
                            for _ in 0..3 {
                                client.query(&["-s", &test_device, "shell", "am", "start", "-a", "android.settings.SETTINGS"], &mut || false, false).unwrap();
                                std::thread::sleep(Duration::from_millis(650));
                                client.query(&["-s", &test_device, "shell", "input", "keyevent", "3"], &mut || false, false).unwrap();
                                std::thread::sleep(Duration::from_millis(650));
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            (turned_on, turned_off, video_after_exit)
        });
        let limit = format!("--time-limit={}", seconds);
        let mut args = vec!["-s", device.as_str(), "--turn-screen-off", "--no-audio", "--no-clipboard-autosync", &limit];
        if std::env::var("SCRCPY_PLUS_TEST_VIDEO").as_deref() != Ok("1") { args.push("--no-window"); }
        let result = run_session(&scrcpy, args, Some(Duration::from_secs(seconds + 5)));
        done.store(true, Ordering::SeqCst);
        let (turned_on, turned_off, video_after_exit) = watcher.join().unwrap();
        result.unwrap();
        assert!(idle());
        if return_home || cancel_biometric { assert!(turned_on && turned_off, "The real protected-entry and clear-exit transitions must both be observed"); }
        if let Some(exit) = video_after_exit {
            assert!(VIDEO_FRAMES.lock().unwrap().iter().any(|(time, fps)| *time > exit + Duration::from_secs(2) && *fps > 0), "Video must continue after the automatic protected-screen exit");
            println!("Verified decoded/rendered video after the automatic protected-screen exit.");
        }
    }
}
