use std::io::{ Read, Write };
use std::net::{ TcpStream, ToSocketAddrs };
#[cfg(windows)]
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;
use serde::Serialize;

const TIMEOUT: Duration = Duration::from_secs(2);
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime::new());

pub struct Runtime {
    pub paused: bool,
    pub changing: Option<bool>,
    pub commands: usize,
    pub sessions: usize,
    installing: bool,
    revision: u64,
}

impl Runtime {
    pub const fn new() -> Self {
        Self { paused: false, changing: None, commands: 0, sessions: 0, installing: false, revision: 0 }
    }

    fn begin(&mut self, session: bool) -> Result<(), String> {
        if self.installing { return Err("A tool installation is in progress. Wait for it to finish first.".into()); }
        if self.paused || self.changing.is_some() {
            return Err("ADB is paused. Start ADB before using a device.".into());
        }
        if session { self.sessions += 1; } else { self.commands += 1; }
        Ok(())
    }

    fn change(&mut self, start: bool) -> Result<(), String> {
        if self.installing || self.changing.is_some() || self.commands > 0 || self.sessions > 0 {
            return Err("Close SCRCPY sessions and wait for current ADB operations to finish first.".into());
        }
        self.paused = true;
        self.changing = Some(start);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    fn finish(&mut self, started: bool) {
        self.paused = !started;
        self.changing = None;
        self.revision = self.revision.wrapping_add(1);
    }

    fn observe(&mut self, running: bool, revision: u64) {
        if running && !self.installing && self.changing.is_none() && self.revision == revision {
            self.paused = false;
        }
    }

    fn install(&mut self) -> Result<(), String> {
        if self.installing || self.changing.is_some() || self.commands > 0 || self.sessions > 0 {
            return Err("Close SCRCPY sessions and wait for current ADB operations to finish before installing tools.".into());
        }
        self.installing = true;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    fn configure(&mut self) -> Result<(), String> {
        if self.installing || self.changing.is_some() { return Err("Wait for the current tool operation to finish before changing executables.".into()); }
        self.commands += 1;
        Ok(())
    }

    fn finish_install(&mut self) {
        self.installing = false;
        self.revision = self.revision.wrapping_add(1);
    }
}

pub struct Installation;

impl Drop for Installation {
    fn drop(&mut self) { RUNTIME.lock().unwrap().finish_install(); }
}

pub fn install() -> Result<Installation, String> {
    RUNTIME.lock().unwrap().install()?;
    Ok(Installation)
}

pub struct Operation { session: bool }

impl Drop for Operation {
    fn drop(&mut self) {
        let mut runtime = RUNTIME.lock().unwrap();
        if self.session { runtime.sessions -= 1; } else { runtime.commands -= 1; }
    }
}

pub fn begin(session: bool) -> Result<Operation, String> {
    RUNTIME.lock().unwrap().begin(session)?;
    Ok(Operation { session })
}

pub fn configure() -> Result<Operation, String> {
    RUNTIME.lock().unwrap().configure()?;
    Ok(Operation { session: false })
}

pub struct Change { started: bool }

impl Change {
    pub fn complete(&mut self, start: bool) { self.started = start; }
}

impl Drop for Change {
    fn drop(&mut self) {
        RUNTIME.lock().unwrap().finish(self.started);
    }
}

pub fn change(start: bool) -> Result<Change, String> {
    RUNTIME.lock().unwrap().change(start)?;
    Ok(Change { started: false })
}

#[cfg(test)]
pub fn runtime() -> (bool, Option<bool>, usize) {
    let runtime = RUNTIME.lock().unwrap();
    (runtime.paused, runtime.changing, runtime.sessions)
}

pub fn revision() -> u64 { RUNTIME.lock().unwrap().revision }

pub fn observe(running: bool, revision: u64) -> (bool, Option<bool>, usize) {
    let mut runtime = RUNTIME.lock().unwrap();
    runtime.observe(running, revision);
    (runtime.paused, runtime.changing, runtime.sessions)
}

// Match ADB's environment precedence; never silently probe a different server.
pub fn endpoint() -> Result<String, String> {
    if let Ok(socket) = std::env::var("ADB_SERVER_SOCKET") {
        if !socket.is_empty() { return Ok(socket); }
    }
    let host = std::env::var("ANDROID_ADB_SERVER_ADDRESS").unwrap_or("localhost".into());
    let port = std::env::var("ANDROID_ADB_SERVER_PORT").unwrap_or("5037".into());
    let port: u16 = port.parse().map_err(|_| "Invalid ANDROID_ADB_SERVER_PORT")?;
    if port == 0 { return Err("Invalid ANDROID_ADB_SERVER_PORT".into()); }
    Ok(format!("tcp:{}:{}", if host.is_empty() { "localhost" } else { &host }, port))
}

fn exchange(stream: &mut (impl Read + Write), request: &str) -> Result<Vec<u8>, String> {
    stream.write_all(format!("{:04x}{}", request.len(), request).as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = [0; 4];
    stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
    if &reply != b"OKAY" && &reply != b"FAIL" { return Err("The endpoint did not reply as an ADB server.".into()); }
    if &reply == b"OKAY" && request == "host:kill" { return Ok(Vec::new()); }
    let failed = &reply == b"FAIL";
    stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
    let length = usize::from_str_radix(std::str::from_utf8(&reply).map_err(|_| "Invalid ADB reply")?, 16)
        .map_err(|_| "Invalid ADB reply length")?;
    if length > 4096 { return Err("ADB reply is too large.".into()); }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).map_err(|e| e.to_string())?;
    if failed { return Err("ADB rejected the server query.".into()); }
    Ok(payload)
}

#[cfg(windows)]
fn local_listener(address: SocketAddr) -> Result<bool, String> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{ GetTcpTable2, GetTcp6Table2, MIB_TCPTABLE2, MIB_TCP6TABLE2, MIB_TCPROW2, MIB_TCP6ROW2, MIB_TCP_STATE_LISTEN };
    use std::mem::{ offset_of, size_of };
    use std::net::{ IpAddr, Ipv4Addr, Ipv6Addr };

    let ipv4 = address.is_ipv4();
    let read = |buffer: *mut u8, size: &mut u32| unsafe {
        if ipv4 { GetTcpTable2(buffer.cast(), size, 0) } else { GetTcp6Table2(buffer.cast(), size, 0) }
    };
    let mut size = 0;
    if read(std::ptr::null_mut(), &mut size) != 122 { return Err("Could not inspect local TCP listeners.".into()); }
    for _ in 0..3 {
        if size < 4 || size > 16 * 1024 * 1024 { return Err("Invalid local TCP table size.".into()); }
        // Aligned storage for the OS table, including its variable-length row array.
        let mut storage = vec![0u64; (size as usize + 7) / 8];
        let pointer = storage.as_mut_ptr().cast::<u8>();
        let result = read(pointer, &mut size);
        if result == 122 { continue; }
        if result != 0 { return Err(format!("Could not inspect local TCP listeners ({}).", result)); }
        let offset = if ipv4 { offset_of!(MIB_TCPTABLE2, table) } else { offset_of!(MIB_TCP6TABLE2, table) };
        let row_size = if ipv4 { size_of::<MIB_TCPROW2>() } else { size_of::<MIB_TCP6ROW2>() };
        // SAFETY: the successful OS call initialized the count; validate the entire
        // row range against the allocated buffer before reading any row.
        let count = unsafe { pointer.cast::<u32>().read() } as usize;
        if count > (storage.len() * 8 - offset) / row_size { return Err("Invalid local TCP table rows.".into()); }
        for index in 0..count {
            let row = unsafe { pointer.add(offset + index * row_size) };
            let (state, port, ip) = if ipv4 {
                let row = unsafe { row.cast::<MIB_TCPROW2>().read_unaligned() };
                (row.dwState, row.dwLocalPort, IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())))
            } else {
                let row = unsafe { row.cast::<MIB_TCP6ROW2>().read_unaligned() };
                (row.State as u32, row.dwLocalPort, IpAddr::V6(Ipv6Addr::from(unsafe { row.LocalAddr.u.Byte })))
            };
            if state == MIB_TCP_STATE_LISTEN as u32 && u16::from_be(port as u16) == address.port() && (ip.is_unspecified() || ip == address.ip()) {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    Err("The local TCP table kept changing. Try again.".into())
}

fn query_bytes(socket: &str, request: &str) -> Result<Option<Vec<u8>>, String> {
    if let Some(address) = socket.strip_prefix("tcp:") {
        let address = if address.parse::<u16>().is_ok() { format!("localhost:{}", address) } else { address.to_string() };
        let mut addresses: Vec<_> = address.to_socket_addrs().map_err(|e| e.to_string())?.collect();
        if addresses.iter().any(|address| address.port() == 0) { return Err("ADB server port must be between 1 and 65535.".into()); }
        addresses.sort_by_key(|address| !address.is_ipv4());
        if addresses.is_empty() { return Err("ADB server address could not be resolved.".into()); }
        let mut refused = true;
        let mut last_error = String::new();
        for address in addresses {
            // A closed local Windows port may time out rather than refuse a SYN.
            // Check the read-only OS listener table before making a connection.
            #[cfg(windows)]
            if address.ip().is_loopback() && !local_listener(address)? { continue; }
            match TcpStream::connect_timeout(&address, TIMEOUT) {
                Ok(mut stream) => {
                    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
                    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
                    return exchange(&mut stream, request).map(Some);
                },
                Err(error) => {
                    refused &= error.kind() == std::io::ErrorKind::ConnectionRefused;
                    last_error = error.to_string();
                },
            }
        }
        return if refused { Ok(None) } else { Err(last_error) };
    }
    #[cfg(unix)]
    if let Some(path) = socket.strip_prefix("localfilesystem:") {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
                stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
                return exchange(&mut stream, request).map(Some);
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound || error.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("This ADB server socket is not supported by the status controls. Select a TCP endpoint.".into())
}

pub fn query(socket: &str, request: &str) -> Result<Option<String>, String> {
    query_bytes(socket, request)?.map(|payload| String::from_utf8(payload).map_err(|_| "Invalid ADB reply text".into())).transpose()
}

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Details {
    pub version: Option<String>,
    pub build: Option<String>,
    pub path: Option<String>,
}

fn varint(input: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let Some((&byte, rest)) = input.split_first() else { return Err("Incomplete ADB server details".into()); };
        *input = rest;
        if shift == 63 && byte > 1 { return Err("Invalid ADB server detail length".into()); }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 { return Ok(value); }
    }
    Err("Invalid ADB server details".into())
}

// Read only the three public build/path fields in AdbServerStatus. Other fields
// can contain key-store locations and are skipped rather than sent to the GUI.
fn parse_details(mut input: &[u8]) -> Result<Details, String> {
    if input.len() > 4096 { return Err("ADB server details are too large".into()); }
    let mut details = Details::default();
    while !input.is_empty() {
        let tag = varint(&mut input)?;
        let field = tag >> 3;
        if field == 0 || field > 0x1fffffff { return Err("Invalid ADB server detail field".into()); }
        let length = match tag & 7 {
            0 => { varint(&mut input)?; continue; },
            1 => 8,
            2 => usize::try_from(varint(&mut input)?).map_err(|_| "Invalid ADB server detail length")?,
            5 => 4,
            _ => return Err("Unsupported ADB server detail field".into()),
        };
        if length > input.len() { return Err("Incomplete ADB server details".into()); }
        let (value, rest) = input.split_at(length);
        input = rest;
        if tag & 7 == 2 && (5..=7).contains(&field) {
            let text = std::str::from_utf8(value).map_err(|_| "Invalid ADB server detail text")?;
            let value = (!text.is_empty()).then(|| text.to_string());
            match field { 5 => details.version = value, 6 => details.build = value, 7 => details.path = value, _ => {} }
        }
    }
    Ok(details)
}

pub fn details(socket: &str) -> Result<Option<Details>, String> {
    query_bytes(socket, "host:server-status")?.map(|payload| parse_details(&payload)).transpose()
}

pub fn version(socket: &str) -> Result<Option<u32>, String> {
    query(socket, "host:version")?.map(|text| {
        u32::from_str_radix(&text, 16).map_err(|_| "Invalid ADB server version.".into())
    }).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn server(reply: &'static [u8]) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("tcp:{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 16];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"000chost:version");
            stream.write_all(reply).unwrap();
        });
        (address, handle)
    }

