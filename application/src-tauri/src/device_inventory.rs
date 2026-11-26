use serde::{ Deserialize, Serialize };
use std::collections::{ HashMap, HashSet };
use std::time::{ Duration, Instant };
use crate::{ adb_server, mdns_discovery, screen_power::AdbClient, system_command };

const MAX_ROUTES: usize = 64;
const PROPERTIES: [&str; 8] = ["ro.serialno", "ro.product.manufacturer", "ro.product.device", "ro.product.model", "ro.build.version.release", "ro.build.version.sdk", "ro.build.id", "ro.build.version.security_patch"];
// Only fixed ASCII property names reach echo; no value/escape interpretation is
// changed. Android's builtin avoids eight formatter processes per validation.
const DETAILS: &str = "for property in ro.serialno ro.product.manufacturer ro.product.device ro.product.model ro.build.version.release ro.build.version.sdk ro.build.id ro.build.version.security_patch; do echo -n \"$property=\"; getprop \"$property\"; done";
const CHANGED: &str = "The device connection changed. Wait for the device list to refresh, then select its connection again.";

#[derive(Clone, Deserialize, Serialize)]
pub struct Selection {
    pub serial: String,
    pub transport_id: Option<u64>,
    pub identity: Option<String>,
    pub adb_path: String,
    pub server_socket: String,
}

#[derive(Clone, Serialize)]
pub struct Info {
    pub model: String,
    pub model_id: String,
    pub version: String,
    pub sdk: String,
    pub build: String,
    pub security_patch: String,
}

#[derive(Clone, Serialize)]
pub struct Route {
    pub serial: String,
    pub state: String,
    pub kind: &'static str,
    pub transport_id: Option<u64>,
    pub info: Option<Info>,
    pub selection: Selection,
    #[serde(skip)]
    model: String,
    #[serde(skip)]
    suffix: String,
}

#[derive(Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub label: String,
    pub verified: bool,
    pub routes: Vec<Route>,
}

fn valid_value(value: &str) -> bool {
    value.len() <= 256 && !value.chars().any(char::is_control)
}

fn identity_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    valid_value(value) && !value.is_empty() && !["unknown", "none", "null", "default", "android", "serial", "0123456789abcdef", "1234567890abcdef", "1234567890", "123456789"].contains(&lower.as_str())
        && !value.chars().all(|character| character == '0' || character == 'f' || character == 'F')
}

fn kind(serial: &str, usb: bool) -> &'static str {
    if serial.contains("._adb-tls-connect._tcp") { "mdns" }
    else if serial.starts_with("emulator-") { "emulator" }
    else if !usb && serial.contains(':') { "tcp" }
    else { "usb" }
}

fn instance(serial: &str) -> &str {
    serial.split_once("._adb-tls-connect._tcp").map(|(instance, _)| instance).unwrap_or(serial)
}

fn listing(output: &str, client: &AdbClient) -> Vec<Route> {
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    let mut header = false;
    for line in output.lines() {
        if line.trim() == "List of devices attached" { header = true; continue; }
        if !header || line.trim().is_empty() || line.starts_with('*') { continue; }
        let mut fields = line.split_whitespace();
        let (Some(serial), Some(state)) = (fields.next(), fields.next()) else { continue; };
        if serial.len() > 1024 || serial.starts_with('-') || serial.chars().any(char::is_control) || !seen.insert(serial.to_string()) { continue; }
        let fields: Vec<_> = fields.collect();
        let transport_id = fields.iter().find_map(|field| field.strip_prefix("transport_id:").and_then(|id| id.parse::<u64>().ok())).filter(|id| *id != 0);
        let model = fields.iter().find_map(|field| field.strip_prefix("model:")).unwrap_or_default().replace('_', " ");
        let state = if state == "no" && fields.first() == Some(&"permissions") { "no permissions" } else { state };
        let kind = kind(serial, fields.iter().any(|field| field.starts_with("usb:")));
        routes.push(Route {
            serial: serial.into(), state: state.into(), kind, transport_id, info: None,
            selection: Selection { serial: serial.into(), transport_id, identity: None, adb_path: client.path.clone(), server_socket: client.socket.clone() },
            model: if valid_value(&model) { model } else { String::new() }, suffix: instance(serial).into(),
        });
        if routes.len() == MAX_ROUTES { break; }
    }
    routes
}

