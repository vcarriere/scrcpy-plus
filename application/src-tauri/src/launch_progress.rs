use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{ Arc, Mutex };
use std::time::{ Duration, Instant };

const MAX_LAUNCHES: usize = 32;
const MAX_LINE: usize = 1024;
const VIDEO_WAIT: Duration = Duration::from_secs(10);
static LAUNCHES: Mutex<Option<HashMap<String, Reporter>>> = Mutex::new(None);
thread_local! { static CURRENT: RefCell<Option<Reporter>> = const { RefCell::new(None) }; }

#[derive(Clone, Serialize)]
pub struct Snapshot {
  phase: &'static str,
  reason: &'static str,
  elapsed_ms: u64,
  finished: bool,
  video_ready: bool,
}

struct State {
  phase: &'static str,
  reason: &'static str,
  ended: Option<Instant>,
  mirror: Option<Instant>,
  generation: u64,
  video_ready: bool,
}

struct Progress { started: Instant, state: Mutex<State> }
#[derive(Clone)]
pub(crate) struct Reporter(Arc<Progress>);

impl Reporter {
  fn new() -> Self {
    Self(Arc::new(Progress { started: Instant::now(), state: Mutex::new(State {
      phase: "checking", reason: "verifying_connection", ended: None, mirror: None, generation: 0, video_ready: false,
    }) }))
  }

  fn snapshot(&self) -> Snapshot {
    let mut state = self.0.state.lock().unwrap();
    if state.ended.is_none() && state.phase == "starting_mirror" && state.mirror.is_some_and(|started| started.elapsed() >= VIDEO_WAIT) {
      state.phase = "launched";
      state.reason = "video_unconfirmed";
    }
    Snapshot { phase: state.phase, reason: state.reason,
      elapsed_ms: state.ended.unwrap_or_else(Instant::now).duration_since(self.0.started).as_millis().min(u64::MAX as u128) as u64,
      finished: state.ended.is_some(), video_ready: state.video_ready }
  }

  fn update(&self, phase: &'static str, reason: &'static str) {
    let mut state = self.0.state.lock().unwrap();
    if state.ended.is_none() { state.phase = phase; state.reason = reason; }
  }

  fn finish(&self, success: bool) {
    let mut state = self.0.state.lock().unwrap();
    if state.ended.is_some() { return; }
    state.phase = if success { "closed" } else { "error" };
    state.reason = if success { "session_ended" } else { "session_failed" };
    state.ended = Some(Instant::now());
    state.generation = state.generation.wrapping_add(1);
    state.mirror = None;
    state.video_ready = false;
  }

  pub(crate) fn mirror_start(&self) -> u64 {
    let mut state = self.0.state.lock().unwrap();
    state.generation = state.generation.wrapping_add(1);
    state.video_ready = false;
    state.mirror = Some(Instant::now());
    if state.ended.is_none() && state.phase != "manual_unlock" {
      state.phase = "starting_mirror";
      state.reason = "waiting_for_video";
    }
    state.generation
  }

  fn video(&self, generation: u64) {
    let mut state = self.0.state.lock().unwrap();
    if state.ended.is_some() || state.generation != generation || state.mirror.is_none() { return; }
    state.video_ready = true;
    if ["starting_mirror", "launched"].contains(&state.phase) {
      state.phase = "ready";
      state.reason = "video_initialized";
    }
  }

  fn mirror_closed(&self) {
    let mut state = self.0.state.lock().unwrap();
    state.generation = state.generation.wrapping_add(1);
    state.mirror = None;
    state.video_ready = false;
  }

  fn unlocked(&self) {
    let mut state = self.0.state.lock().unwrap();
    if state.ended.is_some() || state.phase != "manual_unlock" { return; }
    if state.video_ready { state.phase = "ready"; state.reason = "video_initialized"; }
    else if state.mirror.is_some() { state.phase = "starting_mirror"; state.reason = "waiting_for_video"; }
    else { state.phase = "preparing"; state.reason = "preparing_screen_control"; }
  }
}

