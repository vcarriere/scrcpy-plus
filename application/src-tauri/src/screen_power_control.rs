use std::io::{ Read, Write };
use std::net::{ Shutdown, SocketAddr, TcpStream };
use std::path::{ Path, PathBuf };
use std::process::{ Child, Command, Stdio };
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::atomic::{ AtomicU32, Ordering };
use std::time::{ Duration, Instant, SystemTime, UNIX_EPOCH };
use crate::screen_power::AdbClient;
use zeroize::Zeroizing;

static NEXT_SOCKET: AtomicU32 = AtomicU32::new(1);

pub struct Control {
    client: AdbClient,
    device: String,
    transport: Option<String>,
    identity: String,
    remote: String,
    socket: String,
    port: Option<u16>,
    child: Option<Child>,
    stream: Option<TcpStream>,
}

fn local_server(endpoint: &str) -> bool {
    if endpoint.starts_with("localfilesystem:") { return true; }
    let Some(address) = endpoint.strip_prefix("tcp:") else { return false; };
    if address.parse::<u16>().is_ok_and(|port| port != 0) { return true; }
    if let Some(port) = address.strip_prefix("localhost:") { return port.parse::<u16>().is_ok_and(|port| port != 0); }
    address.parse::<SocketAddr>().is_ok_and(|address| address.ip().is_loopback() && address.port() != 0)
}

fn server_path(executable: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SCRCPY_SERVER_PATH") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path).ok_or("SCRCPY_SERVER_PATH does not point to a scrcpy-server file.".into());
    }
    let path = Path::new(executable);
    let resolved = if path.is_file() { Some(path.to_path_buf()) } else {
        std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|directory| directory.join(path)).find(|candidate| candidate.is_file()))
    };
    let sibling = resolved.and_then(|path| path.parent().map(|directory| directory.join("scrcpy-server")));
    sibling.filter(|path| path.is_file()).ok_or("Automatic screen control needs scrcpy-server beside the selected scrcpy executable, or SCRCPY_SERVER_PATH.".into())
}

fn selector<'a>(transport: Option<&'a str>, device: &'a str) -> [&'a str; 2] {
    transport.map(|transport| ["-t", transport]).unwrap_or(["-s", device])
}

fn server_command(remote: &str, scid: u32) -> String {
    format!("CLASSPATH={} app_process / com.genymobile.scrcpy.Server 5.0.1 scid={:08x} tunnel_forward=true video=false audio=false control=true cleanup=false power_on=false clipboard_autosync=false send_device_meta=false send_dummy_byte=true log_level=error", remote, scid)
}

fn key_message(action: u8, keycode: u32, meta: u32) -> [u8; 14] {
    let mut message = [0; 14];
    message[1] = action;
    message[2..6].copy_from_slice(&keycode.to_be_bytes());
    message[10..14].copy_from_slice(&meta.to_be_bytes());
    message
}

fn text_message(character: u8) -> Result<Zeroizing<[u8; 6]>, String> {
    if !(32..=126).contains(&character) { return Err("Unsupported password character.".into()); }
    Ok(Zeroizing::new([1, 0, 0, 0, 1, character]))
}

fn write_control(stream: &mut Option<TcpStream>, message: &[u8]) -> Result<(), String> {
    let result = stream.as_mut().ok_or("The screen-power control channel is closed. Close this mirror and start it again.")?.write_all(message);
    if result.is_err() {
        // A partial protocol message must never be followed by another message
        // on this connection, including any remaining password characters.
        if let Some(stream) = stream.take() { let _ = stream.shutdown(Shutdown::Both); }
        return Err("The screen-control connection failed. Close this mirror and start it again.".into());
    }
    Ok(())
}

impl Control {
    pub fn start(executable: &str, client: &AdbClient, device: &str, identity: &str, cancelled: &mut impl FnMut() -> bool) -> Result<Self, String> {
        Self::start_selected(executable, client, device, identity, None, cancelled)
    }

    pub fn start_pinned(executable: &str, client: &AdbClient, device: &str, identity: &str, transport_id: u64, cancelled: &mut impl FnMut() -> bool) -> Result<Self, String> {
        if transport_id == 0 { return Err("The selected device transport is unavailable.".into()); }
        Self::start_selected(executable, client, device, identity, Some(transport_id.to_string()), cancelled)
    }

    fn query_selected(&self, args: &[&str], cancelled: &mut impl FnMut() -> bool) -> Result<String, String> {
        let mut selected = selector(self.transport.as_deref(), &self.device).to_vec();
        selected.extend_from_slice(args);
        self.client.query(&selected, cancelled, false)
    }

    fn verify_identity(&self, cancelled: &mut impl FnMut() -> bool) -> Result<(), String> {
        if !self.query_selected(&["shell", "getprop", "ro.serialno"], cancelled).is_ok_and(|serial| serial.trim() == self.identity) {
            return Err("The control channel's device identity changed. No password was sent.".into());
        }
        Ok(())
    }

