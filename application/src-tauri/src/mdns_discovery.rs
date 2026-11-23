use serde::{ Deserialize, Serialize };
use socket2::{ Domain, Protocol, Socket, Type };
use std::fs::{ self, OpenOptions };
use std::io::{ Read, Write };
use std::net::{ Ipv4Addr, SocketAddrV4 };
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{ Duration, Instant, SystemTime, UNIX_EPOCH };

const INTERVAL: Duration = Duration::from_secs(30);
const MAX_PEERS: usize = 16;
const MAX_AGE: u64 = 30 * 24 * 60 * 60;
const SERVICE: &str = "_adb-tls-connect._tcp";
static DISCOVERY: Mutex<Option<Discovery>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Peer {
    address: Ipv4Addr,
    last_seen: u64,
}

struct Discovery {
    path: PathBuf,
    peers: Vec<Peer>,
    last_scan: Option<Instant>,
}

fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn local_address(address: Ipv4Addr) -> bool {
    address.is_private() || address.is_link_local()
}

fn endpoint_address(endpoint: &str) -> Option<Ipv4Addr> {
    let endpoint: SocketAddrV4 = endpoint.parse().ok()?;
    (endpoint.port() != 0 && local_address(*endpoint.ip())).then_some(*endpoint.ip())
}

fn local_server(endpoint: &str) -> bool {
    if endpoint.starts_with("localfilesystem:") { return true; }
    let Some(address) = endpoint.strip_prefix("tcp:") else { return false; };
    if address.parse::<u16>().is_ok_and(|port| port != 0) { return true; }
    if let Some(port) = address.strip_prefix("localhost:") {
        return port.parse::<u16>().is_ok_and(|port| port != 0);
    }
    address.parse::<std::net::SocketAddr>().is_ok_and(|address| address.ip().is_loopback() && address.port() != 0)
}

// Only associate advertised addresses with transports ADB already reports online.
// These addresses are discovery hints, never physical device or credential identity.
fn connected_addresses(devices: &str, services: &str) -> Vec<Ipv4Addr> {
    let online: Vec<_> = devices.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let serial = fields.next()?;
        (fields.next()? == "device").then_some(serial)
    }).collect();
    let mut addresses: Vec<_> = online.iter().filter_map(|serial| endpoint_address(serial)).collect();
    for line in services.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 3 || !fields[1].eq_ignore_ascii_case(SERVICE) { continue; }
        let serial = format!("{}.{}", fields[0].trim_end_matches('.'), SERVICE);
        if online.iter().any(|online| online.trim_end_matches('.').eq_ignore_ascii_case(&serial)) {
            if let Some(address) = endpoint_address(fields[2]) { addresses.push(address); }
        }
    }
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

fn advertised_addresses(services: &str) -> Vec<Ipv4Addr> {
    services.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        fields.next()?;
        if !fields.next()?.eq_ignore_ascii_case(SERVICE) { return None; }
        endpoint_address(fields.next()?)
    }).collect()
}

impl Discovery {
    fn new(path: PathBuf) -> Self {
        let peers = (|| -> Option<Vec<Peer>> {
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16384 { return None; }
            let mut data = Vec::new();
            fs::File::open(&path).ok()?.take(16385).read_to_end(&mut data).ok()?;
            if data.len() > 16384 { return None; }
            serde_json::from_slice(&data).ok()
        })().unwrap_or_default();
        let mut discovery = Self { path, peers, last_scan: None };
        discovery.prune(timestamp());
        discovery
    }

    fn prune(&mut self, now: u64) {
        self.peers.retain(|peer| local_address(peer.address) && peer.last_seen <= now && now - peer.last_seen <= MAX_AGE);
        self.peers.sort_by_key(|peer| std::cmp::Reverse(peer.last_seen));
        let mut addresses = Vec::new();
        self.peers.retain(|peer| {
            if addresses.contains(&peer.address) { return false; }
            addresses.push(peer.address);
            true
        });
        self.peers.truncate(MAX_PEERS);
    }

