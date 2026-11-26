use serde::Serialize;
use std::process::Child;
use std::sync::atomic::Ordering;
use std::time::{ Duration, Instant };
use zeroize::Zeroizing;
use crate::{ adb_server, device_inventory::Selection, screen_power::{ self, Monitor, Panel, Registration }, unlock_detection::{ self, Snapshot, Target }, unlock_store::{ self, Binding, Metadata, Secret } };

#[derive(Serialize)]
pub struct Observation {
    method: String,
    user: Option<u32>,
    supported: bool,
    enrolled: bool,
    enabled: bool,
    blocked: bool,
    active: bool,
    state: String,
    reason: String,
}

fn observation(target: &Target, method: String) -> Result<Observation, String> {
    let metadata = if unlock_store::supported() { unlock_store::metadata(&target.binding) } else { Ok(None) };
    let invalid = metadata.is_err();
    let metadata = metadata.ok().flatten();
    let session = screen_power::status_for(&target.binding.identity);
    let enabled = metadata.as_ref().is_some_and(|record| record.enabled);
    let invalidated = metadata.as_ref().is_some_and(|record| record.invalidated);
    let in_flight = session.as_ref().and_then(|status| status.unlock_state) == Some("unlocking");
    let blocked = invalid || (!in_flight && metadata.as_ref().is_some_and(|record| record.blocked));
    Ok(Observation {
        supported: unlock_store::supported() && target.sdk >= 35 && target.binding.user == 0 && method == "password",
        method, user: Some(target.binding.user), enrolled: invalid || metadata.is_some(), enabled, blocked,
        active: session.is_some(),
        state: session.as_ref().and_then(|status| status.unlock_state).unwrap_or(if blocked { "blocked" } else if enabled { "enabled" } else { "disabled" }).into(),
        reason: if invalidated { "method_changed".into() } else { session.and_then(|status| status.unlock_reason).unwrap_or(if invalid { "credential_record_unavailable" } else if blocked { "unverified_attempt" } else { "not_monitoring" }).into() },
    })
}

pub fn inspect(selection: &Selection) -> Result<Observation, String> {
    let _operation = adb_server::begin(false)?;
    let target = unlock_detection::target(selection)?;
    let method = unlock_detection::configured(&target, &mut screen_power::shutting_down)?;
    observation(&target, method)
}

fn editable(target: &Target) -> Result<(), String> {
    if screen_power::status_for(&target.binding.identity).is_some() { return Err("Stop this phone's session before changing automatic unlock.".into()); }
    Ok(())
}

pub fn enroll(selection: &Selection, password: String, confirmation: String) -> Result<Observation, String> {
    let password = Zeroizing::new(password);
    let confirmation = Zeroizing::new(confirmation);
    if *password != *confirmation { return Err("The passwords do not match.".into()); }
    if !(4..=64).contains(&password.len()) || !password.bytes().all(|value| (32..=126).contains(&value)) {
        return Err("Use 4–64 printable ASCII characters.".into());
    }
    let _operation = adb_server::begin(false)?;
    let target = unlock_detection::target(selection)?;
    editable(&target)?;
    let method = unlock_detection::configured(&target, &mut screen_power::shutting_down)?;
    if method != "password" || target.sdk < 35 || target.binding.user != 0 || !unlock_store::supported() {
        return Err("Automatic unlock currently supports an owner password on Windows with Android 15 or later.".into());
    }
    unlock_store::enroll(&target.binding, &password)?;
    observation(&target, method)
}

pub fn set_enabled(selection: &Selection, enabled: bool) -> Result<Observation, String> {
    let _operation = adb_server::begin(false)?;
    let target = unlock_detection::target(selection)?;
    editable(&target)?;
    let method = unlock_detection::configured(&target, &mut screen_power::shutting_down)?;
    if enabled && (method != "password" || target.sdk < 35 || target.binding.user != 0) { return Err("The enrolled password method could not be verified.".into()); }
    unlock_store::set_enabled(&target.binding, enabled)?;
    observation(&target, method)
}

pub fn remove(selection: &Selection) -> Result<Observation, String> {
    let _operation = adb_server::begin(false)?;
    let target = unlock_detection::target(selection)?;
    editable(&target)?;
    unlock_store::remove(&target.binding)?;
    let method = unlock_detection::configured(&target, &mut screen_power::shutting_down)?;
    observation(&target, method)
}

pub fn stop(selection: &Selection) -> Result<Observation, String> {
    // Host-only cancellation must remain available after a transport disappears.
    let identity = selection.identity.as_ref().ok_or("This device identity is unavailable.")?;
    screen_power::stop(identity)?;
    Ok(Observation { method: "unknown".into(), user: None, supported: false, enrolled: true, enabled: true, blocked: false,
        active: true, state: "stopping".into(), reason: "requested".into() })
}

pub fn enabled(selection: Option<&Selection>) -> Result<Option<Target>, String> {
    let Some(selection) = selection.filter(|selection| selection.identity.is_some()) else { return Ok(None); };
    if !unlock_store::supported() { return Ok(None); }
    // Only enrolled/enabled owner records need unlock-specific device queries.
    // Optional detection must not prevent an ordinary manual mirror from opening.
    let binding = Binding { identity: selection.identity.clone().unwrap(), user: 0 };
    if !unlock_store::metadata(&binding)?.is_some_and(|record| record.enabled) { return Ok(None); }
    let target = unlock_detection::target(selection)?;
    Ok(unlock_store::metadata(&target.binding)?.is_some_and(|record| record.enabled).then_some(target))
}

