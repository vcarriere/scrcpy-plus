use std::io::{ Read, Write };
use std::path::PathBuf;
use std::process::{ Child, Command, Stdio };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::time::{ Duration, Instant, SystemTime, UNIX_EPOCH };
use sha2::{ Digest, Sha256 };
use crate::{ adb_server, screen_power::AdbClient, unlock_detection::Target };
#[cfg(windows)]
use std::os::windows::{ fs::OpenOptionsExt, io::AsRawHandle, process::CommandExt };

const HELPER: &[u8] = include_bytes!("../resources/unlock-guard.jar");
const LIMIT: usize = 262144;
const UNAVAILABLE: &str = "The private unlock-state channel is unavailable. Automatic input stopped.";
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

// One shell connection owns the complete diagnostic lifetime. Reconnecting ADB
// cannot silently attach this channel to a replacement transport or phone.
pub struct Session {
    client: AdbClient,
    transport: String,
    identity: String,
    remote: String,
    local: PathBuf,
    file: Option<std::fs::File>,
    child: Option<Child>,
    failed: bool,
}

struct Frame {
    begin: String,
    end: String,
    started: bool,
    complete: bool,
    pending: Vec<u8>,
    output: String,
    received: usize,
}

impl Frame {
    fn new(nonce: &str) -> Self {
        Self { begin: format!("BEGIN {}", nonce), end: format!("END {}", nonce), started: false, complete: false, pending: Vec::new(), output: String::new(), received: 0 }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<bool, String> {
        self.received = self.received.checked_add(bytes.len()).filter(|size| *size <= LIMIT).ok_or(UNAVAILABLE)?;
        if self.complete { return Err(UNAVAILABLE.into()); }
        self.pending.extend_from_slice(bytes);
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8(self.pending.drain(..=index).collect()).map_err(|_| UNAVAILABLE)?;
            let line = line.strip_suffix('\n').unwrap();
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.chars().any(|character| character.is_control() && character != '\t') { return Err(UNAVAILABLE.into()); }
            if !self.started {
                if line != self.begin { return Err(UNAVAILABLE.into()); }
                self.started = true;
            } else if line == self.end {
                if !self.pending.is_empty() { return Err(UNAVAILABLE.into()); }
                self.complete = true;
                return Ok(true);
            } else {
                if line.starts_with("BEGIN ") || line.starts_with("END ") || line.starts_with("ERROR ") { return Err(UNAVAILABLE.into()); }
                self.output.push_str(line);
                self.output.push('\n');
            }
        }
        if self.pending.len() > 65536 { return Err(UNAVAILABLE.into()); }
        Ok(false)
    }
}

impl Session {
    pub fn start(target: &Target, cancelled: &mut impl FnMut() -> bool) -> Result<Self, String> {
        #[cfg(not(windows))]
        { let _ = (target, cancelled); return Err(UNAVAILABLE.into()); }
        #[cfg(windows)]
        {
            if target.sdk < 35 || target.binding.user != 0 || !crate::screen_power_control::local_server(&target.client.socket) || cancelled() { return Err(UNAVAILABLE.into()); }
            let transport = target.selection.transport_id.filter(|id| *id != 0).ok_or(UNAVAILABLE)?.to_string();
            let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let name = format!("scrcpy-plus-unlock-guard-{}-{}-{}.jar", std::process::id(), seed, NEXT_REQUEST.fetch_add(1, Ordering::Relaxed));
            let local = std::env::temp_dir().join(&name);
            // Keep a non-writable/non-deletable handle until push and session
            // cleanup finish. Only these embedded bytes may run on the phone.
            let file = std::fs::OpenOptions::new().create_new(true).write(true).read(true).share_mode(1).open(&local).map_err(|_| UNAVAILABLE)?;
            let mut session = Self { client: target.client.clone(), transport, identity: target.hardware_serial.clone(), remote: format!("/data/local/tmp/{}", name), local, file: Some(file), child: None, failed: false };
            session.file.as_mut().unwrap().write_all(HELPER).map_err(|_| UNAVAILABLE)?;
            session.file.as_mut().unwrap().flush().map_err(|_| UNAVAILABLE)?;
            session.query(&["push", session.local.to_str().ok_or(UNAVAILABLE)?, &session.remote], cancelled)?;
            let digest: String = Sha256::digest(HELPER).iter().map(|byte| format!("{:02x}", byte)).collect();
            let hash = session.query(&["shell", "sha256sum", &session.remote], cancelled)?;
            if !hash.split_whitespace().eq([digest.as_str(), session.remote.as_str()]) { return Err(UNAVAILABLE.into()); }
            let script = format!("CLASSPATH={} app_process / com.scrcpyplus.unlockguard.Main", session.remote);
            let mut command = Command::new(&session.client.path);
            command.args(["-t", &session.transport, "shell", "-T", &script]).env("ADB_SERVER_SOCKET", &session.client.socket)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(0x08000000);
            session.child = Some(command.spawn().map_err(|_| UNAVAILABLE)?);
            Ok(session)
        }
    }

