use std::io::{ Read, Write };
use std::net::{ TcpStream, ToSocketAddrs };
#[cfg(windows)]
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(2);
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime::new());

pub struct Runtime {
    pub paused: bool,
    pub changing: Option<bool>,
    pub commands: usize,
    pub sessions: usize,
    revision: u64,
}

impl Runtime {
    pub const fn new() -> Self {
        Self { paused: false, changing: None, commands: 0, sessions: 0, revision: 0 }
    }

    fn begin(&mut self, session: bool) -> Result<(), String> {
        if self.paused || self.changing.is_some() {
            return Err("ADB is paused. Start ADB before using a device.".into());
        }
        if session { self.sessions += 1; } else { self.commands += 1; }
        Ok(())
    }

    fn change(&mut self, start: bool) -> Result<(), String> {
        if self.changing.is_some() || self.commands > 0 || self.sessions > 0 {
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
        if running && self.changing.is_none() && self.revision == revision {
            self.paused = false;
        }
    }
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

fn exchange(stream: &mut (impl Read + Write), request: &str) -> Result<String, String> {
    stream.write_all(format!("{:04x}{}", request.len(), request).as_bytes()).map_err(|e| e.to_string())?;
    let mut reply = [0; 4];
    stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
    if &reply != b"OKAY" && &reply != b"FAIL" { return Err("The endpoint did not reply as an ADB server.".into()); }
    if &reply == b"OKAY" && request == "host:kill" { return Ok(String::new()); }
    let failed = &reply == b"FAIL";
    stream.read_exact(&mut reply).map_err(|e| e.to_string())?;
    let length = usize::from_str_radix(std::str::from_utf8(&reply).map_err(|_| "Invalid ADB reply")?, 16)
        .map_err(|_| "Invalid ADB reply length")?;
    if length > 4096 { return Err("ADB reply is too large.".into()); }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).map_err(|e| e.to_string())?;
    if failed { return Err("ADB rejected the server query.".into()); }
    String::from_utf8(payload).map_err(|_| "Invalid ADB reply text".into())
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

pub fn query(socket: &str, request: &str) -> Result<Option<String>, String> {
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
        assert_eq!(version(&address).unwrap(), None);
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