fn validate_args(args: &[&str]) -> Result<(), String> {
    // Deliberately allow only the existing GUI's main-display options. Unknown
    // flags could create an additional capture/control sink or change cleanup.
    let switches = ["--turn-screen-off", "--stay-awake", "--show-touches", "--fullscreen", "--always-on-top", "--window-borderless", "--disable-screensaver", "--power-off-on-close", "--capture-orientation=@", "--no-audio"];
    let values = ["--window-title=", "--crop=", "--video-bit-rate=", "--max-size=", "--max-fps=", "--window-x=", "--window-y=", "--window-width=", "--window-height=", "--display-id=0"];
    if args.first() != Some(&"-s") || args.len() < 3 || !args.contains(&"--turn-screen-off") { return Err("Enable Turn off phone screen for private automatic unlock.".into()); }
    if args[2..].iter().any(|arg| !switches.contains(arg) && !values.iter().any(|prefix| if *prefix == "--display-id=0" { arg == prefix } else { arg.starts_with(prefix) })) {
        return Err("Automatic unlock requires the standard main-display options. Disable recording, capture sinks, custom flags, and unsupported display/control options.".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome { Unlocked, AlreadyUnlocked, Deferred, Blocked }

trait Replay {
    fn observe(&mut self) -> Result<Snapshot, String>;
    fn power_off(&mut self) -> Result<(), String>;
    fn key(&mut self, key: u32) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
    fn prepare_editor(&mut self, alternate: bool) -> Result<(), String>;
    fn mark(&mut self, blocked: bool) -> Result<(), String>;
    fn password(&mut self) -> Result<Secret, String>;
    fn character(&mut self, character: u8) -> Result<(), String>;
    fn wait(&mut self, milliseconds: u64) -> bool;
}

fn ready(snapshot: &Snapshot, focus: Option<&str>) -> bool {
    snapshot.method == "password" && snapshot.safe_locked && snapshot.awake && snapshot.password_editor
        && snapshot.panel == Some(Panel::Off) && snapshot.focus.is_some()
        && focus.is_none_or(|focus| snapshot.focus.as_deref() == Some(focus))
}

fn replay(device: &mut impl Replay) -> Result<Outcome, String> {
    let initial = device.observe()?;
    if initial.unlocked { return Ok(Outcome::AlreadyUnlocked); }
    if initial.method != "password" || !initial.safe_locked { return Ok(Outcome::Deferred); }
    if !initial.awake { device.key(224)?; }
    device.power_off()?;
    device.prepare_editor(false)?;
    let mut prior = None;
    let mut approved = None;
    let mut alternate_prepared = false;
    for observation in 0..20 {
        // The first observation can run immediately. Approval still requires
        // two matching editor observations separated by the settling interval.
        if !device.wait(if observation == 0 { 0 } else { 150 }) { return Ok(Outcome::Deferred); }
        let snapshot = device.observe()?;
        if snapshot.unlocked { return Ok(Outcome::AlreadyUnlocked); }
        if snapshot.method != "password" || !snapshot.safe_locked { return Ok(Outcome::Deferred); }
        if snapshot.alternate && !alternate_prepared {
            alternate_prepared = true;
            device.prepare_editor(true)?;
        }
        if ready(&snapshot, prior.as_deref()) {
            if prior.is_some() { approved = snapshot.focus; break; }
            prior = snapshot.focus;
        } else { prior = if ready(&snapshot, None) { snapshot.focus } else { None }; }
    }
    let Some(focus) = approved else { return Ok(Outcome::Deferred); };
    // Clear a pre-existing partial field only on the same verified editor. This
    // happens before loading the password or claiming an input attempt.
    if !ready(&device.observe()?, Some(&focus)) { return Ok(Outcome::Deferred); }
    device.clear()?;
    if !ready(&device.observe()?, Some(&focus)) { return Ok(Outcome::Deferred); }
    device.mark(true)?;
    let password = match device.password() {
        Ok(password) => password,
        Err(error) => { device.mark(false)?; return Err(error); },
    };
    // Once any character write can have started, all failures leave the
    // persistent block in place. A transport write is not proof of Android input.
    let attempt = (|| -> Result<bool, String> {
        for character in password.value.bytes() {
            if !ready(&device.observe()?, Some(&focus)) { return Ok(false); }
            device.character(character)?;
        }
        if !ready(&device.observe()?, Some(&focus)) { return Ok(false); }
        device.key(66)?;
        for observation in 0..13 {
            // A freshly verified unlock needs no artificial minimum delay.
            // If it is not ready, retain the twelve bounded settling waits.
            if !device.wait(if observation == 0 { 0 } else { 350 }) { return Ok(false); }
            if device.observe()?.unlocked { return Ok(true); }
        }
        Ok(false)
    })();
    drop(password);
    match attempt {
        Ok(true) => { device.mark(false)?; Ok(Outcome::Unlocked) },
        _ => Ok(Outcome::Blocked),
    }
}

struct Device<'a, F: FnMut() -> bool> {
    target: &'a Target,
    monitor: &'a mut Monitor,
    metadata: Metadata,
    cancelled: &'a mut F,
    registration: &'a Registration,
}

impl<F: FnMut() -> bool> Device<'_, F> {
    fn validate_enrollment(&mut self) -> Result<(), String> {
        if (self.cancelled)() { return Err("Unlock cancelled.".into()); }
        let current = unlock_store::metadata(&self.target.binding)?.ok_or("The saved credential is unavailable.")?;
        if current.generation != self.metadata.generation || !current.enabled { return Err("The enrolled credential changed.".into()); }
        Ok(())
    }
}

fn prepare_password_view(target: &Target, alternate: bool, cancelled: &mut impl FnMut() -> bool) -> Result<(), String> {
    let snapshot = unlock_detection::snapshot(target, cancelled)?;
    if snapshot.unlocked { return Ok(()); }
    if !snapshot.safe_locked || !snapshot.awake || snapshot.method != "password" || snapshot.panel != Some(Panel::Off) {
        return Err(format!("The password screen is unavailable: locked={}, awake={}, method={}, panel={:?}.", snapshot.safe_locked, snapshot.awake, snapshot.method, snapshot.panel));
    }
    if !alternate { unlock_detection::query(target, "wm dismiss-keyguard", cancelled)?; }
    else if snapshot.alternate {
        let size = unlock_detection::query(target, "wm size", cancelled)?;
        let dimensions = size.lines().filter_map(|line| line.strip_prefix("Physical size: ").or_else(|| line.strip_prefix("Override size: "))).last()
            .and_then(|value| value.split_once('x')).and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
            .filter(|(width, height)| (100..=10000).contains(width) && (100..=10000).contains(height)).ok_or("The main display size is unavailable.")?;
        let current = unlock_detection::snapshot(target, cancelled)?;
        if current.safe_locked && current.alternate && current.panel == Some(Panel::Off) {
            unlock_detection::query(target, &format!("input -d 0 tap {} {}", dimensions.0 / 2, dimensions.1 / 4), cancelled)?;
        }
    }
    Ok(())
}

impl<F: FnMut() -> bool> Replay for Device<'_, F> {
    fn observe(&mut self) -> Result<Snapshot, String> {
        self.validate_enrollment()?;
        unlock_detection::snapshot(self.target, self.cancelled)
    }
    fn power_off(&mut self) -> Result<(), String> {
        if (self.cancelled)() { return Err("Unlock cancelled.".into()); }
        self.monitor.channel.power(false)?;
        if !self.wait(300) { return Err("Unlock cancelled.".into()); }
        // The immediately following prepare_password_view verifies the full
        // awake, locked, password, and panel-off state before any editor input.
        Ok(())
    }
    fn key(&mut self, key: u32) -> Result<(), String> {
        if (self.cancelled)() { return Err("Unlock cancelled.".into()); }
        if key == 66 { crate::launch_progress::update("unlocking", "verifying_unlock"); }
        if key == 224 {
            // The shell's explicit main-display wake path is also used by the
            // proven helper. This command contains no credential data.
            unlock_detection::query(self.target, "input -d 0 keyevent 224", self.cancelled)?;
            return Ok(());
        }
        self.monitor.channel.key(key)
    }
    fn clear(&mut self) -> Result<(), String> {
        if (self.cancelled)() { return Err("Unlock cancelled.".into()); }
        self.monitor.channel.select_all()?;
        self.monitor.channel.key(67)
    }
    fn prepare_editor(&mut self, alternate: bool) -> Result<(), String> {
        self.validate_enrollment()?;
        prepare_password_view(self.target, alternate, self.cancelled)
    }
    fn mark(&mut self, blocked: bool) -> Result<(), String> {
        self.metadata = unlock_store::set_blocked(&self.target.binding, blocked)?;
        self.registration.unlock(if blocked { "unlocking" } else { "monitoring" }, if blocked { "private_entry" } else { "unlock_verified" });
        if blocked { crate::launch_progress::update("unlocking", "entering_password"); }
        Ok(())
    }
    fn password(&mut self) -> Result<Secret, String> { unlock_store::password(&self.metadata) }
    fn character(&mut self, character: u8) -> Result<(), String> {
        if (self.cancelled)() { return Err("Unlock cancelled.".into()); }
        self.monitor.channel.character(character)
    }
    fn wait(&mut self, milliseconds: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(milliseconds);
        while Instant::now() < deadline {
            if (self.cancelled)() { return false; }
            std::thread::sleep(Duration::from_millis(20));
        }
        !(self.cancelled)()
    }
}

