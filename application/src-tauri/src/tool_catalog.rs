use serde::{ Deserialize, Serialize };
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{ HashMap, HashSet };
use std::io::Read;
use std::time::Duration;
use quick_xml::{ events::{ BytesStart, Event }, Reader };

const GOOGLE_REPOSITORY: &str = "https://dl.google.com/android/repository/";
const GOOGLE_METADATA: &str = "https://dl.google.com/android/repository/repository2-3.xml";
const GOOGLE_RELEASES: &str = "https://developer.android.com/tools/releases/platform-tools";
const SCRCPY_API: &str = "https://api.github.com/repos/Genymobile/scrcpy/releases";
const SCRCPY_DOWNLOAD: &str = "https://github.com/Genymobile/scrcpy/releases/download/";
const SCRCPY_PAGE: &str = "https://github.com/Genymobile/scrcpy/releases/tag/";
const MAX_METADATA: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const INVALID_GOOGLE: &str = "Google's Platform-Tools metadata was incomplete or ambiguous.";
const INVALID_SCRCPY: &str = "The official scrcpy release metadata was incomplete or ambiguous.";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub release_url: String,
    pub checksum: String,
    pub checksum_type: String,
    pub size: u64,
}

fn numeric_version(version: &str, build: bool) -> Option<Vec<u32>> {
    if version.is_empty() || version.len() > 80 { return None; }
    let version = version.strip_prefix('v').or_else(|| version.strip_prefix('V')).unwrap_or(version);
    let version = if let Some((version, suffix)) = version.split_once('-') {
        if !build || suffix.is_empty() || suffix.len() > 20 || !suffix.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
        version
    } else { version };
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() < 2 || parts.len() > 4 { return None; }
    parts.into_iter().map(|part| {
        if part.is_empty() || part.len() > 9 || !part.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
        part.parse().ok()
    }).collect()
}

pub fn compare_versions(first: &str, second: &str) -> Option<Ordering> {
    // ADB reports the Platform-Tools revision followed by a numeric build ID.
    // That build ID is retained for display but is not another release component.
    let first = numeric_version(first, true)?;
    let second = numeric_version(second, true)?;
    for index in 0..first.len().max(second.len()) {
        let ordering = first.get(index).unwrap_or(&0).cmp(second.get(index).unwrap_or(&0));
        if ordering != Ordering::Equal { return Some(ordering); }
    }
    Some(Ordering::Equal)
}

fn fetch(url: &str, limit: u64) -> Result<String, String> {
    // GitHub's selected checksum asset redirects to its HTTPS asset storage.
    // Metadata endpoints themselves must not redirect to a different source.
    let redirects = if url.starts_with(SCRCPY_DOWNLOAD) { 3 } else { 0 };
    let response = ureq::get(url).header("User-Agent", concat!("scrcpy-plus/", env!("CARGO_PKG_VERSION")))
        .config().timeout_global(Some(Duration::from_secs(15))).max_redirects(redirects).https_only(true).build().call()
        .map_err(|_| "Could not read official update metadata. Check the connection and try again.")?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().take(limit + 1).read_to_end(&mut bytes).map_err(|_| "The official update metadata download was interrupted.")?;
    if bytes.len() as u64 > limit { return Err("The official update metadata exceeded the size limit.".into()); }
    String::from_utf8(bytes).map_err(|_| "The official update metadata was not valid UTF-8.".into())
}

#[derive(Default)]
struct Node {
    name: String,
    attributes: HashMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn one(&self, name: &str) -> Result<&Node, String> {
        self.optional(name)?.ok_or_else(|| INVALID_GOOGLE.into())
    }

    fn optional(&self, name: &str) -> Result<Option<&Node>, String> {
        let mut nodes = self.children.iter().filter(|node| node.name == name);
        let first = nodes.next();
        if nodes.next().is_some() { return Err(INVALID_GOOGLE.into()); }
        Ok(first)
    }

    fn value(&self) -> Result<&str, String> {
        if !self.children.is_empty() || self.text.len() > 512 { return Err(INVALID_GOOGLE.into()); }
        Ok(self.text.trim())
    }
}