fn details(output: &str) -> Option<(Info, Option<String>, String)> {
    let lines: Vec<_> = output.lines().collect();
    if lines.len() != PROPERTIES.len() { return None; }
    let mut values = Vec::new();
    for (line, property) in lines.iter().zip(PROPERTIES) {
        let (key, property_value) = line.split_once('=')?;
        if key != property || !valid_value(property_value) { return None; }
        values.push(property_value.trim());
    }
    let identity = values[..3].iter().all(|item| identity_value(item)).then(|| serde_json::to_string(&values[..3]).unwrap());
    Some((Info {
        model: values[3].into(), model_id: values[2].into(), version: values[4].into(), sdk: values[5].into(), build: values[6].into(), security_patch: values[7].into(),
    }, identity, values[0].into()))
}

fn inspect(routes: &mut [Route], query: &mut impl FnMut(&[&str]) -> Result<String, String>) {
    for route in routes {
        if route.state != "device" { continue; }
        let Some(transport_id) = route.transport_id else { continue; };
        let id = transport_id.to_string();
        let Ok(output) = query(&["-t", &id, "shell", DETAILS]) else { continue; };
        if let Some((info, identity, suffix)) = details(&output) {
            if !info.model.is_empty() { route.model = info.model.clone(); }
            if route.kind != "emulator" { route.selection.identity = identity; }
            if route.selection.identity.is_some() { route.suffix = suffix; }
            route.info = Some(info);
        }
    }
}

fn route_id(route: &Route) -> String {
    format!("route:{}:{}", route.serial, route.transport_id.unwrap_or_default())
}

fn tail(source: &str, count: usize) -> String {
    let characters: Vec<_> = source.chars().collect();
    characters[characters.len().saturating_sub(count)..].iter().collect()
}

fn group(routes: Vec<Route>) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    let mut indices: HashMap<String, usize> = HashMap::new();
    for route in routes {
        let id = route.selection.identity.as_ref().map(|identity| format!("device:{}", identity)).unwrap_or_else(|| route_id(&route));
        if let Some(index) = indices.get(&id) { devices[*index].routes.push(route); continue; }
        let name = if route.model.is_empty() { instance(&route.serial).to_string() } else { route.model.clone() };
        indices.insert(id.clone(), devices.len());
        devices.push(Device { id, label: name.clone(), name, verified: route.selection.identity.is_some(), routes: vec![route] });
    }
    for index in 0..devices.len() {
        let peers: Vec<_> = devices.iter().enumerate().filter(|(other, device)| *other != index && device.name.eq_ignore_ascii_case(&devices[index].name)).map(|(other, _)| other).collect();
        if peers.is_empty() { continue; }
        let source = &devices[index].routes[0].suffix;
        let mut count = 4;
        while count < source.chars().count().min(12) && peers.iter().any(|other| tail(source, count) == tail(&devices[*other].routes[0].suffix, count)) { count += 1; }
        let mut suffix = tail(source, count);
        if peers.iter().any(|other| suffix == tail(&devices[*other].routes[0].suffix, count)) {
            let route = &devices[index].routes[0];
            let connection = route.transport_id.map(|id| id.to_string()).unwrap_or_else(|| format!("#{}", index + 1));
            suffix = format!("{} · {} {}", tail(source, 4), route.kind, connection);
        }
        devices[index].label = format!("{} · {}", devices[index].name, suffix);
    }
    devices
}

pub fn refresh() -> Result<Vec<Device>, String> {
    let _operation = adb_server::begin(false)?;
    let client = AdbClient { path: system_command::adb_path(), socket: adb_server::endpoint()? };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut cancelled = || Instant::now() >= deadline;
    let output = client.query(&["devices", "-l"], &mut cancelled, false).map_err(|_| "Could not refresh the device list. Check the selected ADB executable and connection.")?;
    let output = mdns_discovery::refresh(output, |args| {
        client.query(if args == ["devices"] { &["devices", "-l"] } else { args }, &mut cancelled, false)
    });
    let mut routes = listing(&output, &client);
    inspect(&mut routes, &mut |args| client.query(args, &mut cancelled, false));
    // Reconcile against a fresh listing so vanished/replaced transports cannot
    // retain metadata read earlier in this refresh. New routes stay unverified.
    if let Ok(latest) = client.query(&["devices", "-l"], &mut cancelled, false) {
        let mut current = listing(&latest, &client);
        for route in &mut current {
            if let Some(previous) = routes.iter().find(|previous| previous.serial == route.serial && previous.transport_id == route.transport_id && previous.state == route.state) {
                *route = previous.clone();
            }
        }
        routes = current;
    } else {
        for route in &mut routes { route.selection.identity = None; route.info = None; route.suffix = instance(&route.serial).into(); }
    }
    Ok(group(routes))
}