    fn remember(&mut self, addresses: Vec<Ipv4Addr>, now: u64) {
        let before = self.peers.clone();
        self.prune(now);
        for address in addresses {
            if !local_address(address) { continue; }
            if let Some(peer) = self.peers.iter_mut().find(|peer| peer.address == address) {
                // Avoid rewriting the cache on every five-second device refresh.
                if now.saturating_sub(peer.last_seen) >= 3600 { peer.last_seen = now; }
            } else {
                self.peers.insert(0, Peer { address, last_seen: now });
            }
        }
        self.prune(now);
        if self.peers != before { let _ = self.save(); }
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let directory = self.path.parent().ok_or("Missing discovery cache directory")?;
        fs::create_dir_all(directory)?;
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink()) {
            return Err("Invalid discovery cache file".into());
        }
        let temporary = self.path.with_extension(format!("{}.tmp", std::process::id()));
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            file.write_all(&serde_json::to_vec(&self.peers)?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result
    }

    fn refresh(&mut self, devices: String, command: &mut impl FnMut(&[&str]) -> Result<String, String>, probe: &mut impl FnMut(&[Ipv4Addr]) -> bool) -> String {
        self.remember(connected_addresses(&devices, ""), timestamp());
        if self.last_scan.is_some_and(|last| last.elapsed() < INTERVAL) { return devices; }
        self.last_scan = Some(Instant::now());
        let Ok(services) = command(&["mdns", "services"]) else { return devices; };
        self.remember(connected_addresses(&devices, &services), timestamp());
        let advertised = advertised_addresses(&services);
        let missing: Vec<_> = self.peers.iter().filter(|peer| !advertised.contains(&peer.address)).map(|peer| peer.address).collect();
        if missing.is_empty() || !probe(&missing) { return devices; }
        // ADB receives the phone's multicast response and authenticates any connection.
        // Never reconnect through a cached debugging port or trust an IP as identity.
        let refreshed = command(&["devices"]).unwrap_or(devices);
        if let Ok(services) = command(&["mdns", "services"]) {
            self.remember(connected_addresses(&refreshed, &services), timestamp());
        }
        refreshed
    }
}

fn same_subnet(peer: Ipv4Addr, interface: Ipv4Addr, netmask: Ipv4Addr) -> bool {
    let mask = u32::from(netmask);
    let prefix = mask.leading_ones();
    if !(1..=30).contains(&prefix) || mask != u32::MAX << (32 - prefix) { return false; }
    let host = u32::from(peer) & !mask;
    local_address(peer) && local_address(interface) && peer != interface && host != 0 && host != !mask
        && u32::from(peer) & mask == u32::from(interface) & mask
}

fn question() -> Vec<u8> {
    let mut packet = vec![0; 12];
    packet[5] = 1;
    for label in "_adb-tls-connect._tcp.local".split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.extend_from_slice(&[0, 0, 12, 0, 1]); // PTR, IN; request a multicast response.
    packet
}

fn probe(addresses: &[Ipv4Addr]) -> bool {
    let Ok(interfaces) = if_addrs::get_if_addrs() else { return false; };
    let routes: Vec<_> = addresses.iter().take(MAX_PEERS).filter_map(|peer| {
        interfaces.iter().filter(|interface| interface.is_oper_up() && !interface.is_p2p()).filter_map(|interface| {
            let if_addrs::IfAddr::V4(address) = &interface.addr else { return None; };
            same_subnet(*peer, address.ip, address.netmask).then_some((*peer, address.ip, address.prefixlen))
        }).max_by_key(|route| route.2)
    }).collect();
    if routes.is_empty() { return false; }
    let result = (|| -> std::io::Result<bool> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        // Sharing source port 5353 is required: an ephemeral source did not recover
        // discovery in the reproduced failure. Leave ADB's own listener in place.
        socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 5353).into())?;
        socket.set_ttl_v4(255)?;
        socket.set_write_timeout(Some(Duration::from_millis(100)))?;
        let multicast = Ipv4Addr::new(224, 0, 0, 251);
        let packet = question();
        let mut joined = Vec::new();
        let mut sent = false;
        for (peer, interface, _) in routes {
            if !joined.contains(&interface) {
                if socket.join_multicast_v4(&multicast, &interface).is_err() { continue; }
                joined.push(interface);
            }
            sent |= socket.send_to(&packet, &SocketAddrV4::new(peer, 5353).into()).is_ok();
        }
        // Keep the shared membership alive while ADB ingests the response.
        if sent { std::thread::sleep(Duration::from_secs(1)); }
        Ok(sent)
    })();
    result.unwrap_or(false)
}