    #[test]
    fn reads_protocol_version_without_launching_a_client() {
        let (address, handle) = server(b"OKAY00040029");
        assert_eq!(version(&address).unwrap(), Some(41));
        handle.join().unwrap();
    }

    #[test]
    fn installation_is_exclusive_and_preserves_intentional_stop() {
        let mut runtime = Runtime::new();
        runtime.begin(true).unwrap();
        assert!(runtime.install().is_err());
        runtime.sessions = 0;
        runtime.begin(false).unwrap();
        assert!(runtime.install().is_err());
        runtime.commands = 0;
        runtime.paused = true;
        runtime.install().unwrap();
        assert!(runtime.install().is_err());
        assert!(runtime.begin(true).is_err());
        assert!(runtime.begin(false).is_err());
        assert!(runtime.configure().is_err());
        assert!(runtime.change(true).is_err());
        runtime.observe(true, runtime.revision);
        assert!(runtime.paused);
        runtime.finish_install();
        assert!(runtime.paused);
        runtime.paused = false;
        runtime.install().unwrap();
        runtime.finish_install();
        assert!(runtime.begin(true).is_ok());
    }

    #[test]
    fn rejects_a_non_adb_service() {
        let (address, handle) = server(b"HTTP/1.1");
        assert!(version(&address).is_err());
        handle.join().unwrap();
    }