fn validate_args(selection: &Selection, args: &[&str]) -> Result<(), String> {
    if args.first() != Some(&"-s") || args.get(1).copied() != Some(selection.serial.as_str()) { return Err(CHANGED.into()); }
    let mut index = 2;
    while index < args.len() {
        let arg = args[index];
        let (selector, value_next) = short_options(arg);
        if selector || ["--serial", "--select-usb", "--select-tcpip", "--tcpip"].contains(&arg)
            || arg.starts_with("--serial=") || arg.starts_with("--tcpip=") {
            return Err("Choose the connection in Devices instead of overriding its device selector in custom flags.".into());
        }
        index += if value_next { 2 } else { 1 };
    }
    Ok(())
}

fn short_options(arg: &str) -> (bool, bool) {
    let Some(options) = arg.strip_prefix('-').filter(|options| !options.starts_with('-')) else { return (false, false); };
    for (offset, option) in options.char_indices() {
        if ['s', 'd', 'e'].contains(&option) { return (true, false); }
        // Value-taking short options in scrcpy 5.0.1, plus the legacy crop alias
        // -c. The rest of this token is a value, not additional option letters.
        if ['b', 'c', 'm', 'p', 'r', 'V'].contains(&option) { return (false, offset + option.len_utf8() == options.len()); }
    }
    (false, false)
}