struct Mirror(Option<Child>);

impl Mirror {
    fn close(&mut self) -> Result<(), String> {
        if let Some(child) = self.0.as_mut() {
            crate::launch_progress::mirror_closed();
            if child.try_wait().map_err(|_| "Could not inspect the owned mirror.")?.is_none() { child.kill().map_err(|_| "Could not close the mirror for private entry.")?; }
            child.wait().map_err(|_| "Could not verify that the mirror closed.")?;
            self.0 = None;
        }
        Ok(())
    }
}

impl Drop for Mirror { fn drop(&mut self) { let _ = self.close(); } }

pub fn run(executable: &str, args: Vec<&str>, target: Target, automatic_screen: bool) -> Result<String, String> {
    validate_args(&args)?;
    let registration = Registration::managed(&target.selection.serial, &target.binding.identity, automatic_screen)?;
    let _lease = unlock_store::attempt_lock(&target.binding)?;
    // Drop the diagnostic connection before releasing the phone's attempt lease.
    let mut target = target;
    crate::launch_progress::update("preparing", "preparing_screen_control");
    let mut monitor = Monitor::new_pinned(executable, &args, &target)?;
    let mut cancelled = || screen_power::shutting_down() || registration.control.load(Ordering::SeqCst) == 3;
    let diagnostics = unlock_detection::start_diagnostics(&mut target, &mut cancelled).is_ok();
    #[cfg(test)]
    if std::env::var("SCRCPY_PLUS_TEST_FAST_DIAGNOSTICS").as_deref() == Ok("1") { assert!(diagnostics, "The real unlock benchmark requires the new diagnostic backend; fallback cannot count as a speed result."); }
    let _ = diagnostics;
    let mut mirror = Mirror(None);
    let mut next_unlock = Instant::now();
    let mut next_power = Instant::now();
    let mut failures = 0u32;
    let mut private_block = unlock_store::metadata(&target.binding)?.is_some_and(|record| record.blocked);
    let mut pending_power = false;
    let mut next_restore = Instant::now();
    loop {
        let mut cancelled = || screen_power::shutting_down() || registration.control.load(Ordering::SeqCst) == 3;
        if cancelled() { return Ok(String::new()); }
        if let Some(child) = mirror.0.as_mut() {
            if let Some(status) = child.try_wait().map_err(|_| "Could not inspect the owned mirror.")? {
                mirror.0 = None;
                return if status.success() { Ok(String::new()) } else { Err("The mirroring session ended unsuccessfully.".into()) };
            }
        }
        if !pending_power && Instant::now() >= next_unlock {
            next_unlock = Instant::now() + Duration::from_secs(5);
            let metadata = unlock_store::metadata(&target.binding)?.ok_or("The saved credential is unavailable.")?;
            if !metadata.enabled { return Err("Automatic unlock was disabled. Start a new manual session.".into()); }
            match unlock_detection::snapshot(&target, &mut cancelled) {
                Ok(snapshot) if ["pin", "pattern", "none"].contains(&snapshot.method.as_str()) => {
                    unlock_store::invalidate(&target.binding)?;
                    return Err("The device unlock method changed. Replace the saved password before enabling automatic unlock again.".into());
                },
                Ok(snapshot) if snapshot.unlocked => {
                    if metadata.blocked { unlock_store::set_blocked(&target.binding, false)?; }
                    if private_block {
                        pending_power = true;
                        private_block = false;
                    }
                    registration.unlock("monitoring", "watching");
                    crate::launch_progress::unlocked();
                    failures = 0;
                },
                Ok(snapshot) if metadata.blocked => {
                    registration.unlock("blocked", "unverified_attempt");
                    crate::launch_progress::update("manual_unlock", "automatic_unlock_blocked");
                    let _ = snapshot;
                },
                Ok(snapshot) if snapshot.safe_locked && snapshot.method == "password" => {
                    // The only mirror is closed and waited before any password
                    // retrieval. The remaining channel cannot capture video or
                    // audio and has clipboard synchronization disabled.
                    crate::launch_progress::update("unlocking", "preparing_private_entry");
                    if mirror.close().is_err() {
                        unlock_store::set_blocked(&target.binding, true)?;
                        return Err("Could not verify that the mirror closed. Automatic unlock is blocked; close the session and unlock manually.".into());
                    }
                    registration.unlock("preparing", "private_entry");
                    let mut device = Device { target: &target, monitor: &mut monitor, metadata, cancelled: &mut cancelled, registration: &registration };
                    let outcome = replay(&mut device);
                    match outcome {
                        Ok(Outcome::Unlocked | Outcome::AlreadyUnlocked) => {
                            pending_power = true;
                            failures = 0;
                            registration.unlock("monitoring", "unlock_verified");
                        },
                        Ok(Outcome::Blocked) => {
                            private_block = true;
                            registration.unlock("blocked", "unverified_attempt");
                            crate::launch_progress::update("manual_unlock", "automatic_unlock_blocked");
                        },
                        _ => {
                            failures = failures.saturating_add(1);
                            next_unlock = Instant::now() + retry_delay(failures);
                            registration.unlock("waiting", "password_editor_unavailable");
                            crate::launch_progress::update("manual_unlock", "password_editor_unavailable");
                        },
                    }
                },
                Ok(_) => {
                    registration.unlock("unavailable", "unsupported_or_changed_method");
                    crate::launch_progress::update("manual_unlock", "unsupported_unlock_state");
                },
                Err(_) => {
                    failures = failures.saturating_add(1);
                    next_unlock = Instant::now() + retry_delay(failures);
                    registration.unlock("waiting", "device_state_unavailable");
                    crate::launch_progress::update("manual_unlock", "device_state_unavailable");
                },
            }
        }
        if cancelled() { return Ok(String::new()); }
        if pending_power && Instant::now() >= next_restore {
            crate::launch_progress::update("unlocking", "restoring_screen_off");
            next_restore = Instant::now() + Duration::from_secs(3);
            let restored = (|| -> Result<bool, String> {
                let current = unlock_detection::snapshot(&target, &mut cancelled)?;
                if current.safe_locked && current.method == "password" {
                    // A verified new lock is a new authentication episode, not
                    // a retry of the password after a failed power command.
                    pending_power = false;
                    next_unlock = Instant::now();
                    registration.unlock("waiting", "new_lock_episode");
                    return Ok(false);
                }
                if !current.unlocked { return Ok(false); }
                monitor.channel.power(false)?;
                std::thread::sleep(Duration::from_millis(150));
                let after = unlock_detection::snapshot(&target, &mut cancelled)?;
                Ok(after.unlocked && after.panel == Some(Panel::Off))
            })().unwrap_or(false);
            if restored {
                pending_power = false; monitor.reset_after_unlock(); registration.unlock("monitoring", "unlock_verified");
                next_unlock = Instant::now() + Duration::from_secs(5);
                crate::launch_progress::unlocked();
            }
            else if pending_power {
                registration.unlock("waiting", "screen_off_unavailable");
                crate::launch_progress::update("manual_unlock", "screen_off_unavailable");
            }
            else { continue; }
        }
        if mirror.0.is_none() && !private_block && !pending_power {
            crate::device_inventory::validate(&target.selection, &args)?;
            mirror.0 = Some(screen_power::spawn_client(executable, &args, &target.client)?);
        }
        if automatic_screen && !private_block && !pending_power && Instant::now() >= next_power {
            next_power = Instant::now() + Duration::from_millis(450);
            monitor.tick(&registration, &mut cancelled);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn retry_delay(failures: u32) -> Duration { Duration::from_secs([5, 10, 20, 30][failures.saturating_sub(1).min(3) as usize]) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn snapshot() -> Snapshot {
        Snapshot { method: "password".into(), unlocked: false, safe_locked: true, awake: true, password_editor: true,
            focus: Some("synthetic password window".into()), alternate: false, panel: Some(Panel::Off) }
    }

    struct Fake { observations: VecDeque<Snapshot>, events: Vec<&'static str>, waits: Vec<u64>, blocked: bool, fail_write: bool, count: usize, change_after: Option<usize>, success: bool, missing: bool, cancel_after_input: bool }
    impl Default for Fake {
        fn default() -> Self { Self { observations: VecDeque::new(), events: Vec::new(), waits: Vec::new(), blocked: false, fail_write: false, count: 0, change_after: None, success: false, missing: false, cancel_after_input: false } }
    }
    impl Replay for Fake {
        fn observe(&mut self) -> Result<Snapshot, String> {
            if self.cancel_after_input && self.events.contains(&"character") { return Err("Unlock cancelled.".into()); }
            self.count += 1;
            let mut value = self.observations.pop_front().unwrap_or_else(snapshot);
            if self.change_after.is_some_and(|after| self.count > after) { value.focus = Some("different window".into()); }
            if self.success && self.events.contains(&"enter") { value.unlocked = true; value.safe_locked = false; }
            Ok(value)
        }
        fn power_off(&mut self) -> Result<(), String> { self.events.push("private_panel"); Ok(()) }
        fn key(&mut self, key: u32) -> Result<(), String> { if key == 66 { self.events.push("enter"); } Ok(()) }
        fn clear(&mut self) -> Result<(), String> { self.events.push("clear_editor"); Ok(()) }
        fn prepare_editor(&mut self, _: bool) -> Result<(), String> { self.events.push("prepare"); Ok(()) }
        fn mark(&mut self, blocked: bool) -> Result<(), String> { self.blocked = blocked; self.events.push(if blocked { "block" } else { "clear" }); Ok(()) }
        fn password(&mut self) -> Result<Secret, String> { self.events.push("secret"); if self.missing { Err("secret unavailable".into()) } else { Ok(Secret { value: "synthetic".into() }) } }
        fn character(&mut self, _: u8) -> Result<(), String> { self.events.push("character"); if self.fail_write { Err("transport uncertain".into()) } else { Ok(()) } }
        fn wait(&mut self, milliseconds: u64) -> bool { self.waits.push(milliseconds); !self.cancel_after_input || !self.events.contains(&"character") }
    }

    #[test]
    fn unlocked_and_nonpassword_states_never_retrieve_a_secret() {
        for kind in ["unlocked", "pin", "pattern", "none", "unknown"] {
            let mut value = snapshot();
            if kind == "unlocked" { value.unlocked = true; } else { value.method = kind.into(); }
            let mut fake = Fake::default(); fake.observations.push_back(value);
            assert_eq!(replay(&mut fake).unwrap(), if kind == "unlocked" { Outcome::AlreadyUnlocked } else { Outcome::Deferred });
            assert!(fake.events.is_empty());
        }
    }

    #[test]
    fn privacy_and_persistent_block_precede_secret_and_uncertain_write() {
        let mut fake = Fake { fail_write: true, ..Fake::default() };
        assert_eq!(replay(&mut fake).unwrap(), Outcome::Blocked);
        assert!(fake.blocked);
        assert_eq!(fake.events, ["private_panel", "prepare", "clear_editor", "block", "secret", "character"]);
        assert!(!fake.events.contains(&"enter"));
    }

    #[test]
    fn changed_editor_before_each_character_or_submission_keeps_the_attempt_blocked() {
        for delivered in 0..="synthetic".len() {
            let mut fake = Fake { change_after: Some(5 + delivered), ..Fake::default() };
            assert_eq!(replay(&mut fake).unwrap(), Outcome::Blocked);
            assert!(fake.blocked);
            assert_eq!(fake.events.iter().filter(|event| **event == "character").count(), delivered);
            assert!(!fake.events.contains(&"enter"));
        }
    }

    #[test]
    fn changed_editor_before_or_after_clear_prevents_secret_retrieval() {
        for changed_at in [4, 5] {
            let mut observations = (0..changed_at).map(|_| snapshot()).collect::<VecDeque<_>>();
            observations.back_mut().unwrap().focus = Some("different window".into());
            let mut fake = Fake { observations, ..Fake::default() };
            assert_eq!(replay(&mut fake).unwrap(), Outcome::Deferred);
            assert_eq!(fake.events.contains(&"clear_editor"), changed_at == 5);
            assert!(!fake.events.contains(&"secret") && !fake.events.contains(&"character"));
            assert!(!fake.blocked);
        }
    }

    #[test]
    fn recording_capture_and_selector_overrides_are_rejected() {
        let valid = ["-s", "synthetic", "--turn-screen-off", "--video-bit-rate=8M"];
        assert!(validate_args(&valid).is_ok());
        for flag in ["--record=private.mp4", "--v4l2-sink=/dev/video0", "--no-control", "--new-display", "--no-cleanup", "--serial=other", "--video-source=camera", "--verbosity=verbose"] {
            let mut args = valid.to_vec(); args.push(flag); assert!(validate_args(&args).is_err());
        }
    }

    #[test]
    fn transient_failures_use_bounded_backoff() {
        assert_eq!((1..=6).map(|count| retry_delay(count).as_secs()).collect::<Vec<_>>(), [5, 10, 20, 30, 30, 30]);
    }

    #[test]
    fn only_verified_success_clears_an_attempt_after_submission() {
        let mut fake = Fake { success: true, ..Fake::default() };
        assert_eq!(replay(&mut fake).unwrap(), Outcome::Unlocked);
        assert!(!fake.blocked);
        assert_eq!(fake.events.iter().filter(|event| **event == "enter").count(), 1);
        assert_eq!(fake.events.last(), Some(&"clear"));
        assert_eq!(fake.waits, [0, 150, 0], "Keep the stable-editor interval while accepting an immediately verified unlock.");
        let mut rejected = Fake::default();
        assert_eq!(replay(&mut rejected).unwrap(), Outcome::Blocked);
        assert!(rejected.blocked && !rejected.events.contains(&"clear"));
        assert_eq!(rejected.waits.iter().filter(|wait| **wait == 350).count(), 12, "An unverified result retains the complete bounded settling window.");
    }

    #[test]
    fn proven_zero_input_secret_failure_can_clear_the_attempt() {
        let mut fake = Fake { missing: true, ..Fake::default() };
        assert!(replay(&mut fake).is_err());
        assert!(!fake.blocked && !fake.events.contains(&"character"));
    }

    #[test]
    fn cancellation_after_first_character_stops_input_and_keeps_the_block() {
        let mut fake = Fake { cancel_after_input: true, ..Fake::default() };
        assert_eq!(replay(&mut fake).unwrap(), Outcome::Blocked);
        assert!(fake.blocked);
        assert_eq!(fake.events.iter().filter(|event| **event == "character").count(), 1);
        assert!(!fake.events.contains(&"enter"));
    }

    #[test]
    fn privacy_closes_only_the_owned_child_and_waits_for_its_exit() {
        let spawn = || {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--ignored", "--exact", "screen_power::tests::sleeping_subprocess_fixture", "--skip", "SCRCPY_PLUS_SLEEP_FIXTURE"])
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            #[cfg(windows)]
            { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
            command.spawn().unwrap()
        };
        let mut owned = Mirror(Some(spawn()));
        let mut unrelated = Mirror(Some(spawn()));
        std::thread::sleep(Duration::from_millis(50));
        owned.close().unwrap();
        assert!(owned.0.is_none());
        assert!(unrelated.0.as_mut().unwrap().try_wait().unwrap().is_none());
    }

    #[test]
    #[ignore = "Read-only test: requires explicitly selected ADB and one connected Pixel 7 Pro; never retrieves credentials or changes phone state"]
    fn live_detect_pixel_without_unlocking() {
        let target = live_target();
        let started = Instant::now();
        let method = unlock_detection::configured(&target, &mut || false).unwrap();
        assert_eq!(method, "password", "The owner's expected configured method changed; do not attempt input.");
        let snapshot = unlock_detection::snapshot(&target, &mut || false).unwrap();
        println!("Read-only Pixel detection: method={}, Android SDK={}, user={}, unlocked={}, safe_locked={}, awake={}, password_editor={}, alternate={}, elapsed_ms={}",
            method, target.sdk, target.binding.user, snapshot.unlocked, snapshot.safe_locked, snapshot.awake, snapshot.password_editor, snapshot.alternate, started.elapsed().as_millis());
        assert_eq!(snapshot.method, "password");
    }

    fn live_target() -> Target {
        let adb = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Select the test ADB explicitly.");
        let scrcpy = std::env::var("SCRCPY_PLUS_TEST_SCRCPY").expect("Select the test scrcpy explicitly.");
        crate::system_command::set_custom_paths(Some(scrcpy), Some(adb));
        assert!(adb_server::version(&adb_server::endpoint().unwrap()).unwrap().is_some(), "The existing ADB server must already be running.");
        let devices = crate::device_inventory::refresh().unwrap();
        let candidates = devices.iter().filter(|device| device.verified && device.routes.iter().any(|route| route.info.as_ref().is_some_and(|info| info.model == "Pixel 7 Pro"))).collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1, "Select exactly one connected Pixel 7 Pro.");
        let selection = &candidates[0].routes.iter().find(|route| route.state == "device" && route.selection.identity.is_some()).unwrap().selection;
        unlock_detection::target(selection).unwrap()
    }

    #[test]
    #[ignore = "Requires explicit Pixel testing authorization; prepares a private password editor and restores sleep/panel state, without retrieving or typing a credential"]
    fn live_prepare_private_editor_without_credential_input() {
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_PREPARE").as_deref(), Ok("1"));
        let target = live_target();
        let _lease = unlock_store::attempt_lock(&target.binding).unwrap();
        let initial = unlock_detection::snapshot(&target, &mut || false).unwrap();
        assert!(initial.safe_locked && initial.method == "password", "The phone must already be locked for this test.");
        let executable = std::env::var("SCRCPY_PLUS_TEST_SCRCPY").unwrap();
        let args = ["-s", target.selection.serial.as_str(), "--turn-screen-off"];
        let mut monitor = Monitor::new_pinned(&executable, &args, &target).unwrap();
        let result = (|| -> Result<(), String> {
            if !initial.awake { unlock_detection::query(&target, "input -d 0 keyevent 224", &mut || false)?; std::thread::sleep(Duration::from_millis(350)); }
            monitor.channel.power(false)?;
            std::thread::sleep(Duration::from_millis(350));
            prepare_password_view(&target, false, &mut || false)?;
            let mut previous = None;
            let mut alternate_prepared = false;
            for _ in 0..20 {
                let current = unlock_detection::snapshot(&target, &mut || false)?;
                if current.alternate && !alternate_prepared {
                    alternate_prepared = true;
                    prepare_password_view(&target, true, &mut || false)?;
                }
                if ready(&current, previous.as_deref()) {
                    if previous.is_some() { println!("Verified pinned control, physical panel Off, and stable current display-0 password editor. No credential retrieved or entered."); return Ok(()); }
                    previous = current.focus;
                } else { previous = None; }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err("The current password editor was not verified.".into())
        })();
        if !initial.awake { unlock_detection::query(&target, "input -d 0 keyevent 223", &mut || false).unwrap(); }
        else if initial.panel == Some(Panel::On) { monitor.channel.power(true).unwrap(); }
        else { monitor.channel.power(false).unwrap(); }
        std::thread::sleep(Duration::from_millis(350));
        let restored = unlock_detection::snapshot(&target, &mut || false).unwrap();
        assert_eq!(restored.awake, initial.awake, "Restore the original logical power state.");
        assert_eq!(restored.panel, initial.panel, "Restore the original physical panel state.");
        result.unwrap();
    }

    #[test]
    #[ignore = "Read-only live benchmark preflight; reads enrollment metadata only, never the saved password or phone input"]
    fn live_unlock_benchmark_preflight() {
        let target = live_target();
        let current = unlock_detection::snapshot(&target, &mut || false).unwrap();
        let metadata = unlock_store::metadata(&target.binding).unwrap();
        let ready = metadata.as_ref().is_some_and(|metadata| metadata.enabled && !metadata.blocked && !metadata.invalidated);
        let lease_available = unlock_store::attempt_lock(&target.binding).is_ok();
        println!("Unlock benchmark preflight: enrolled={}, enabled={}, blocked={}, invalidated={}, lease_available={}, safe_locked={}, awake={}, panel={:?}, sdk={}, user={}",
            metadata.is_some(), metadata.as_ref().is_some_and(|metadata| metadata.enabled), metadata.as_ref().is_some_and(|metadata| metadata.blocked),
            metadata.as_ref().is_some_and(|metadata| metadata.invalidated), lease_available, current.safe_locked, current.awake, current.panel, target.sdk, target.binding.user);
        assert!(ready, "A valid locally enrolled/enabled/unblocked password is required; no secret was retrieved.");
    }

    #[test]
    #[ignore = "Requires explicit locally enrolled/enabled password and testing authorization; performs one guarded native startup unlock in a headless mirror, then restores sleep"]
    fn live_enrolled_password_startup_unlock() {
        use std::sync::{ Arc, atomic::{ AtomicBool, AtomicU64 } };
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_PASSWORD").as_deref(), Ok("1"));
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_VIDEO").as_deref(), Ok("1"));
        let target = live_target();
        let metadata = unlock_store::metadata(&target.binding).unwrap().expect("Enroll locally in the masked app fields first.");
        assert!(metadata.enabled && !metadata.blocked && !metadata.invalidated, "Enable the local enrollment and resolve any previous attempt block before testing.");
        let _lease = unlock_store::attempt_lock(&target.binding).expect("Close the current mirror/test build before running the isolated benchmark.");
        struct RestoreSleep { target: Target, armed: bool }
        impl RestoreSleep {
            fn restore(&mut self) -> Result<(), String> {
                self.armed = false;
                let _ = unlock_detection::snapshot(&self.target, &mut || false)?;
                unlock_detection::query(&self.target, "input -d 0 keyevent 223", &mut || false)?;
                if unlock_detection::snapshot(&self.target, &mut || false)?.awake { return Err("The phone did not return to sleep.".into()); }
                Ok(())
            }
        }
        impl Drop for RestoreSleep { fn drop(&mut self) { if self.armed { let _ = self.restore(); } } }
        let mut restore = RestoreSleep { target: target.clone(), armed: true };
        let mut initial = unlock_detection::snapshot(&target, &mut || false).unwrap();
        if !initial.safe_locked || initial.awake {
            unlock_detection::query(&target, "input -d 0 keyevent 223", &mut || false).unwrap();
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                initial = unlock_detection::snapshot(&target, &mut || false).unwrap();
                if initial.safe_locked && !initial.awake { break; }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        assert!(initial.safe_locked && !initial.awake && initial.method == "password", "The same phone must be verified locked/asleep before the single native attempt; no credential was retrieved.");
        let identity = target.binding.identity.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let verified = Arc::new(AtomicBool::new(false));
        let times = Arc::new([AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX)]);
        let launch_id = format!("unlock-benchmark-{}", std::process::id());
        let watcher_id = launch_id.clone();
        let watcher_times = times.clone();
        let video_baseline = screen_power::video_samples();
        let started = Instant::now();
        let flags = (finished.clone(), entered.clone(), verified.clone());
        let watcher = std::thread::spawn(move || {
            struct StopSession(String);
            impl Drop for StopSession { fn drop(&mut self) { let _ = screen_power::stop(&self.0); } }
            let _stop = StopSession(identity.clone());
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut success_at = None;
            let mut previous_reason = String::new();
            while !flags.0.load(Ordering::SeqCst) && Instant::now() < deadline {
                let elapsed = started.elapsed().as_millis() as u64;
                if let Ok(Some(progress)) = crate::launch_progress::get(&watcher_id) {
                    let progress = serde_json::to_value(progress).unwrap();
                    if let Some(reason) = progress["reason"].as_str().filter(|reason| *reason != previous_reason) {
                        println!("Native startup phase: {}_ms={}", reason, elapsed);
                        previous_reason = reason.into();
                    }
                    for (index, reason) in [(0, "entering_password"), (1, "verifying_unlock")] {
                        if progress["reason"] == reason { let _ = watcher_times[index].compare_exchange(u64::MAX, elapsed, Ordering::SeqCst, Ordering::SeqCst); }
                    }
                    if progress["video_ready"] == true { let _ = watcher_times[3].compare_exchange(u64::MAX, elapsed, Ordering::SeqCst, Ordering::SeqCst); }
                }
                if let Some(status) = screen_power::status_for(&identity) {
                    if status.unlock_state == Some("unlocking") { flags.1.store(true, Ordering::SeqCst); }
                    if matches!(status.unlock_state, Some("blocked" | "unavailable" | "waiting")) { let _ = screen_power::stop(&identity); return; }
                    if flags.1.load(Ordering::SeqCst) && status.unlock_state == Some("monitoring") && status.unlock_reason == Some("unlock_verified") {
                        let _ = watcher_times[2].compare_exchange(u64::MAX, elapsed, Ordering::SeqCst, Ordering::SeqCst);
                    }
                    if watcher_times[2].load(Ordering::SeqCst) != u64::MAX && watcher_times[3].load(Ordering::SeqCst) != u64::MAX && screen_power::video_samples() > video_baseline {
                        success_at.get_or_insert(Instant::now());
                    }
                }
                if success_at.is_some_and(|start| start.elapsed() >= Duration::from_secs(4)) {
                    flags.2.store(true, Ordering::SeqCst);
                    let _ = screen_power::stop(&identity);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if !flags.0.load(Ordering::SeqCst) { let _ = screen_power::stop(&identity); }
        });
        let args = vec!["-s", target.selection.serial.as_str(), "--turn-screen-off", "--no-audio"];
        let result = crate::launch_progress::run(Some(&launch_id), || {
            crate::device_inventory::validate(&target.selection, &args)?;
            crate::system_command::run_scrcpy(args, false, Some(&target.selection))
        });
        finished.store(true, Ordering::SeqCst);
        let watcher_result = watcher.join();
        let after = unlock_detection::snapshot(&target, &mut || false);
        let final_metadata = unlock_store::metadata(&target.binding);
        // A failed or uncertain attempt stays blocked. This restores sleep only;
        // it never clears attempt state, retrieves another secret, or retries.
        let restored = restore.restore();
        let observed = |index: usize| match times[index].load(Ordering::SeqCst) { u64::MAX => None, value => Some(value) };
        println!("Native unlock benchmark: password_entry_started_ms={:?}, submit_started_ms={:?}, verified_unlocked_ms={:?}, headless_video_ready_ms={:?}, elapsed_including_video_watch_and_cleanup_ms={}, success={}, credential_output=false, recording=false",
            observed(0), observed(1), observed(2), observed(3), started.elapsed().as_millis(), result.is_ok() && verified.load(Ordering::SeqCst));
        watcher_result.unwrap();
        restored.unwrap();
        result.unwrap();
        assert!(entered.load(Ordering::SeqCst), "No native password attempt was observed.");
        assert!(verified.load(Ordering::SeqCst), "Native unlock and mirror startup were not verified; do not retry automatically.");
        assert!(after.unwrap().unlocked, "The same owner was not verified unlocked after native input.");
        assert!(!final_metadata.unwrap().unwrap().blocked);
        assert!(!unlock_detection::snapshot(&target, &mut || false).unwrap().awake);
        println!("Native enrolled-password startup unlock verified; owned headless mirror stopped and sleep restored. No credential output or recording.");
    }
}