pub fn configure(directory: PathBuf) {
    *DISCOVERY.lock().unwrap() = Some(Discovery::new(directory.join("known-mdns-peers.json")));
}

pub fn refresh(devices: String, mut command: impl FnMut(&[&str]) -> Result<String, String>) -> String {
    if !crate::adb_server::endpoint().is_ok_and(|endpoint| local_server(&endpoint)) { return devices; }
    let Ok(mut discovery) = DISCOVERY.try_lock() else { return devices; };
    match discovery.as_mut() {
        Some(discovery) => discovery.refresh(devices, &mut command, &mut probe),
        None => devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCache { directory: PathBuf }

    impl TestCache {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let directory = std::env::temp_dir().join(format!("scrcpy-plus-mdns-{}-{}", std::process::id(), nonce));
            fs::create_dir(&directory).unwrap();
            Self { directory }
        }

        fn path(&self) -> PathBuf { self.directory.join("known-mdns-peers.json") }
    }

    impl Drop for TestCache {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.path());
            let _ = fs::remove_dir(&self.directory);
        }
    }

    #[test]
    fn learns_only_addresses_of_online_transports() {
        let devices = "List of devices attached\nadb-sample._adb-tls-connect._tcp\tdevice\nadb-offline._adb-tls-connect._tcp\toffline\n10.23.4.8:45678\tdevice\n10.23.4.9:45678\tunauthorized\n8.8.8.8:45678\tdevice\n";
        let services = "List of discovered mdns services\nadb-sample _adb-tls-connect._tcp 10.23.4.7:45678\nadb-offline _adb-tls-connect._tcp 10.23.4.9:45678\nadb-unknown _adb-tls-connect._tcp 10.23.4.10:45678\nadb-sample _adb-tls-pairing._tcp 10.23.4.11:45678\n";
        assert_eq!(connected_addresses(devices, services), vec![Ipv4Addr::new(10, 23, 4, 7), Ipv4Addr::new(10, 23, 4, 8)]);
        assert_eq!(connected_addresses("ADB-SAMPLE._ADB-TLS-CONNECT._TCP. device", services), vec![Ipv4Addr::new(10, 23, 4, 7)]);
        for endpoint in ["127.0.0.1:5555", "224.0.0.251:5353", "10.23.4.7:0", "10.23.4.7:65536", "10.23.4.7", "phone.local:5555", "[::1]:5555"] {
            assert_eq!(endpoint_address(endpoint), None);
        }
    }

    #[test]
    fn restricts_recovery_to_local_servers_and_connected_subnets() {
        for endpoint in ["tcp:5037", "tcp:localhost:5037", "tcp:127.0.0.1:5037", "tcp:[::1]:5037", "localfilesystem:/tmp/adb.sock"] {
            assert!(local_server(endpoint));
        }
        for endpoint in ["tcp:10.23.4.2:5037", "tcp:example.com:5037", "tcp:localhost:0", "tcp:0", "other:5037"] {
            assert!(!local_server(endpoint));
        }
        let interface = Ipv4Addr::new(10, 23, 4, 2);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        assert!(same_subnet(Ipv4Addr::new(10, 23, 4, 7), interface, mask));
        for peer in [interface, Ipv4Addr::new(10, 23, 4, 0), Ipv4Addr::new(10, 23, 4, 255), Ipv4Addr::new(10, 23, 5, 7), Ipv4Addr::new(8, 8, 8, 8)] {
            assert!(!same_subnet(peer, interface, mask));
        }
        assert!(!same_subnet(Ipv4Addr::new(10, 23, 4, 7), interface, Ipv4Addr::UNSPECIFIED));
        assert!(!same_subnet(Ipv4Addr::new(10, 23, 4, 7), interface, Ipv4Addr::new(255, 0, 255, 0)));
        assert!(same_subnet(Ipv4Addr::new(169, 254, 7, 3), Ipv4Addr::new(169, 254, 7, 2), Ipv4Addr::new(255, 255, 0, 0)));
    }

    #[test]
    fn sends_a_standard_ptr_question_requesting_multicast_answers() {
        assert_eq!(question(), b"\0\0\0\0\0\x01\0\0\0\0\0\0\x10_adb-tls-connect\x04_tcp\x05local\0\0\x0c\0\x01");
    }

    #[test]
    fn cache_is_bounded_expiring_deduplicated_and_tolerates_damage() {
        let cache = TestCache::new();
        fs::write(cache.path(), b"not json").unwrap();
        let mut discovery = Discovery::new(cache.path());
        assert!(discovery.peers.is_empty());
        let now = timestamp();
        discovery.peers = vec![
            Peer { address: Ipv4Addr::new(10, 23, 4, 7), last_seen: now - MAX_AGE - 1 },
            Peer { address: Ipv4Addr::new(10, 23, 4, 8), last_seen: now + 1 },
            Peer { address: Ipv4Addr::new(8, 8, 8, 8), last_seen: now },
        ];
        let newest = Ipv4Addr::new(10, 23, 4, 39);
        discovery.remember((20..=39).map(|host| Ipv4Addr::new(10, 23, 4, host)).collect(), now);
        assert_eq!(discovery.peers.len(), MAX_PEERS);
        assert_eq!(discovery.peers[0].address, newest);
        assert!(discovery.peers.iter().all(|peer| peer.last_seen == now));
        discovery.remember(vec![newest, newest], now + 3600);
        let loaded = Discovery::new(cache.path());
        // The synthetic future timestamp must be discarded when loading at real time.
        assert_eq!(loaded.peers.len(), MAX_PEERS - 1);
        assert_eq!(discovery.peers.iter().filter(|peer| peer.address == newest).count(), 1);
        fs::write(cache.path(), vec![b' '; 16385]).unwrap();
        assert!(Discovery::new(cache.path()).peers.is_empty());
    }

    #[test]
    fn remembers_connections_without_rewriting_them_every_poll() {
        let cache = TestCache::new();
        let mut discovery = Discovery::new(cache.path());
        let now = timestamp();
        let address = Ipv4Addr::new(10, 23, 4, 7);
        discovery.remember(vec![address], now);
        assert_eq!(Discovery::new(cache.path()).peers, discovery.peers);
        discovery.remember(vec![address], now + 30);
        assert_eq!(discovery.peers[0].last_seen, now);
        discovery.remember(vec![address], now + 3600);
        assert_eq!(discovery.peers[0].last_seen, now + 3600);
    }

    #[test]
    fn recovers_missing_services_and_throttles_duplicate_queries() {
        let cache = TestCache::new();
        let mut discovery = Discovery::new(cache.path());
        let devices = "List of devices attached\n10.23.4.7:45678 device\n".to_string();
        let refreshed = "List of devices attached\nadb-sample._adb-tls-connect._tcp device\n".to_string();
        let mut calls = Vec::new();
        let mut probes = 0;
        let mut command = |args: &[&str]| {
            calls.push(args.join(" "));
            if args == ["devices"] { return Ok(refreshed.clone()); }
            if calls.len() == 1 { return Ok("List of discovered mdns services\n".into()); }
            Ok("adb-sample _adb-tls-connect._tcp 10.23.4.7:45679\n".into())
        };
        let mut probe = |addresses: &[Ipv4Addr]| {
            assert_eq!(addresses, &[Ipv4Addr::new(10, 23, 4, 7)]);
            probes += 1;
            true
        };
        assert_eq!(discovery.refresh(devices.clone(), &mut command, &mut probe), refreshed);
        assert_eq!(discovery.refresh(devices.clone(), &mut command, &mut probe), devices);
        assert_eq!(probes, 1);
        assert_eq!(calls, vec!["mdns services", "devices", "mdns services"]);
    }

    #[test]
    fn leaves_device_results_intact_when_discovery_is_unavailable() {
        let cache = TestCache::new();
        let mut discovery = Discovery::new(cache.path());
        let devices = "List of devices attached\n10.23.4.7:45678 device\n".to_string();
        assert_eq!(discovery.refresh(devices.clone(), &mut |_| Err("unsupported".into()), &mut |_| panic!("must not probe")), devices);
        discovery.last_scan = None;
        let mut calls = 0;
        let mut command = |_: &[&str]| {
            calls += 1;
            if calls == 1 { Ok(String::new()) } else { Err("temporary failure".into()) }
        };
        assert_eq!(discovery.refresh(devices.clone(), &mut command, &mut |_| true), devices);
        assert_eq!(calls, 3);
        discovery.last_scan = None;
        let advertised = "adb-sample _adb-tls-connect._tcp 10.23.4.7:45678".to_string();
        assert_eq!(discovery.refresh(devices.clone(), &mut |_| Ok(advertised.clone()), &mut |_| panic!("already advertised")), devices);
    }

    #[test]
    #[ignore = "Requires an existing local ADB server and a connected wireless device"]
    fn probe_known_connected_peer() {
        let executable = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Set SCRCPY_PLUS_TEST_ADB");
        let directory = std::env::var_os("SCRCPY_PLUS_TEST_MDNS_CACHE").expect("Set SCRCPY_PLUS_TEST_MDNS_CACHE to an isolated cache directory");
        assert!(crate::adb_server::version(&crate::adb_server::endpoint().unwrap()).unwrap().is_some());
        crate::system_command::set_custom_paths(None, Some(executable));
        let devices = crate::system_command::adb(vec!["devices"]).unwrap();
        let services = crate::system_command::adb(vec!["mdns", "services"]).unwrap();
        let addresses = connected_addresses(&devices, &services);
        assert!(!addresses.is_empty(), "A connected wireless peer must first be discovered or connected normally");
        let mut discovery = Discovery::new(PathBuf::from(directory).join("known-mdns-peers.json"));
        discovery.remember(addresses.clone(), timestamp());
        assert!(probe(&addresses), "Could not send a local mDNS query");
        println!("Queried {} known peer(s); addresses and device identifiers are omitted.", addresses.len());
    }

    #[test]
    #[ignore = "Requires a previously learned isolated cache and a missing live mDNS service"]
    fn recover_from_cached_peer() {
        let executable = std::env::var("SCRCPY_PLUS_TEST_ADB").expect("Set SCRCPY_PLUS_TEST_ADB");
        let directory = std::env::var_os("SCRCPY_PLUS_TEST_MDNS_CACHE").expect("Set SCRCPY_PLUS_TEST_MDNS_CACHE to the previously learned isolated cache");
        assert!(crate::adb_server::version(&crate::adb_server::endpoint().unwrap()).unwrap().is_some());
        crate::system_command::set_custom_paths(None, Some(executable));
        let mut discovery = Discovery::new(PathBuf::from(directory).join("known-mdns-peers.json"));
        assert!(!discovery.peers.is_empty(), "Learn a connected wireless peer first");
        let before = crate::system_command::adb(vec!["mdns", "services"]).unwrap();
        let advertised = advertised_addresses(&before);
        let missing: Vec<_> = discovery.peers.iter().filter(|peer| !advertised.contains(&peer.address)).map(|peer| peer.address).collect();
        assert!(!missing.is_empty(), "This test requires a known peer's service to be missing");
        let devices = crate::system_command::adb(vec!["devices"]).unwrap();
        let recovered = discovery.refresh(devices, &mut |args| crate::system_command::adb(args.to_vec()), &mut probe);
        let after = crate::system_command::adb(vec!["mdns", "services"]).unwrap();
        let addresses = connected_addresses(&recovered, &after);
        assert!(addresses.iter().any(|address| missing.contains(address)), "The missing cached peer was not rediscovered and online");
        println!("Recovered a missing known mDNS service and online transport without restarting ADB.");
    }
}