fn valid_id(id: &str) -> bool {
  !id.is_empty() && id.len() <= 96 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || [b'-', b'_'].contains(&byte))
}

fn register(id: &str) -> Result<Reporter, String> {
  if !valid_id(id) { return Err("The launch identifier is invalid.".into()); }
  let mut launches = LAUNCHES.lock().unwrap();
  let launches = launches.get_or_insert_with(HashMap::new);
  if launches.contains_key(id) { return Err("This launch identifier has already been used.".into()); }
  if launches.len() >= MAX_LAUNCHES {
    let oldest = launches.iter().filter(|(_, reporter)| reporter.snapshot().finished).min_by_key(|(_, reporter)| reporter.0.started).map(|(id, _)| id.clone());
    if let Some(oldest) = oldest { launches.remove(&oldest); }
    else { return Err("Too many mirroring sessions are being tracked. Close an existing session first.".into()); }
  }
  let reporter = Reporter::new();
  launches.insert(id.into(), reporter.clone());
  Ok(reporter)
}

pub fn get(id: &str) -> Result<Option<Snapshot>, String> {
  if !valid_id(id) { return Err("The launch identifier is invalid.".into()); }
  Ok(LAUNCHES.lock().unwrap().as_ref().and_then(|launches| launches.get(id)).map(Reporter::snapshot))
}

struct Scope { reporter: Reporter, previous: Option<Reporter> }
impl Drop for Scope {
  fn drop(&mut self) {
    self.reporter.finish(false);
    CURRENT.with(|current| { current.replace(self.previous.take()); });
  }
}

pub fn run<T>(id: Option<&str>, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
  let Some(id) = id else { return operation(); };
  let reporter = register(id)?;
  let previous = CURRENT.with(|current| current.replace(Some(reporter.clone())));
  let _scope = Scope { reporter: reporter.clone(), previous };
  let result = operation();
  reporter.finish(result.is_ok());
  result
}

pub(crate) fn current() -> Option<Reporter> { CURRENT.with(|current| current.borrow().clone()) }
pub(crate) fn update(phase: &'static str, reason: &'static str) { if let Some(reporter) = current() { reporter.update(phase, reason); } }
pub(crate) fn mirror_closed() { if let Some(reporter) = current() { reporter.mirror_closed(); } }
pub(crate) fn unlocked() { if let Some(reporter) = current() { reporter.unlocked(); } }

fn video_marker(line: &[u8]) -> bool {
  let Ok(line) = std::str::from_utf8(line) else { return false; };
  // These exact info messages are emitted after texture creation from decoded
  // frames in scrcpy 5.0.1 app/src/interop/interop_{software,d3d11va,vaapi,
  // videotoolbox}.c. Renderer initialization alone does not establish video.
  // https://github.com/Genymobile/scrcpy/tree/v5.0.1/app/src/interop
  let dimensions = ["INFO: Texture: ", "INFO: Texture (D3D11VA): ", "INFO: Texture (VA-API): ", "INFO: Texture (VideoToolbox): "]
    .iter().find_map(|prefix| line.trim_end_matches('\r').strip_prefix(prefix));
  let Some((width, height)) = dimensions.and_then(|value| value.split_once('x')) else { return false; };
  [width, height].iter().all(|value| !value.is_empty() && value.len() <= 5 && value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u16>().is_ok_and(|value| value > 0))
}