fn verify(selection: &Selection, client: &AdbClient, args: &[&str], query: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<(), String> {
    validate_args(selection, args)?;
    if selection.adb_path != client.path || selection.server_socket != client.socket { return Err(CHANGED.into()); }
    let output = query(&["devices", "-l"])?;
    let route = listing(&output, client).into_iter().find(|route| route.serial == selection.serial && route.transport_id == selection.transport_id && route.state == "device").ok_or(CHANGED)?;
    let Some(id) = route.transport_id else { return Err(CHANGED.into()); };
    if let Some(identity) = &selection.identity {
        let current = query(&["-t", &id.to_string(), "shell", DETAILS]).ok().and_then(|output| details(&output)).and_then(|(_, identity, _)| identity);
        if current.as_ref() != Some(identity) { return Err(CHANGED.into()); }
    }
    Ok(())
}

pub fn validate(selection: &Selection, args: &[&str]) -> Result<(), String> {
    let _operation = adb_server::begin(false)?;
    let client = AdbClient { path: system_command::adb_path(), socket: adb_server::endpoint()? };
    let deadline = Instant::now() + Duration::from_secs(6);
    verify(selection, &client, args, &mut |args| client.query(args, &mut || Instant::now() >= deadline, false))
}

pub fn disconnect(selection: &Selection) -> Result<(), String> {
    let _operation = adb_server::begin(false)?;
    let client = AdbClient { path: system_command::adb_path(), socket: adb_server::endpoint()? };
    let deadline = Instant::now() + Duration::from_secs(6);
    disconnect_checked(selection, &client, &mut |args| client.query(args, &mut || Instant::now() >= deadline, false))
}

fn disconnect_checked(selection: &Selection, client: &AdbClient, query: &mut impl FnMut(&[&str]) -> Result<String, String>) -> Result<(), String> {
    if selection.adb_path != client.path || selection.server_socket != client.socket { return Err(CHANGED.into()); }
    let output = query(&["devices", "-l"])?;
    let route = listing(&output, client).into_iter().find(|route| route.serial == selection.serial && route.transport_id == selection.transport_id).ok_or(CHANGED)?;
    if !["tcp", "mdns"].contains(&route.kind) { return Err("Only wireless connections can be disconnected here.".into()); }
    if route.state == "device" && selection.identity.is_some() {
        let Some(id) = route.transport_id else { return Err(CHANGED.into()); };
        let current = query(&["-t", &id.to_string(), "shell", DETAILS]).ok().and_then(|output| details(&output)).and_then(|(_, identity, _)| identity);
        if current != selection.identity { return Err(CHANGED.into()); }
    }
    query(&["disconnect", &selection.serial]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Read-only live inventory timing; requires explicitly selected existing ADB and Pixel transport, never reads credentials or sends input"]
    fn live_inventory_timing_without_input() {
        let client = AdbClient { path: std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap(), socket: "tcp:localhost:5037".into() };
        let transport = std::env::var("SCRCPY_PLUS_TEST_TRANSPORT").unwrap();
        assert!(adb_server::version(&client.socket).unwrap().is_some());
        let original = DETAILS.replace("echo -n \"$property=\"", "printf '%s=' \"$property\"");
        assert_ne!(original, DETAILS);
        let mut external = Vec::new();
        let mut builtin = Vec::new();
        let mut expected = None;
        for trial in 0..5 {
            for fast in if trial % 2 == 0 { [false, true] } else { [true, false] } {
                let started = Instant::now();
                let output = client.query(&["-t", &transport, "shell", if fast { DETAILS } else { &original }], &mut || false, false).unwrap();
                let (_, identity, _) = details(&output).expect("Every measured response must pass the existing strict inventory parser.");
                assert!(identity.is_some());
                if let Some(expected) = &expected { assert!(expected == &output, "Both commands must return exactly the same verified property data."); } else { expected = Some(output); }
                if fast { builtin.push(started.elapsed().as_millis()); } else { external.push(started.elapsed().as_millis()); }
            }
        }
        let mut external_sorted = external.clone(); external_sorted.sort();
        let mut builtin_sorted = builtin.clone(); builtin_sorted.sort();
        println!("Validated inventory timing: external_ms={:?}, external_median_ms={}, builtin_ms={:?}, builtin_median_ms={}, identical_output=true, credential_access=false, input_sent=false", external, external_sorted[2], builtin, builtin_sorted[2]);
    }

    fn client() -> AdbClient { AdbClient { path: "synthetic-adb".into(), socket: "tcp:127.0.0.1:1".into() } }

    fn properties(serial: &str, maker: &str, product: &str) -> String {
        PROPERTIES.iter().zip([serial, maker, product, "Example Phone", "17", "37", "TEST.1", "2026-11-01"]).map(|(key, value)| format!("{}={}\n", key, value)).collect()
    }

    #[test]
    fn groups_only_matching_identity_read_from_pinned_online_transports() {
        let mut routes = listing("List of devices attached\nUSB123 device usb:1 model:Example_Phone transport_id:1\n10.23.4.5:5555 device model:Example_Phone transport_id:2\nadb-random-name._adb-tls-connect._tcp device model:Example_Phone transport_id:3\n", &client());
        let mut ids = Vec::new();
        inspect(&mut routes, &mut |args| { assert_eq!(args[0], "-t"); ids.push(args[1].to_string()); Ok(properties("PHONE1234", "Example", "example")) });
        let devices = group(routes);
        assert_eq!(ids, ["1", "2", "3"]);
        assert_eq!(devices.len(), 1);
        assert!(devices[0].verified);
        assert_eq!(devices[0].label, "Example Phone");
        assert_eq!(devices[0].routes.iter().map(|route| route.kind).collect::<Vec<_>>(), ["usb", "tcp", "mdns"]);
        assert_eq!(devices[0].routes[2].serial, "adb-random-name._adb-tls-connect._tcp");
    }

    #[test]
    fn keeps_unknown_offline_unauthorized_and_conflicting_phones_separate() {
        let mut routes = listing("List of devices attached\nusb-one device transport_id:1\nusb-two device transport_id:2\n10.23.4.5:5555 offline transport_id:3\nadb-unknown._adb-tls-connect._tcp unauthorized transport_id:4\nusb-five device transport_id:5\nusb-six device transport_id:6\n", &client());
        let mut queries = 0;
        inspect(&mut routes, &mut |args| {
            queries += 1;
            Ok(match args[1] {
                "1" => properties("PHONE1234", "Example", "one"),
                "2" => properties("PHONE1234", "Other", "one"),
                "5" => properties("unknown", "Example", "one"),
                "6" => properties("00000000", "Example", "one"),
                _ => panic!("Offline/unauthorized routes must not be queried"),
            })
        });
        let devices = group(routes);
        assert_eq!(queries, 4);
        assert_eq!(devices.len(), 6);
        assert_eq!(devices.iter().filter(|device| device.verified).count(), 2);
    }

    #[test]
    fn disambiguates_equal_models_even_when_serial_suffixes_collide() {
        let mut routes = listing("List of devices attached\none device transport_id:1\ntwo device transport_id:2\n", &client());
        inspect(&mut routes, &mut |args| Ok(properties(if args[1] == "1" { "FIRST1234" } else { "SECOND1234" }, "Example", "one")));
        let devices = group(routes);
        assert_ne!(devices[0].label, devices[1].label);
        assert!(devices.iter().all(|device| device.label.starts_with("Example Phone · ")));
    }

    #[test]
    fn long_common_suffixes_use_a_short_unique_connection_hint() {
        let mut routes = listing("List of devices attached\none device transport_id:1\ntwo device transport_id:2\n", &client());
        inspect(&mut routes, &mut |args| Ok(properties(&format!("{}{}", if args[1] == "1" { "FIRST" } else { "SECOND" }, "1234567890".repeat(12)), "Example", "one")));
        let devices = group(routes);
        assert_ne!(devices[0].label, devices[1].label);
        assert!(devices[0].label.ends_with("7890 · usb 1"));
        assert!(devices[1].label.ends_with("7890 · usb 2"));
        assert!(devices.iter().all(|device| device.label.chars().count() - device.name.chars().count() < 24));
    }

    #[test]
    fn rejects_missing_duplicate_or_injected_property_lines() {
        assert!(details("ro.serialno=PHONE1234\n").is_none());
        assert!(details(&properties("PHONE\nro.serialno=OTHER", "Example", "one")).is_none());
        assert!(details(&properties("PHONE1234", "Example", "one").replace("ro.product.model=Example Phone", "ro.serialno=OTHER")).is_none());
        assert!(details(&properties("0123456789ABCDEF", "Example", "one")).unwrap().1.is_none());
    }

    #[test]
    fn listings_are_bounded_and_keep_full_long_and_ipv6_selectors() {
        let long = format!("adb-{}._adb-tls-connect._tcp", "a".repeat(63));
        let output = format!("List of devices attached\n{} device transport_id:7\n{} device transport_id:7\n[fd00::1234]:5555 device transport_id:8\n", long, long);
        let routes = listing(&output, &client());
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].serial, long);
        assert_eq!(routes[1].kind, "tcp");
        let huge = format!("List of devices attached\n{}", (0..100).map(|id| format!("device-{} device transport_id:{}\n", id, id + 1)).collect::<String>());
        assert_eq!(listing(&huge, &client()).len(), MAX_ROUTES);
    }

    #[test]
    fn launch_rechecks_transport_identity_and_captured_server() {
        let client = client();
        let output = "List of devices attached\n10.23.4.5:5555 device transport_id:7\n";
        let mut routes = listing(output, &client);
        inspect(&mut routes, &mut |_| Ok(properties("PHONE1234", "Example", "one")));
        let selection = routes[0].selection.clone();
        let args = ["-s", selection.serial.as_str(), "--no-audio"];
        let verify_with = |listing: &str, serial: &str| verify(&selection, &client, &args, &mut |args| Ok(if args == ["devices", "-l"] { listing.into() } else { properties(serial, "Example", "one") }));
        assert!(verify_with(output, "PHONE1234").is_ok());
        assert!(verify_with(output, "OTHER5678").is_err());
        assert!(verify_with(&output.replace("transport_id:7", "transport_id:8"), "PHONE1234").is_err());
        assert!(verify_with(&output.replace(" device ", " offline "), "PHONE1234").is_err());
        let other_client = AdbClient { path: client.path.clone(), socket: "tcp:127.0.0.1:2".into() };
        assert!(verify(&selection, &other_client, &args, &mut |_| panic!("Must reject the changed server before queries")).is_err());
        for flag in ["--serial=other", "-sother", "--tcpip=10.23.4.6", "--select-usb", "-d", "-e", "-fd", "-fSe"] {
            assert!(validate_args(&selection, &["-s", &selection.serial, flag]).is_err());
        }
    }

    #[test]
    fn normal_short_option_values_and_non_selector_clusters_remain_available() {
        let selection = listing("List of devices attached\nselected device transport_id:7\n", &client()).remove(0).selection;
        for flag in ["-m1024", "-b8M", "-p27183", "-Vdebug", "-rdeleted-scenes.mp4", "-c1024:768:0:0", "-fSwn", "-frselected-video.mp4"] {
            assert!(validate_args(&selection, &["-s", &selection.serial, flag]).is_ok(), "Valid value/cluster rejected: {}", flag);
        }
        assert!(validate_args(&selection, &["-s", &selection.serial, "-r", "-selected-video.mp4", "-V", "debug"]).is_ok());
        for flag in ["-fsother", "-fnd", "-fwe", "-sf", "-es"] {
            assert!(validate_args(&selection, &["-s", &selection.serial, flag]).is_err(), "Selector cluster accepted: {}", flag);
        }
        assert!(validate_args(&selection, &["-s", &selection.serial, "-r", "recording.mp4", "-d"]).is_err());
    }

    #[test]
    fn failed_identity_reads_never_reuse_a_previous_refresh() {
        let output = "List of devices attached\n10.23.4.5:5555 device model:Example_Phone transport_id:7\nadb-other._adb-tls-connect._tcp device model:Example_Phone transport_id:8\n";
        let mut previous = listing(output, &client());
        inspect(&mut previous, &mut |_| Ok(properties("PHONE1234", "Example", "one")));
        assert_eq!(group(previous).len(), 1);
        let mut current = listing(output, &client());
        inspect(&mut current, &mut |args| if args[1] == "7" { Err("timeout".into()) } else { Ok(properties("OTHER5678", "Example", "one")) });
        let devices = group(current);
        assert_eq!(devices.len(), 2);
        assert!(!devices[0].verified);
        assert!(devices[0].routes[0].selection.identity.is_none());
        assert!(devices[1].verified);
    }

    #[test]
    fn disconnect_removes_only_the_selected_current_wireless_route() {
        let client = client();
        let output = "List of devices attached\n10.23.4.5:5555 offline transport_id:7\nUSB123 device usb:1 transport_id:8\n";
        let routes = listing(output, &client);
        let mut calls = Vec::new();
        disconnect_checked(&routes[0].selection, &client, &mut |args| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
            Ok(if args == ["devices", "-l"] { output.into() } else { String::new() })
        }).unwrap();
        assert_eq!(calls, [vec!["devices", "-l"], vec!["disconnect", "10.23.4.5:5555"]]);
        assert!(disconnect_checked(&routes[1].selection, &client, &mut |_| Ok(output.into())).is_err());
        assert!(disconnect_checked(&routes[0].selection, &client, &mut |_| Ok(output.replace("transport_id:7", "transport_id:9"))).is_err());
    }

    #[test]
    #[ignore = "Requires an explicitly selected ADB and an existing server; reads inventory and validates selections without launching mirrors"]
    fn inspect_live_inventory_without_changing_devices() {
        let adb = std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap();
        system_command::set_custom_paths(None, Some(adb));
        assert!(adb_server::version(&adb_server::endpoint().unwrap()).unwrap().is_some(), "The read-only test requires an already-running server");
        let devices = refresh().unwrap();
        assert!(!devices.is_empty(), "Connect a test device before running this opt-in check");
        let verified = devices.iter().filter(|device| device.verified).count();
        let routes: usize = devices.iter().map(|device| device.routes.len()).sum();
        for device in &devices {
            for route in &device.routes {
                if route.state == "device" && route.transport_id.is_some() {
                    validate(&route.selection, &["-s", &route.serial, "--no-audio"]).unwrap();
                }
            }
        }
        println!("Read {} device entries, {} verified identities, and {} connection paths; selection rechecks passed without launching mirrors.", devices.len(), verified, routes);
    }

    #[test]
    #[ignore = "Requires a caller-created extra wireless route; groups it, disconnects only that route, and verifies the original path remains"]
    fn check_live_test_owned_duplicate_route() {
        let adb = std::env::var("SCRCPY_PLUS_TEST_ADB").unwrap();
        let serial = std::env::var("SCRCPY_PLUS_TEST_CREATED_ROUTE").unwrap();
        system_command::set_custom_paths(None, Some(adb));
        assert!(adb_server::version(&adb_server::endpoint().unwrap()).unwrap().is_some());
        let devices = refresh().unwrap();
        let device = devices.iter().find(|device| device.routes.iter().any(|route| route.serial == serial)).expect("The caller-created route must be online");
        assert!(device.verified && device.routes.len() > 1, "Both paths must share a freshly verified physical identity");
        let created = device.routes.iter().find(|route| route.serial == serial).unwrap();
        let originals: Vec<_> = device.routes.iter().filter(|route| route.serial != serial).map(|route| route.serial.clone()).collect();
        validate(&created.selection, &["-s", &serial]).unwrap();
        disconnect(&created.selection).unwrap();
        let after = refresh().unwrap();
        let remaining = after.iter().find(|remaining| remaining.id == device.id).expect("The original physical device must remain listed");
        assert!(originals.iter().all(|serial| remaining.routes.iter().any(|route| &route.serial == serial && route.state == "device")));
        assert!(remaining.routes.iter().all(|route| route.serial != serial));
        println!("Verified one physical entry with multiple real connection paths; checked disconnect removed only the test-owned path.");
    }
}