fn node(start: &BytesStart<'_>) -> Result<Node, String> {
    let name = start.local_name().as_ref().to_string();
    let mut attributes = HashMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| INVALID_GOOGLE)?;
        let key = attribute.key.as_ref().to_string();
        let value = attribute.value.as_ref().to_string();
        if value.len() > 1024 || attributes.insert(key, value).is_some() { return Err(INVALID_GOOGLE.into()); }
    }
    Ok(Node { name, attributes, ..Node::default() })
}

fn google_packages(xml: &str) -> Result<Vec<Node>, String> {
    if xml.len() as u64 > MAX_METADATA { return Err(INVALID_GOOGLE.into()); }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut roots = 0;
    let mut nodes = 0;
    let mut packages = Vec::new();
    let mut capture = Vec::<Node>::new();
    loop {
        match reader.read_event().map_err(|_| INVALID_GOOGLE)? {
            Event::Start(start) => {
                nodes += 1;
                if nodes > 100000 { return Err(INVALID_GOOGLE.into()); }
                if depth == 0 { roots += 1; }
                depth += 1;
                if depth > 64 { return Err(INVALID_GOOGLE.into()); }
                let current = node(&start)?;
                if !capture.is_empty() || current.name == "remotePackage" && current.attributes.get("path").map(String::as_str) == Some("platform-tools") {
                    capture.push(current);
                }
            },
            Event::Empty(start) => {
                nodes += 1;
                if nodes > 100000 { return Err(INVALID_GOOGLE.into()); }
                if depth == 0 { roots += 1; }
                if !capture.is_empty() { capture.last_mut().unwrap().children.push(node(&start)?); }
            },
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(INVALID_GOOGLE)?;
                if let Some(current) = capture.pop() {
                    if let Some(parent) = capture.last_mut() { parent.children.push(current); }
                    else { packages.push(current); }
                }
            },
            Event::Text(text) if !capture.is_empty() => {
                let text = text.as_ref();
                let target = capture.last_mut().unwrap();
                if target.text.len() + text.len() > 1024 { return Err(INVALID_GOOGLE.into()); }
                target.text.push_str(text);
            },
            Event::CData(_) | Event::GeneralRef(_) if !capture.is_empty() => return Err(INVALID_GOOGLE.into()),
            Event::DocType(_) => return Err("Update metadata must not contain a document type or external entities.".into()),
            Event::Text(text) if depth == 0 && !text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => return Err(INVALID_GOOGLE.into()),
            Event::Eof => break,
            _ => {},
        }
    }
    if depth != 0 || roots != 1 || !capture.is_empty() || packages.len() > 256 { return Err(INVALID_GOOGLE.into()); }
    Ok(packages)
}

fn google_platform(os: &str, arch: &str) -> Option<(&'static str, &'static str)> {
    match (os, arch) {
        ("windows", "x86_64") => Some(("windows", "win")),
        ("linux", "x86_64") => Some(("linux", "linux")),
        ("macos", "x86_64" | "aarch64") => Some(("macosx", "darwin")),
        _ => None,
    }
}