// Drain both owned output pipes continuously, retaining at most one bounded
// line per reader. Never return, persist, or forward raw process diagnostics.
pub(crate) fn drain(reader: impl Read, reporter: Option<&Reporter>, generation: u64, mut line_seen: impl FnMut(&[u8])) {
  let mut reader = reader;
  let mut chunk = [0; 4096];
  let mut line = Vec::with_capacity(MAX_LINE);
  let mut oversized = false;
  while let Ok(count) = reader.read(&mut chunk) {
    if count == 0 { break; }
    for byte in &chunk[..count] {
      if *byte == b'\n' {
        if !oversized {
          if video_marker(&line) { if let Some(reporter) = reporter { reporter.video(generation); } }
          line_seen(&line);
        }
        line.clear(); oversized = false;
      } else if line.len() < MAX_LINE && !oversized { line.push(*byte); }
      else { line.clear(); oversized = true; }
    }
  }
  if !oversized && !line.is_empty() {
    if video_marker(&line) { if let Some(reporter) = reporter { reporter.video(generation); } }
    line_seen(&line);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Cursor;

  #[test]
  fn only_bounded_decoded_video_markers_confirm_readiness() {
    let reporter = Reporter::new();
    let generation = reporter.mirror_start();
    drain(Cursor::new(b"scrcpy 5.0.1\nINFO: Renderer: direct3d11\n[server] INFO: Device: synthetic\n"), Some(&reporter), generation, |_| {});
    assert_eq!(reporter.snapshot().phase, "starting_mirror");
    for line in ["INFO: Texture: 0x1080", "INFO: Texture: 1920x0", "INFO: Texture: 1920x1080 extra", "ERROR: Texture: 1920x1080", "INFO: Texture: 9999999x1080", "INFO: Texture: -1x1080"] { assert!(!video_marker(line.as_bytes())); }
    for kind in ["Texture", "Texture (D3D11VA)", "Texture (VA-API)", "Texture (VideoToolbox)"] { assert!(video_marker(format!("INFO: {}: 1920x1080\r", kind).as_bytes())); }
    let mut oversized = vec![b'x'; MAX_LINE * 10]; oversized.extend_from_slice(b"INFO: Texture: 1920x1080\n");
    drain(Cursor::new(oversized), Some(&reporter), generation, |_| panic!("An oversized line must not be retained"));
    assert!(!reporter.snapshot().video_ready);
    drain(Cursor::new(b"INFO: Texture: 1920x1080\n"), Some(&reporter), generation, |_| {});
    assert_eq!(reporter.snapshot().phase, "ready");
  }

  #[test]
  fn quiet_launch_is_unconfirmed_and_stale_output_cannot_finish_a_new_mirror() {
    let reporter = Reporter::new();
    let previous = reporter.mirror_start();
    reporter.0.state.lock().unwrap().mirror = Some(Instant::now() - VIDEO_WAIT);
    assert_eq!(reporter.snapshot().phase, "launched");
    assert!(!reporter.snapshot().video_ready);
    reporter.mirror_closed();
    let current = reporter.mirror_start();
    reporter.video(previous);
    assert!(!reporter.snapshot().video_ready);
    reporter.update("manual_unlock", "automatic_unlock_blocked");
    reporter.video(current);
    assert_eq!(reporter.snapshot().phase, "manual_unlock");
    assert!(reporter.snapshot().video_ready);
    reporter.unlocked();
    assert_eq!(reporter.snapshot().phase, "ready");
    reporter.finish(false);
    reporter.video(current);
    assert_eq!(reporter.snapshot().phase, "error");
    assert!(!reporter.snapshot().video_ready);
  }

  #[test]
  fn scoped_progress_finishes_errors_and_never_returns_process_details() {
    let id = format!("native-test-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let result: Result<(), String> = run(Some(&id), || { update("preparing", "preparing_screen_control"); Err("synthetic sensitive diagnostic".into()) });
    assert!(result.is_err());
    let snapshot = get(&id).unwrap().unwrap();
    assert!(snapshot.finished);
    assert_eq!(snapshot.reason, "session_failed");
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("sensitive"));
    assert!(current().is_none());
    assert!(run(Some(&id), || Ok(())).is_err());
    for invalid in ["", "../other", "contains space"] { assert!(run(Some(invalid), || Ok(())).is_err()); }
  }
}