    fn query(&self, args: &[&str], cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
        let mut selected = vec!["-t", &self.transport];
        selected.extend_from_slice(args);
        self.client.query(&selected, cancelled, false)
    }

    pub fn snapshot(&mut self, cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
        if self.failed || cancelled() { return Err(UNAVAILABLE.into()); }
        let result = self.read_snapshot(cancelled);
        if result.is_err() {
            self.failed = true;
            self.close();
        }
        result
    }

    #[cfg(not(windows))]
    fn read_snapshot(&mut self, _: &mut impl FnMut() -> bool) -> Result<String, String> { Err(UNAVAILABLE.into()) }

    #[cfg(windows)]
    fn read_snapshot(&mut self, cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;
        let nonce = format!("{}_{}", std::process::id(), NEXT_REQUEST.fetch_add(1, Ordering::Relaxed));
        let request = format!("SNAPSHOT {}\n", nonce);
        let child = self.child.as_mut().ok_or(UNAVAILABLE)?;
        if child.try_wait().map_err(|_| UNAVAILABLE)?.is_some() { return Err("The private diagnostic process exited before its request.".into()); }
        let stdout = child.stdout.as_mut().ok_or(UNAVAILABLE)?;
        let mut available = 0;
        // Unsolicited/stale output is never accepted as a fresh observation.
        if unsafe { PeekNamedPipe(stdout.as_raw_handle(), std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut available, std::ptr::null_mut()) } == 0 || available != 0 { return Err("The private diagnostic pipe was unavailable before its request.".into()); }
        child.stdin.as_mut().ok_or(UNAVAILABLE)?.write_all(request.as_bytes()).map_err(|_| UNAVAILABLE)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut frame = Frame::new(&nonce);
        loop {
            if cancelled() || Instant::now() >= deadline { return Err(UNAVAILABLE.into()); }
            let stdout = child.stdout.as_mut().ok_or(UNAVAILABLE)?;
            if unsafe { PeekNamedPipe(stdout.as_raw_handle(), std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut available, std::ptr::null_mut()) } == 0 { return Err("The private diagnostic pipe closed during its request.".into()); }
            if available > 0 {
                let mut bytes = [0; 8192];
                let count = stdout.read(&mut bytes[..(available as usize).min(8192)]).map_err(|_| UNAVAILABLE)?;
                if count == 0 { return Err(UNAVAILABLE.into()); }
                if frame.push(&bytes[..count])? { return Ok(frame.output); }
            } else {
                if child.try_wait().map_err(|_| UNAVAILABLE)?.is_some() { return Err(UNAVAILABLE.into()); }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            // EOF lets the phone helper exit normally. Never terminate any
            // shared ADB server, other client, or other phone-side process.
            child.stdin.take();
            let deadline = Instant::now() + Duration::from_millis(200);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(5)); }
            if !matches!(child.try_wait(), Ok(Some(_))) { let _ = child.kill(); }
            let _ = child.wait();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
        if adb_server::version(&self.client.socket).is_ok_and(|version| version.is_some()) {
            let deadline = Instant::now() + Duration::from_millis(1000);
            let mut cancelled = || Instant::now() >= deadline;
            if self.query(&["shell", "getprop", "ro.serialno"], &mut cancelled).is_ok_and(|serial| serial.trim() == self.identity) {
                let _ = self.query(&["shell", "rm", "-f", &self.remote], &mut cancelled);
            }
        }
        self.file.take();
        let _ = std::fs::remove_file(&self.local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_complete_current_frames_across_split_crlf_and_utf8() {
        let bytes = "BEGIN current\r\n@@identity\r\nexämple\r\n@@end\r\nEND current\r\n".as_bytes();
        for split in 0..bytes.len() {
            let mut frame = Frame::new("current");
            assert!(!frame.push(&bytes[..split]).unwrap());
            assert!(frame.push(&bytes[split..]).unwrap());
            assert_eq!(frame.output, "@@identity\nexämple\n@@end\n");
        }
        assert!(!Frame::new("current").push(b"BEGIN current\n@@end\n").unwrap());
    }

    #[test]
    fn rejects_stale_duplicate_error_corrupt_and_unbounded_frames() {
        for bytes in [b"BEGIN old\n".as_slice(), b"ERROR current timeout\n", b"noise\n", b"BEGIN current\nBEGIN current\n", b"BEGIN current\nEND old\n", b"BEGIN current\nEND current\nextra", b"BEGIN current\n\xff\n", b"BEGIN current\ninvalid\rvalue\n"] {
            assert!(Frame::new("current").push(bytes).is_err());
        }
        assert!(Frame::new("current").push(&vec![b'x'; 65537]).is_err());
        let mut frame = Frame::new("current");
        assert!(!frame.push(b"BEGIN current\n").unwrap());
        for _ in 0..31 { assert!(!frame.push(&[vec![b'x'; 8190], vec![b'\n']].concat()).unwrap()); }
        assert!(frame.push(&vec![b'x'; 16384]).is_err());
        let mut frame = Frame::new("current");
        assert!(frame.push(b"BEGIN current\n@@end\nEND current\n").unwrap());
        assert!(frame.push(b"BEGIN current\n").is_err());
    }
}