fn digest(value: &str, kind: &str) -> bool {
    let length = match kind { "sha1" => 40, "sha256" => 64, _ => return false };
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn google_releases(xml: &str, os: &str, arch: &str) -> Result<Vec<Release>, String> {
    let (host, suffix) = google_platform(os, arch).ok_or("Google does not publish a verified Platform-Tools archive for this operating system and architecture.")?;
    let mut releases = Vec::new();
    let mut versions = HashSet::new();
    for package in google_packages(xml)? {
        if package.one("channelRef")?.attributes.get("ref").map(String::as_str) != Some("channel-0") { continue; }
        let revision = package.one("revision")?;
        if revision.optional("preview")?.is_some() { continue; }
        let part = |name| -> Result<u32, String> {
            let value = revision.optional(name)?.map(Node::value).transpose()?.unwrap_or("0");
            if value.is_empty() || value.len() > 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) { return Err(INVALID_GOOGLE.into()); }
            value.parse().map_err(|_| INVALID_GOOGLE.into())
        };
        let major = part("major")?;
        if major == 0 { return Err(INVALID_GOOGLE.into()); }
        let version = format!("{}.{}.{}", major, part("minor")?, part("micro")?);
        if !versions.insert(version.clone()) { return Err(INVALID_GOOGLE.into()); }
        let mut matching = Vec::new();
        for archive in &package.one("archives")?.children {
            if archive.name != "archive" { continue; }
            if archive.one("host-os")?.value()? != host { continue; }
            if let Some(declared) = archive.optional("host-arch")? {
                let declared = declared.value()?;
                let matches = declared == arch || arch == "x86_64" && ["x64", "amd64"].contains(&declared);
                if !matches { continue; }
            }
            if let Some(bits) = archive.optional("host-bits")? { if bits.value()? != "64" { continue; } }
            let complete = archive.one("complete")?;
            let filename = complete.one("url")?.value()?;
            if filename != format!("platform-tools_r{}-{}.zip", version, suffix) { return Err("The Platform-Tools archive URL did not match its revision and platform.".into()); }
            let checksum = complete.one("checksum")?;
            let checksum_type = checksum.attributes.get("type").ok_or(INVALID_GOOGLE)?;
            let checksum = checksum.value()?;
            if !digest(checksum, checksum_type) { return Err("Google's published package checksum was missing or invalid.".into()); }
            let size = complete.one("size")?.value()?.parse::<u64>().ok().filter(|size| *size > 0 && *size <= MAX_ARCHIVE).ok_or(INVALID_GOOGLE)?;
            matching.push(Release { version: version.clone(), url: format!("{}{}", GOOGLE_REPOSITORY, filename), release_url: GOOGLE_RELEASES.into(),
                checksum: checksum.to_ascii_lowercase(), checksum_type: checksum_type.clone(), size });
        }
        if matching.len() > 1 { return Err(INVALID_GOOGLE.into()); }
        releases.extend(matching);
    }
    sort_releases(&mut releases);
    if releases.is_empty() { return Err("No official stable Platform-Tools package is available for this platform.".into()); }
    Ok(releases)
}

pub fn adb_releases() -> Result<Vec<Release>, String> {
    google_platform(std::env::consts::OS, std::env::consts::ARCH).ok_or("Google does not publish a verified Platform-Tools archive for this operating system and architecture.")?;
    google_releases(&fetch(GOOGLE_METADATA, MAX_METADATA)?, std::env::consts::OS, std::env::consts::ARCH)
}

fn scrcpy_asset(os: &str, arch: &str, version: &str) -> Option<String> {
    let platform = match (os, arch) {
        ("windows", "x86") => "win32".to_string(),
        ("windows", "x86_64") => "win64".to_string(),
        ("windows", "aarch64") => "winarm64".to_string(),
        ("linux" | "macos", "x86_64" | "aarch64") => format!("{}-{}", os, arch),
        _ => return None,
    };
    Some(format!("scrcpy-{}-{}.{}", platform, version, if os == "windows" { "zip" } else { "tar.gz" }))
}

fn sort_releases(releases: &mut [Release]) {
    releases.sort_by(|first, second| compare_versions(&second.version, &first.version).unwrap_or(Ordering::Equal));
}