    fn start_selected(executable: &str, client: &AdbClient, device: &str, identity: &str, transport: Option<String>, cancelled: &mut impl FnMut() -> bool) -> Result<Self, String> {
        if !local_server(&client.socket) { return Err("Automatic screen control requires a local ADB server.".into()); }
        let version = AdbClient { path: executable.into(), socket: client.socket.clone() }.query(&["--version"], cancelled, false)?;
        // The control protocol is internal to scrcpy. Use only a verified release
        // and its matching server; leave other versions available for normal use.
        if !version.lines().any(|line| line.split_whitespace().take(2).eq(["scrcpy", "5.0.1"])) {
            return Err("Automatic screen control currently supports scrcpy 5.0.1. Select its executable and matching scrcpy-server.".into());
        }
        let server = server_path(executable)?;
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos();
        let scid = (seed ^ std::process::id().rotate_left(12) ^ NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)) & 0x7fffffff;
        let mut control = Self {
            client: client.clone(), device: device.into(), transport, identity: identity.into(),
            remote: format!("/data/local/tmp/scrcpy-plus-power-{}-{:08x}.jar", std::process::id(), scid),
            socket: format!("localabstract:scrcpy_{:08x}", scid), port: None, child: None, stream: None,
        };
        control.verify_identity(cancelled)?;
        control.query_selected(&["push", server.to_str().ok_or("The scrcpy-server path is not valid Unicode.")?, &control.remote], cancelled)?;
        let port = control.query_selected(&["forward", "--no-rebind", "tcp:0", &control.socket], cancelled)?;
        control.port = Some(port.trim().parse::<u16>().ok().filter(|port| *port != 0).ok_or("ADB did not allocate a screen-control port.")?);
        // A separate control-only server uses scrcpy's SurfaceControl power path.
        // It never captures video/audio, synchronizes clipboard, wakes the phone,
        // or changes the main mirror's cleanup behavior.
        let shell = server_command(&control.remote, scid);
        let mut command = Command::new(&client.path);
        command.args(selector(control.transport.as_deref(), device)).args(["shell", &shell]).env("ADB_SERVER_SOCKET", &client.socket)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        control.child = Some(command.spawn().map_err(|_| "Could not start scrcpy's screen-power control channel.")?);
        let address = SocketAddr::from(([127, 0, 0, 1], control.port.unwrap()));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if cancelled() { return Err("Screen-control operation cancelled.".into()); }
            if control.child.as_mut().unwrap().try_wait().map_err(|_| "Could not inspect the screen-power control channel.")?.is_some() {
                return Err("The screen-power control channel exited. Check that scrcpy and scrcpy-server are both version 5.0.1.".into());
            }
            if let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(150)) {
                stream.set_read_timeout(Some(Duration::from_millis(150))).map_err(|_| "Could not set the screen-control timeout.")?;
                stream.set_write_timeout(Some(Duration::from_millis(200))).map_err(|_| "Could not set the screen-control timeout.")?;
                let mut hello = [255];
                if stream.read_exact(&mut hello).is_ok() && hello == [0] {
                    control.stream = Some(stream);
                    control.verify_identity(cancelled)?;
                    return Ok(control);
                }
            }
            if Instant::now() >= deadline { return Err("Timed out connecting to scrcpy's screen-power control channel.".into()); }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn power(&mut self, on: bool) -> Result<(), String> {
        self.send(&[10, u8::from(on)])
    }

    fn send(&mut self, message: &[u8]) -> Result<(), String> {
        if self.child.as_mut().ok_or("The screen-power control channel is closed.")?.try_wait().map_err(|_| "Could not inspect screen control.")?.is_some() {
            return Err("The screen-power control channel ended.".into());
        }
        write_control(&mut self.stream, message)
    }

    pub fn key(&mut self, keycode: u32) -> Result<(), String> {
        self.key_with_meta(keycode, 0)
    }

    pub fn select_all(&mut self) -> Result<(), String> { self.key_with_meta(29, 0x1000) }

    fn key_with_meta(&mut self, keycode: u32, meta: u32) -> Result<(), String> {
        for action in [0, 1] {
            self.send(&key_message(action, keycode, meta))?;
        }
        Ok(())
    }

    pub fn character(&mut self, character: u8) -> Result<(), String> {
        // INJECT_TEXT: type, big-endian byte count, UTF-8 payload. Neither the
        // password nor its characters pass through shell/process arguments.
        let message = text_message(character)?;
        self.send(&*message)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() { let _ = stream.shutdown(Shutdown::Both); }
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + Duration::from_millis(400);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
            if !matches!(child.try_wait(), Ok(Some(_))) { let _ = child.kill(); }
            let _ = child.wait();
        }
        // Cleanup must not bring an externally stopped shared server back up.
        if !crate::adb_server::version(&self.client.socket).is_ok_and(|version| version.is_some()) { return; }
        let deadline = Instant::now() + Duration::from_millis(1200);
        let mut cancelled = || Instant::now() >= deadline;
        if self.verify_identity(&mut cancelled).is_err() { return; }
        if let Some(port) = self.port {
            let local = format!("tcp:{}", port);
            if let Ok(forwards) = self.query_selected(&["forward", "--list"], &mut cancelled) {
                if forwards.lines().any(|line| line.split_whitespace().eq([self.device.as_str(), local.as_str(), self.socket.as_str()])) {
                    let _ = self.query_selected(&["forward", "--remove", &local], &mut cancelled);
                }
            }
        }
        if self.verify_identity(&mut cancelled).is_ok() {
            let _ = self.query_selected(&["shell", "rm", "-f", &self.remote], &mut cancelled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_tunnels_require_a_local_adb_server() {
        for socket in ["tcp:5037", "tcp:127.0.0.1:5037", "tcp:localhost:5037", "tcp:[::1]:5037", "localfilesystem:/tmp/adb.sock"] { assert!(local_server(socket)); }
        for socket in ["tcp:0", "tcp:example.com:5037", "tcp:10.0.0.1:5037", "tcp:localhost:0"] { assert!(!local_server(socket)); }
    }

    #[test]
    fn pinned_transport_always_takes_precedence_over_a_reused_serial() {
        assert_eq!(selector(Some("27"), "synthetic-reused-ip:12345"), ["-t", "27"]);
        assert_eq!(selector(None, "synthetic-device"), ["-s", "synthetic-device"]);
    }

    #[test]
    fn private_channel_disables_capture_clipboard_and_password_logging() {
        let command = server_command("/synthetic/server.jar", 7);
        for option in ["video=false", "audio=false", "control=true", "cleanup=false", "power_on=false", "clipboard_autosync=false", "send_device_meta=false", "log_level=error"] {
            assert!(command.split_whitespace().any(|value| value == option));
        }
        assert!(!command.contains("password"));
    }

    #[test]
    fn protocol_frames_have_the_expected_type_lengths_endianness_and_metadata() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut sender = Some(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let (mut receiver, _) = listener.accept().unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        write_control(&mut sender, &key_message(0, 29, 0x1000)).unwrap();
        write_control(&mut sender, &key_message(1, 29, 0x1000)).unwrap();
        write_control(&mut sender, &*text_message(b'Z').unwrap()).unwrap();
        write_control(&mut sender, &[10, 0]).unwrap();
        let mut bytes = [0; 36];
        receiver.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes[..14], &[0, 0, 0, 0, 0, 29, 0, 0, 0, 0, 0, 0, 16, 0]);
        assert_eq!(&bytes[14..28], &[0, 1, 0, 0, 0, 29, 0, 0, 0, 0, 0, 0, 16, 0]);
        assert_eq!(&bytes[28..], &[1, 0, 0, 0, 1, b'Z', 10, 0]);
        assert!(text_message(0).is_err());
        assert!(text_message(127).is_err());
    }

    #[test]
    fn a_failed_protocol_write_closes_the_channel_before_any_following_character() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut sender = Some(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let (_receiver, _) = listener.accept().unwrap();
        sender.as_mut().unwrap().shutdown(Shutdown::Write).unwrap();
        assert!(write_control(&mut sender, &*text_message(b'A').unwrap()).is_err());
        assert!(sender.is_none());
        assert!(write_control(&mut sender, &*text_message(b'B').unwrap()).is_err());
    }

    #[test]
    #[ignore = "Requires an explicitly selected phone and existing mirror; restores only the physical panel to On"]
    fn live_restore_existing_mirror_panel() {
        let client = AdbClient { path: std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap(), socket: crate::adb_server::endpoint().unwrap() };
        let device = std::env::var("SCRCPY_PLUS_TEST_DEVICE").unwrap();
        let executable = std::env::var("SCRCPY_PLUS_TEST_SCRCPY").unwrap();
        let _session = crate::adb_server::begin(true).unwrap();
        let mut cancelled = || false;
        let identity = client.query(&["-s", &device, "shell", "getprop", "ro.serialno"], &mut cancelled, false).unwrap();
        let mut control = Control::start(&executable, &client, &device, identity.trim(), &mut cancelled).unwrap();
        control.power(true).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let display = client.query(&["-s", &device, "shell", "dumpsys SurfaceFlinger --displays | grep -E '^[[:space:]]*((Virtual )?Display [0-9]|powerMode=)'"], &mut cancelled, false).unwrap();
        assert_eq!(crate::screen_power::panel(&display), Some(crate::screen_power::Panel::On));
        println!("Existing mirror left running; verified physical panel On through scrcpy control.");
    }
}