    #[test]
    fn closed_port_stays_stopped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("tcp:{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(version(&address).unwrap(), None);
        assert_eq!(details(&address).unwrap(), None);
        assert_eq!(version(&address).unwrap(), None);
    }

    #[test]
    fn reads_server_build_details_and_ignores_private_or_future_fields() {
        let payload = b"\x08\x04\x2a\x0637.0.3\x32\x041234\x3a\x08/opt/adb\x42\x01\xff\x60\x01\x6a\x06secret\xa5\x01\0\0\0\0";
        let expected = Details { version: Some("37.0.3".into()), build: Some("1234".into()), path: Some("/opt/adb".into()) };
        assert_eq!(parse_details(payload).unwrap(), expected);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("tcp:{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 22];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"0012host:server-status");
            stream.write_all(format!("OKAY{:04x}", payload.len()).as_bytes()).unwrap();
            stream.write_all(payload).unwrap();
        });
        assert_eq!(details(&address).unwrap(), Some(expected));
        handle.join().unwrap();
    }

    #[test]
    fn rejects_malformed_metadata_without_reinterpreting_it_as_a_version() {
        for payload in [b"\x2a\x06bad".as_slice(), b"\x2a\x01\xff", b"\0", b"\x0b", b"\x2a\xff\xff\xff\xff\xff\xff\xff\xff\xff\x7f"] {
            assert!(parse_details(payload).is_err());
        }
        assert_eq!(parse_details(b"\x08\x01").unwrap(), Details::default());
        assert!(parse_details(&vec![0; 4097]).is_err());
    }

    #[test]
    #[ignore = "Requires an existing local ADB server; does not launch a client"]
    fn reads_live_server_details_passively() {
        let socket = std::env::var("SCRCPY_PLUS_TEST_ADB_SOCKET").expect("Set SCRCPY_PLUS_TEST_ADB_SOCKET");
        assert!(version(&socket).unwrap().is_some());
        let observed = details(&socket).unwrap().expect("Existing server required");
        assert!(observed.version.is_some());
        assert!(observed.path.is_some());
        println!("Server release: {}; build: {}; executable path available: {}", observed.version.unwrap(), observed.build.unwrap_or_default(), observed.path.is_some());
    }

    #[test]
    fn two_servers_have_independent_status() {
        let (first, one) = server(b"OKAY00040029");
        let (second, two) = server(b"OKAY00040028");
        assert_ne!(first, second);
        assert_eq!(version(&first).unwrap(), Some(41));
        assert_eq!(version(&second).unwrap(), Some(40));
        one.join().unwrap();
        two.join().unwrap();
    }

    #[test]
    fn stopped_server_keeps_commands_paused() {
        let mut runtime = Runtime::new();
        runtime.change(false).unwrap();
        runtime.finish(false);
        runtime.observe(false, runtime.revision);
        assert!(runtime.begin(false).is_err());
        assert!(runtime.begin(true).is_err());
        runtime.change(true).unwrap();
        assert!(runtime.begin(false).is_err());
        runtime.finish(true);
        assert!(runtime.begin(true).is_ok());
        assert!(runtime.change(false).is_err());
    }

    #[test]
    fn fresh_running_server_resumes_without_starting_it() {
        let mut runtime = Runtime::new();
        runtime.change(false).unwrap();
        runtime.finish(false);
        runtime.observe(true, runtime.revision);
        assert!(!runtime.paused);
        assert!(runtime.changing.is_none());
        assert!(runtime.begin(false).is_ok());
    }

    #[test]
    fn stale_running_observations_cannot_undo_stop() {
        let mut runtime = Runtime::new();
        let before_stop = runtime.revision;
        runtime.change(false).unwrap();
        let during_stop = runtime.revision;
        runtime.observe(true, during_stop);
        assert!(runtime.paused);
        runtime.finish(false);
        runtime.observe(true, before_stop);
        assert!(runtime.paused);
        runtime.observe(true, during_stop);
        assert!(runtime.paused);
        runtime.observe(false, runtime.revision);
        assert!(runtime.paused);
        runtime.observe(true, runtime.revision);
        assert!(!runtime.paused);
    }

    #[test]
    fn guards_release_counts_and_preserve_pause_after_an_incomplete_change() {
        let session = begin(true).unwrap();
        assert!(change(false).is_err());
        drop(session);
        let stop = change(false).unwrap();
        assert!(begin(false).is_err());
        drop(stop);
        assert!(runtime().0);
        assert!(crate::system_command::adb(vec!["devices"]).is_err());
        assert!(crate::protected_screen::inspect("synthetic-device").is_err());
        assert!(crate::protected_screen::inspect("synthetic-device").is_err());
        assert!(crate::system_command::run_scrcpy(vec!["-s", "synthetic-device", "--turn-screen-off"], true, None).is_err());
        let mut start = change(true).unwrap();
        start.complete(true);
        drop(start);
        assert!(!runtime().0);
        let command = begin(false).unwrap();
        assert!(change(false).is_err());
        drop(command);
        assert_eq!(runtime().2, 0);
    }
}