fn scrcpy_versions(data: &Value, os: &str, arch: &str) -> Result<Vec<Release>, String> {
    scrcpy_asset(os, arch, "v1.0").ok_or("No official scrcpy archive is supported for this operating system and architecture.")?;
    let data = data.as_array().filter(|data| data.len() <= 100).ok_or(INVALID_SCRCPY)?;
    let mut releases = Vec::new();
    let mut versions = HashSet::new();
    for release in data {
        if release["draft"].as_bool() != Some(false) || release["prerelease"].as_bool() != Some(false) { continue; }
        // Historical publisher tags such as v1.5-fixversion are not comparable
        // stable revisions. They must not hide current valid releases.
        let Some(version) = release["tag_name"].as_str().filter(|version| numeric_version(version, false).is_some()) else { continue; };
        let name = scrcpy_asset(os, arch, version).ok_or(INVALID_SCRCPY)?;
        let assets = release["assets"].as_array().filter(|assets| assets.len() <= 128).ok_or(INVALID_SCRCPY)?;
        let matches = assets.iter().filter(|asset| asset["name"].as_str() == Some(&name)).collect::<Vec<_>>();
        if matches.len() > 1 { return Err(INVALID_SCRCPY.into()); }
        let Some(asset) = matches.first() else { continue; };
        let url = format!("{}{}/{}", SCRCPY_DOWNLOAD, version, name);
        let page = format!("{}{}", SCRCPY_PAGE, version);
        if asset["browser_download_url"].as_str() != Some(&url) || release["html_url"].as_str() != Some(&page) { return Err("The scrcpy asset URL did not match its official release and platform.".into()); }
        let size = asset["size"].as_u64().filter(|size| *size > 0 && *size <= MAX_ARCHIVE).ok_or(INVALID_SCRCPY)?;
        let (checksum_type, checksum) = match asset.get("digest") {
            None | Some(Value::Null) => (String::new(), String::new()),
            Some(Value::String(value)) => {
                let checksum = value.strip_prefix("sha256:").filter(|checksum| digest(checksum, "sha256")).ok_or("The scrcpy archive's published digest was invalid or unsupported.")?;
                ("sha256".into(), checksum.to_ascii_lowercase())
            },
            _ => return Err(INVALID_SCRCPY.into()),
        };
        if !versions.insert(version.to_string()) { return Err(INVALID_SCRCPY.into()); }
        releases.push(Release { version: version.into(), url, release_url: page, checksum, checksum_type, size });
    }
    sort_releases(&mut releases);
    if releases.is_empty() { return Err("No official stable scrcpy archive is available for this platform.".into()); }
    Ok(releases)
}

pub fn scrcpy_releases() -> Result<Vec<Release>, String> {
    let data: Value = serde_json::from_str(&fetch(&format!("{}?per_page=100", SCRCPY_API), MAX_METADATA)?).map_err(|_| INVALID_SCRCPY)?;
    scrcpy_versions(&data, std::env::consts::OS, std::env::consts::ARCH)
}

fn sums_checksum(output: &str, filename: &str) -> Result<String, String> {
    let mut matching = Vec::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let Some(checksum) = parts.next() else { continue; };
        let Some(name) = parts.next() else { continue; };
        if name.trim_start_matches('*') != filename { continue; }
        if parts.next().is_some() || !digest(checksum, "sha256") { return Err("The release checksum file was malformed.".into()); }
        matching.push(checksum.to_ascii_lowercase());
    }
    if matching.len() != 1 { return Err("The release checksum file did not uniquely identify the selected archive.".into()); }
    Ok(matching.remove(0))
}

pub fn scrcpy_release(version: &str) -> Result<Release, String> {
    if numeric_version(version, false).is_none() { return Err("Select a stable numeric scrcpy release.".into()); }
    let data: Value = serde_json::from_str(&fetch(&format!("{}/tags/{}", SCRCPY_API, version), MAX_METADATA)?).map_err(|_| INVALID_SCRCPY)?;
    let mut releases = scrcpy_versions(&serde_json::json!([data.clone()]), std::env::consts::OS, std::env::consts::ARCH)?;
    let mut release = releases.remove(0);
    if release.version != version { return Err(INVALID_SCRCPY.into()); }
    if release.checksum.is_empty() {
        let expected = format!("{}{}/SHA256SUMS.txt", SCRCPY_DOWNLOAD, version);
        let sums = data["assets"].as_array().ok_or(INVALID_SCRCPY)?.iter().filter(|asset| asset["name"].as_str() == Some("SHA256SUMS.txt")).collect::<Vec<_>>();
        if sums.len() > 1 { return Err(INVALID_SCRCPY.into()); }
        if let Some(sums) = sums.first() {
            if sums["browser_download_url"].as_str() != Some(&expected) { return Err("The release checksum file URL was not official.".into()); }
            // Only the selected legacy release needs this extra request. The
            // complete version chooser must not download dozens of manifests.
            release.checksum = sums_checksum(&fetch(&expected, 128 * 1024)?, release.url.rsplit('/').next().ok_or(INVALID_SCRCPY)?)?;
            release.checksum_type = "sha256".into();
        }
    }
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn google(version: &str, channel: &str, extra: &str) -> String {
        format!(r#"<sdk-repository><remotePackage path="platform-tools"><revision><major>{}</major><minor>0</minor><micro>3</micro>{}</revision><channelRef ref="{}"/><archives><archive><complete><size>123</size><checksum type="sha1">0123456789abcdef0123456789abcdef01234567</checksum><url>platform-tools_r{}.0.3-win.zip</url></complete><host-os>windows</host-os></archive></archives></remotePackage></sdk-repository>"#, version, extra, channel, version)
    }

    fn scrcpy(version: &str) -> Value {
        serde_json::json!({ "tag_name": version, "draft": false, "prerelease": false, "html_url": format!("{}{}", SCRCPY_PAGE, version), "assets": [{
            "name": format!("scrcpy-win64-{}.zip", version), "browser_download_url": format!("{}{}/scrcpy-win64-{}.zip", SCRCPY_DOWNLOAD, version, version),
            "size": 123, "digest": format!("sha256:{}", "ab".repeat(32)),
        }] })
    }

    #[test]
    fn compares_platform_tools_revision_without_its_build_id() {
        assert_eq!(compare_versions("37.0.3-16887417", "37.0.3"), Some(Ordering::Equal));
        assert_eq!(compare_versions("v5.0.1", "5.0"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v5.0", "5.0.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("37.0.1-123", "37.0.3"), Some(Ordering::Less));
        for value in ["", "unknown", "5.0-rc1", "5..0", "5.0.0-", "5.0.0+build", "5.0.4294967299", "5.0.1.2.3", "5.0\n"] { assert_eq!(compare_versions(value, "5.0"), None, "accepted {}", value); }
    }

    #[test]
    fn selects_official_stable_google_revision_and_published_checksum() {
        let releases = google_releases(&google("37", "channel-0", ""), "windows", "x86_64").unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "37.0.3");
        assert_eq!(releases[0].checksum_type, "sha1");
        assert_eq!(releases[0].size, 123);
        assert!(releases[0].url.ends_with("platform-tools_r37.0.3-win.zip"));
        assert!(google_releases(&google("37", "channel-1", ""), "windows", "x86_64").is_err());
        assert!(google_releases(&google("37", "channel-0", "<preview>1</preview>"), "windows", "x86_64").is_err());
    }

    #[test]
    fn rejects_unsupported_google_platforms_without_cross_architecture_fallback() {
        for (os, arch) in [("windows", "aarch64"), ("windows", "x86"), ("linux", "aarch64"), ("unknown", "x86_64")] {
            assert!(google_releases(&google("37", "channel-0", ""), os, arch).is_err());
        }
        assert!(google_platform("macos", "aarch64").is_some());
        let mismatch = google("37", "channel-0", "").replace("</host-os>", "</host-os><host-arch>aarch64</host-arch>");
        assert!(google_releases(&mismatch, "windows", "x86_64").is_err());
    }

    #[test]
    fn rejects_malformed_duplicate_or_redirected_google_metadata() {
        let valid = google("37", "channel-0", "");
        for value in [
            valid.replace("</major>", "</major><major>37</major>"),
            valid.replace("</archive>", "</archive><archive><host-os>windows</host-os></archive>"),
            valid.replace("<size>123</size>", "<size>0</size>"),
            valid.replace("<size>123</size>", "<size>123</size><size>123</size>"),
            valid.replace("type=\"sha1\"", "type=\"md5\""),
            valid.replace("0123456789abcdef0123456789abcdef01234567", "abc"),
            valid.replace("platform-tools_r37.0.3-win.zip", "https://evil.invalid/file.zip"),
            valid.replace("platform-tools_r37.0.3-win.zip", "../platform-tools_r37.0.3-win.zip"),
            valid.replace("platform-tools_r37.0.3-win.zip", "platform-tools_r36.0.3-win.zip"),
            valid.replace("</sdk-repository>", ""),
            format!("<!DOCTYPE sdk-repository [<!ENTITY test SYSTEM 'file:///private'>]>{}", valid),
            format!("{}{}", valid, valid),
        ] { assert!(google_releases(&value, "windows", "x86_64").is_err(), "accepted invalid metadata"); }
    }

    #[test]
    fn stable_scrcpy_assets_match_the_exact_platform_and_are_sorted() {
        let mut preview = scrcpy("v6.0"); preview["prerelease"] = Value::Bool(true);
        let mut draft = scrcpy("v7.0"); draft["draft"] = Value::Bool(true);
        let releases = scrcpy_versions(&serde_json::json!([scrcpy("v4.1"), draft, scrcpy("v5.0.1"), preview, scrcpy("v1.5-fixversion")]), "windows", "x86_64").unwrap();
        assert_eq!(releases.iter().map(|release| release.version.as_str()).collect::<Vec<_>>(), ["v5.0.1", "v4.1"]);
        assert_eq!(releases[0].checksum_type, "sha256");
        assert_eq!(scrcpy_asset("windows", "aarch64", "v5.0.1").unwrap(), "scrcpy-winarm64-v5.0.1.zip");
        assert_eq!(scrcpy_asset("macos", "aarch64", "v5.0.1").unwrap(), "scrcpy-macos-aarch64-v5.0.1.tar.gz");
        assert!(scrcpy_asset("windows", "unknown", "v5.0.1").is_none());
        assert!(scrcpy_versions(&serde_json::json!([scrcpy("v5.0.1")]), "windows", "aarch64").is_err());
    }

    #[test]
    fn rejects_scrcpy_asset_digest_url_size_and_version_ambiguity() {
        let valid = scrcpy("v5.0.1");
        let mut invalid = Vec::new();
        for (key, value) in [("browser_download_url", serde_json::json!("https://evil.invalid/scrcpy.zip")), ("size", serde_json::json!(0)), ("digest", serde_json::json!("sha256:abcd")), ("digest", serde_json::json!("sha1:abcd"))] {
            let mut release = valid.clone(); release["assets"][0][key] = value; invalid.push(serde_json::json!([release]));
        }
        let mut duplicate = valid.clone(); duplicate["assets"].as_array_mut().unwrap().push(valid["assets"][0].clone()); invalid.push(serde_json::json!([duplicate]));
        invalid.push(serde_json::json!([valid.clone(), valid]));
        invalid.push(serde_json::json!([scrcpy("v5.0-rc1")]));
        for data in invalid { assert!(scrcpy_versions(&data, "windows", "x86_64").is_err()); }
    }

    #[test]
    fn legacy_checksum_files_require_one_exact_filename() {
        let hash = "ab".repeat(32);
        let filename = "scrcpy-win64-v3.2.zip";
        assert_eq!(sums_checksum(&format!("{}  {}\n", hash, filename), filename).unwrap(), hash);
        assert_eq!(sums_checksum(&format!("{} *{}\n", hash, filename), filename).unwrap(), hash);
        for text in [format!("{} wrong.zip", hash), format!("{} {filename}\n{} {filename}", hash, hash), format!("abc {filename}"), format!("{} {filename} extra", hash)] { assert!(sums_checksum(&text, filename).is_err()); }
    }

    #[test]
    #[ignore = "Reads public official update metadata only; does not download executables, install tools, start ADB, or contact a phone"]
    fn live_official_metadata_without_installing() {
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_UPDATE_METADATA").as_deref(), Ok("1"));
        let adb = adb_releases().unwrap();
        let scrcpy = scrcpy_releases().unwrap();
        println!("Official metadata: ADB {} ({}), scrcpy {} ({}). No executable downloaded or installed.", adb[0].version, adb[0].checksum_type, scrcpy[0].version, scrcpy[0].checksum_type);
        assert!(!adb[0].checksum.is_empty() && !scrcpy[0].checksum.is_empty());
    }

    #[test]
    #[ignore = "Reads an official historical release and its checksum text only; no executable is downloaded or installed"]
    fn live_selected_legacy_checksum_without_installing() {
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_UPDATE_METADATA").as_deref(), Ok("1"));
        let release = scrcpy_release("v3.2").unwrap();
        assert_eq!(release.version, "v3.2");
        assert_eq!(release.checksum_type, "sha256");
        assert!(digest(&release.checksum, "sha256"));
        println!("Selected historical scrcpy metadata resolved its published SHA256SUMS checksum without downloading an executable.");
    }
}
