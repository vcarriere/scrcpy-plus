use serde::{ Deserialize, Serialize };
use serde_json::{ json, Value };
use sha1::Sha1;
use sha2::{ Digest, Sha256 };
use std::collections::HashSet;
use std::fs::{ self, OpenOptions };
use std::io::{ Cursor, Read, Write };
use std::path::{ Path, PathBuf };
use std::process::{ Command, Stdio };
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::{ Mutex, atomic::{ AtomicU64, Ordering } };
use std::time::{ Duration, Instant };
use crate::tool_catalog::Release;

const GOOGLE: &str = "https://dl.google.com/android/repository/";
const SCRCPY: &str = "https://github.com/Genymobile/scrcpy/releases/download/";
const RECEIPT: &str = ".scrcpy-plus-adb.json";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_RECEIPT: u64 = 2 * 1024 * 1024;
const INVALID: &str = "The managed ADB installation is unavailable or has changed. Its previous files were kept.";
static INSTALLING: Mutex<()> = Mutex::new(());
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
pub struct Installed { pub version: String, pub adb_path: String, pub compressed: bool }

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileReceipt { path: String, size: u64, sha256: String }

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
  schema: u32,
  version: String,
  host_os: String,
  host_arch: String,
  url: String,
  checksum: String,
  checksum_type: String,
  archive_size: u64,
  files: Vec<FileReceipt>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CachedReceipt {
  schema: u32,
  archive_size: u64,
  archive_sha256: String,
  receipt: Receipt,
}

struct TemporaryDirectory { path: PathBuf }

impl TemporaryDirectory {
  fn new(root: &Path) -> Result<Self, String> {
    for _ in 0..32 {
      let path = root.join(format!(".install-{}-{}", std::process::id(), NEXT_STAGE.fetch_add(1, Ordering::Relaxed)));
      match fs::create_dir(&path) {
        Ok(()) => return Ok(Self { path }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error.to_string()),
      }
    }
    Err("Could not create an ADB staging directory.".into())
  }
}

impl Drop for TemporaryDirectory {
  fn drop(&mut self) { let _ = fs::remove_dir_all(&self.path); }
}

#[derive(Clone, Copy)]
struct Limits { entries: usize, file: u64, total: u64 }
const LIMITS: Limits = Limits { entries: 4096, file: 256 * 1024 * 1024, total: 1024 * 1024 * 1024 };

fn revision(version: &str) -> bool {
  let parts: Vec<_> = version.split('.').collect();
  version.len() <= 32 && parts.len() == 3 && parts.iter().all(|part|
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) && part.parse::<u32>().is_ok()
      && (part.len() == 1 || !part.starts_with('0')))
}

fn hexadecimal(value: &str, length: usize) -> bool { value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()) }

fn hex(bytes: &[u8]) -> String {
  const DIGITS: &[u8; 16] = b"0123456789abcdef";
  let mut value = String::with_capacity(bytes.len() * 2);
  for byte in bytes { value.push(DIGITS[usize::from(byte >> 4)] as char); value.push(DIGITS[usize::from(byte & 15)] as char); }
  value
}

fn platform() -> Result<&'static str, String> {
  match (std::env::consts::OS, std::env::consts::ARCH) {
    ("windows", "x86_64") => Ok("win"),
    ("linux", "x86_64") => Ok("linux"),
    ("macos", "x86_64" | "aarch64") => Ok("darwin"),
    _ => Err("Google does not provide a supported Platform-Tools build for this operating system and architecture.".into()),
  }
}

fn validate_digest(release: &Release) -> Result<(), String> {
  if release.size == 0 || release.size > MAX_DOWNLOAD { return Err("The release archive size is unavailable or exceeds the download limit.".into()); }
  match release.checksum_type.as_str() {
    "sha1" if hexadecimal(&release.checksum, 40) => Ok(()),
    "sha256" if hexadecimal(&release.checksum, 64) => Ok(()),
    _ => Err("Publisher integrity metadata is unavailable or invalid. The archive was not installed.".into()),
  }
}

fn validate_adb(release: &Release) -> Result<(), String> {
  if !revision(&release.version) { return Err("Invalid Platform-Tools revision.".into()); }
  let expected = format!("{}platform-tools_r{}-{}.zip", GOOGLE, release.version, platform()?);
  if release.url != expected { return Err("Only the official versioned Platform-Tools archive for this platform can be installed.".into()); }
  validate_digest(release)
}

fn validate_source(release: &Release) -> Result<(), String> {
  if release.url.starts_with(GOOGLE) { return validate_adb(release); }
  let (tag, file) = release.url.strip_prefix(SCRCPY).and_then(|path| path.split_once('/')).ok_or("Untrusted release download URL.")?;
  if tag != release.version || tag.is_empty() || tag.len() > 128
    || !tag.bytes().all(|byte| byte.is_ascii_alphanumeric() || [b'.', b'-'].contains(&byte))
    || file.is_empty() || !file.bytes().all(|byte| byte.is_ascii_alphanumeric() || [b'.', b'-', b'_'].contains(&byte))
    || !(file.ends_with(".zip") || file.ends_with(".tar.gz")) || release.checksum_type != "sha256" {
    return Err("The archive must match an official scrcpy release and its published SHA-256 digest.".into());
  }
  validate_digest(release)
}

fn read_archive(reader: impl Read, release: &Release) -> Result<Vec<u8>, String> {
  validate_digest(release)?;
  let mut bytes = Vec::new();
  reader.take(release.size + 1).read_to_end(&mut bytes).map_err(|_| "The release download was interrupted.")?;
  if bytes.len() as u64 != release.size { return Err("The downloaded archive does not match the publisher's size.".into()); }
  let actual = match release.checksum_type.as_str() {
    // Google's authenticated repository currently publishes SHA-1. It is used
    // for legacy integrity comparison, not as a standalone authenticity claim.
    "sha1" => hex(&Sha1::digest(&bytes)),
    "sha256" => hex(&Sha256::digest(&bytes)),
    _ => return Err("Unsupported archive digest.".into()),
  };
  if !actual.eq_ignore_ascii_case(&release.checksum) { return Err("The downloaded archive does not match the publisher's checksum.".into()); }
  Ok(bytes)
}

pub fn download_bytes(release: &Release) -> Result<Vec<u8>, String> {
  validate_source(release)?;
  let response = ureq::get(&release.url).config().https_only(true).max_redirects(5)
    .timeout_global(Some(Duration::from_secs(60))).build().call().map_err(|_| "Could not download the official release archive.")?;
  read_archive(response.into_body().into_reader(), release)
}

fn is_link(metadata: &fs::Metadata) -> bool {
  #[cfg(windows)]
  { use std::os::windows::fs::MetadataExt; if metadata.file_attributes() & 0x400 != 0 { return true; } }
  metadata.file_type().is_symlink()
}

fn normal_directory(path: &Path) -> Result<(), String> {
  for directory in path.ancestors() {
    match fs::symlink_metadata(directory) {
      Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => return Err("ADB installation directories must not be links or ordinary files.".into()),
      Ok(_) => {},
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
      Err(error) => return Err(error.to_string()),
    }
  }
  Ok(())
}

fn safe_path(name: &str) -> Result<PathBuf, String> {
  if name.is_empty() || name.len() > 1024 || !name.is_ascii() || name.contains('\\') { return Err("The archive contains an unsupported path.".into()); }
  let name = name.trim_end_matches('/');
  let mut path = PathBuf::new();
  for part in name.split('/') {
    let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if part.is_empty() || [".", ".."].contains(&part) || part.len() > 255 || part.ends_with(['.', ' '])
      || part.bytes().any(|byte| byte.is_ascii_control() || [b':', b'?', b'*', b'"', b'<', b'>', b'|'].contains(&byte))
      || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
      || stem.len() == 4 && ["COM", "LPT"].contains(&&stem[..3]) && matches!(stem.as_bytes()[3], b'1'..=b'9') {
      return Err("The archive contains an unsafe path.".into());
    }
    path.push(part);
  }
  if path.components().count() > 32 { return Err("The archive path is too deeply nested.".into()); }
  Ok(path)
}

fn relative_path(name: &str) -> Result<PathBuf, String> {
  let path = safe_path(name)?;
  if path.iter().next().is_none_or(|part| part != "platform-tools") { return Err("The archive does not contain a Platform-Tools package.".into()); }
  Ok(path)
}

fn hash_file(path: &Path, limit: u64) -> Result<(u64, String), String> {
  let metadata = fs::symlink_metadata(path).map_err(|_| INVALID)?;
  if !metadata.is_file() || is_link(&metadata) || metadata.len() == 0 || metadata.len() > limit { return Err(INVALID.into()); }
  let mut file = fs::File::open(path).map_err(|_| INVALID)?;
  let mut digest = Sha256::new();
  let mut count = 0;
  let mut chunk = [0; 16384];
  loop {
    let read = file.read(&mut chunk).map_err(|_| INVALID)?;
    if read == 0 { break; }
    count += read as u64;
    if count > limit { return Err(INVALID.into()); }
    digest.update(&chunk[..read]);
  }
  if count != metadata.len() { return Err(INVALID.into()); }
  Ok((count, hex(&digest.finalize())))
}

fn extract(bytes: Vec<u8>, stage: &Path, limits: Limits) -> Result<Vec<FileReceipt>, String> {
  extract_archive(&bytes, stage, limits, true)
}

pub(crate) fn extract_zip(bytes: &[u8], stage: &Path) -> Result<(), String> {
  extract_archive(bytes, stage, LIMITS, false).map(|_| ())
}

fn extract_archive(bytes: &[u8], stage: &Path, limits: Limits, platform_tools: bool) -> Result<Vec<FileReceipt>, String> {
  let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "The release archive is unreadable.")?;
  if zip.len() == 0 || zip.len() > limits.entries { return Err("The archive contains too many files.".into()); }
  let mut seen = HashSet::new();
  let mut total = 0u64;
  let mut files = Vec::new();
  for index in 0..zip.len() {
    let mut entry = zip.by_index(index).map_err(|_| "An archive entry is unreadable.")?;
    let name = entry.name().map_err(|_| "The archive entry name is unreadable.")?.trim_end_matches('/').to_string();
    let relative = if platform_tools { relative_path(&name)? } else { safe_path(&name)? };
    if !seen.insert(name.to_ascii_lowercase()) { return Err("The archive contains duplicate paths.".into()); }
    if entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) { return Err("Archive links are not supported.".into()); }
    let path = stage.join(&relative);
    if entry.is_dir() { fs::create_dir_all(&path).map_err(|e| e.to_string())?; continue; }
    if entry.size() > limits.file { return Err("An archive entry exceeds the extraction limit.".into()); }
    total = total.checked_add(entry.size()).ok_or("Archive size overflow.")?;
    if total > limits.total { return Err("The archive exceeds the extraction limit.".into()); }
    fs::create_dir_all(path.parent().ok_or("Invalid archive entry.")?).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
    let size = std::io::copy(&mut entry.by_ref().take(limits.file + 1), &mut output).map_err(|_| "The archive could not be extracted completely.")?;
    if size != entry.size() || size > limits.file { return Err("An archive entry did not match its declared size.".into()); }
    output.flush().map_err(|e| e.to_string())?;
    drop(output);
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&path, fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o644) & 0o777)).map_err(|e| e.to_string())?; }
    // Empty optional files are retained, but cannot stand in for required tools.
    let sha256 = if size == 0 { hex(&Sha256::digest([])) } else { hash_file(&path, limits.file)?.1 };
    files.push(FileReceipt { path: name, size, sha256 });
  }
  if files.is_empty() { return Err("The archive contains no runtime files.".into()); }
  files.sort_by(|first, second| first.path.cmp(&second.path));
  Ok(files)
}

fn bounded_text(path: &Path, limit: u64) -> Result<String, String> {
  let metadata = fs::symlink_metadata(path).map_err(|_| INVALID)?;
  if !metadata.is_file() || is_link(&metadata) || metadata.len() == 0 || metadata.len() > limit { return Err(INVALID.into()); }
  let mut value = String::new();
  fs::File::open(path).map_err(|_| INVALID)?.take(limit + 1).read_to_string(&mut value).map_err(|_| INVALID)?;
  if value.len() as u64 > limit { return Err(INVALID.into()); }
  Ok(value)
}

fn adb_path(root: &Path) -> PathBuf { root.join("platform-tools").join(if cfg!(windows) { "adb.exe" } else { "adb" }) }

fn runtime_header(path: &Path) -> Result<(), String> {
  let mut bytes = vec![0; 4096];
  let size = fs::File::open(path).map_err(|_| INVALID)?.read(&mut bytes).map_err(|_| INVALID)?;
  bytes.truncate(size);
  let accepted = match std::env::consts::OS {
    "windows" => {
      bytes.starts_with(b"MZ") && bytes.get(60..64).and_then(|offset| Some(u32::from_le_bytes(offset.try_into().ok()?) as usize))
        .is_some_and(|offset| bytes.get(offset..offset + 4) == Some(b"PE\0\0") && bytes.get(offset + 4..offset + 6) == Some(&0x8664u16.to_le_bytes()))
    },
    "linux" => bytes.starts_with(b"\x7fELF\x02\x01") && bytes.get(18..20) == Some(&0x3eu16.to_le_bytes()),
    "macos" => {
      let cpu = if std::env::consts::ARCH == "aarch64" { 0x0100000cu32 } else { 0x01000007u32 };
      if bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) { bytes.get(4..8) == Some(&cpu.to_le_bytes()) }
      else if bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
        bytes.get(4..8).and_then(|count| Some(u32::from_be_bytes(count.try_into().ok()?) as usize)).is_some_and(|count|
          count > 0 && count <= 8 && (0..count).any(|index| bytes.get(8 + index * 20..12 + index * 20) == Some(&cpu.to_be_bytes())))
      } else { false }
    },
    _ => false,
  };
  if !accepted { return Err("The downloaded ADB executable does not match this platform and architecture.".into()); }
  Ok(())
}

fn validate_runtime(root: &Path, version: &str) -> Result<(), String> {
  let path = adb_path(root);
  hash_file(&path, LIMITS.file)?;
  runtime_header(&path)?;
  let package = root.join("platform-tools");
  if cfg!(windows) { for name in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] { hash_file(&package.join(name), LIMITS.file)?; } }
  hash_file(&package.join("NOTICE.txt"), LIMITS.file)?;
  let properties = bounded_text(&package.join("source.properties"), 16384)?;
  let revisions = properties.lines().filter_map(|line| line.split_once('='))
    .filter(|(key, _)| key.trim() == "Pkg.Revision").map(|(_, value)| value.trim()).collect::<Vec<_>>();
  if revisions != [version] { return Err("The Platform-Tools revision does not match the selected release.".into()); }
  Ok(())
}

fn matches_version(output: &str, expected: &str) -> bool {
  let protocols = output.lines().filter_map(|line| line.strip_prefix("Android Debug Bridge version ")).collect::<Vec<_>>();
  let versions = output.lines().filter_map(|line| line.strip_prefix("Version ")).collect::<Vec<_>>();
  protocols.len() == 1 && revision(protocols[0])
    && versions.len() == 1 && (versions[0] == expected || versions[0].strip_prefix(expected).is_some_and(|suffix| suffix.starts_with('-') && suffix.len() > 1 && !suffix.chars().any(char::is_whitespace)))
}

fn version_output(path: &Path) -> Result<String, String> {
  let mut command = Command::new(path);
  command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
  #[cfg(windows)]
  command.creation_flags(0x08000000);
  let mut child = command.spawn().map_err(|_| "The downloaded ADB executable could not be validated.")?;
  let stdout = child.stdout.take().ok_or("Could not read the downloaded ADB version.")?;
  let (sender, receiver) = std::sync::mpsc::channel();
  std::thread::spawn(move || {
    let mut output = Vec::new();
    let result = stdout.take(8193).read_to_end(&mut output).map(|_| output);
    let _ = sender.send(result);
  });
  let deadline = Instant::now() + Duration::from_secs(3);
  let status = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status,
      Err(_) => { let _ = child.kill(); let _ = child.wait(); return Err("Could not inspect ADB version validation.".into()); },
      _ => {},
    }
    if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); return Err("ADB version validation timed out. No server was started.".into()); }
    std::thread::sleep(Duration::from_millis(20));
  };
  let output = receiver.recv_timeout(Duration::from_millis(300)).map_err(|_| "Could not finish reading the ADB version.")?.map_err(|_| "Could not read the ADB version.")?;
  if !status.success() || output.len() > 8192 { return Err("The downloaded ADB executable did not report a valid version.".into()); }
  String::from_utf8(output).map_err(|_| "The downloaded ADB version was unreadable.".into())
}

fn receipt(root: &Path) -> Result<Receipt, String> {
  let version = root.file_name().and_then(|name| name.to_str()).ok_or(INVALID)?;
  receipt_at(root, version)
}

fn receipt_at(root: &Path, version: &str) -> Result<Receipt, String> {
  normal_directory(root)?;
  let record: Receipt = serde_json::from_str(&bounded_text(&root.join(RECEIPT), MAX_RECEIPT)?).map_err(|_| INVALID)?;
  if record.schema != 1 || !revision(&record.version) || record.host_os != std::env::consts::OS || record.host_arch != std::env::consts::ARCH
    || version != record.version || record.files.is_empty() || record.files.len() > LIMITS.entries {
    return Err(INVALID.into());
  }
  let release = Release { version: record.version.clone(), url: record.url.clone(), release_url: String::new(), checksum: record.checksum.clone(), checksum_type: record.checksum_type.clone(), size: record.archive_size };
  validate_adb(&release)?;
  let mut expected = HashSet::new();
  let mut total = 0u64;
  for file in &record.files {
    let relative = relative_path(&file.path)?;
    if !expected.insert(file.path.to_ascii_lowercase()) || !hexadecimal(&file.sha256, 64) || file.size > LIMITS.file { return Err(INVALID.into()); }
    total = total.checked_add(file.size).ok_or(INVALID)?;
    if total > LIMITS.total { return Err(INVALID.into()); }
    for parent in root.join(&relative).parent().into_iter().flat_map(Path::ancestors).take_while(|parent| parent.starts_with(root)) { normal_directory(parent)?; }
    let path = root.join(relative);
    let (size, digest) = if file.size == 0 {
      let metadata = fs::symlink_metadata(&path).map_err(|_| INVALID)?;
      if is_link(&metadata) || !metadata.is_file() || metadata.len() != 0 { return Err(INVALID.into()); }
      (0, hex(&Sha256::digest([])))
    } else { hash_file(&path, LIMITS.file)? };
    if size != file.size || digest != file.sha256 { return Err(INVALID.into()); }
  }
  let mut stack = vec![root.to_path_buf()];
  let mut count = 0;
  while let Some(directory) = stack.pop() {
    for entry in fs::read_dir(&directory).map_err(|_| INVALID)? {
      let entry = entry.map_err(|_| INVALID)?;
      let path = entry.path();
      let metadata = fs::symlink_metadata(&path).map_err(|_| INVALID)?;
      if is_link(&metadata) { return Err(INVALID.into()); }
      if path == root.join(RECEIPT) { continue; }
      count += 1;
      if count > LIMITS.entries * 2 { return Err(INVALID.into()); }
      if metadata.is_dir() { stack.push(path); }
      else if metadata.is_file() {
        let relative = path.strip_prefix(root).map_err(|_| INVALID)?.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        if !expected.remove(&relative) { return Err(INVALID.into()); }
      } else { return Err(INVALID.into()); }
    }
  }
  if !expected.is_empty() { return Err(INVALID.into()); }
  validate_runtime(root, &record.version)?;
  Ok(record)
}

fn existing(base: &Path, release: &Release, read_version: &mut impl FnMut(&Path) -> Result<String, String>) -> Result<Option<Value>, String> {
  let root = base.join("adb").join(&release.version);
  match fs::symlink_metadata(&root) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.to_string()),
    Ok(_) => {},
  }
  let record = receipt(&root)?;
  if record.url != release.url || !record.checksum.eq_ignore_ascii_case(&release.checksum) || record.checksum_type != release.checksum_type || record.archive_size != release.size {
    return Err("The existing version has different release metadata. Its files were kept.".into());
  }
  let path = adb_path(&root);
  if !matches_version(&read_version(&path)?, &release.version) { return Err(INVALID.into()); }
  Ok(Some(json!({ "adb_path": path.to_string_lossy(), "version": release.version, "reused": true })))
}

fn install(base: &Path, release: &Release, bytes: Vec<u8>, read_version: &mut impl FnMut(&Path) -> Result<String, String>, publish: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<Value, String> {
  validate_adb(release)?;
  if let Some(result) = existing(base, release, read_version)? { return Ok(result); }
  let bytes = read_archive(Cursor::new(bytes), release)?;
  normal_directory(base)?;
  let root = base.join("adb");
  normal_directory(&root)?;
  fs::create_dir_all(&root).map_err(|e| e.to_string())?;
  let stage = TemporaryDirectory::new(&root)?;
  let files = extract(bytes, &stage.path, LIMITS)?;
  validate_runtime(&stage.path, &release.version)?;
  #[cfg(unix)]
  { use std::os::unix::fs::PermissionsExt; fs::set_permissions(adb_path(&stage.path), fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?; }
  if !matches_version(&read_version(&adb_path(&stage.path))?, &release.version) { return Err("The downloaded ADB build does not match the selected Platform-Tools revision.".into()); }
  let record = Receipt { schema: 1, version: release.version.clone(), host_os: std::env::consts::OS.into(), host_arch: std::env::consts::ARCH.into(),
    url: release.url.clone(), checksum: release.checksum.clone(), checksum_type: release.checksum_type.clone(), archive_size: release.size, files };
  let bytes = serde_json::to_vec(&record).map_err(|_| "Could not write the managed ADB receipt.")?;
  if bytes.len() as u64 > MAX_RECEIPT { return Err("The managed ADB receipt is too large.".into()); }
  let mut output = OpenOptions::new().write(true).create_new(true).open(stage.path.join(RECEIPT)).map_err(|e| e.to_string())?;
  output.write_all(&bytes).and_then(|_| output.sync_all()).map_err(|_| "Could not persist the managed ADB receipt.")?;
  drop(output);
  let destination = root.join(&release.version);
  if fs::symlink_metadata(&destination).is_ok() { return Err("This ADB version appeared during installation. Its files were kept; try selecting it again.".into()); }
  publish(&stage.path, &destination).map_err(|e| format!("Could not activate the staged ADB version. Previous versions were kept: {}", e))?;
  Ok(json!({ "adb_path": adb_path(&destination).to_string_lossy(), "version": release.version, "reused": false }))
}

pub fn download(base: &Path, release: &Release) -> Result<Value, String> {
  validate_adb(release)?;
  let _installing = INSTALLING.try_lock().map_err(|_| "An ADB installation is already in progress.")?;
  if let Some(result) = existing(base, release, &mut version_output)? { return Ok(result); }
  if let Ok(cached) = cached(base, &release.version) {
    if cached.receipt.url == release.url && cached.receipt.checksum_type == release.checksum_type
      && cached.receipt.checksum.eq_ignore_ascii_case(&release.checksum) && cached.receipt.archive_size == release.size {
      return restore_cached(base, &release.version, Some(&mut version_output));
    }
  }
  let bytes = download_bytes(release)?;
  install(base, release, bytes, &mut version_output, |from, to| fs::rename(from, to))
}

pub fn installed(base: &Path) -> Result<Vec<Installed>, String> {
  let root = base.join("adb");
  normal_directory(&root)?;
  let entries = match fs::read_dir(&root) {
    Ok(entries) => entries,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
    Err(error) => return Err(error.to_string()),
  };
  let mut versions = Vec::new();
  for entry in entries.take(256).flatten() {
    if entry.file_name().to_str().is_none_or(|name| !revision(name)) { continue; }
    if let Ok(record) = receipt(&entry.path()) { versions.push(Installed { version: record.version, adb_path: adb_path(&entry.path()).to_string_lossy().to_string(), compressed: false }); }
  }
  let retained = root.join(".retained");
  if normal_directory(&retained).is_ok() {
    if let Ok(entries) = fs::read_dir(&retained) {
      for entry in entries.take(256).flatten() {
        let Some(version) = entry.file_name().to_str().filter(|name| revision(name)).map(str::to_string) else { continue; };
        if versions.iter().any(|installed| installed.version == version) { continue; }
        if cached(base, &version).is_ok() { versions.push(Installed { version, adb_path: String::new(), compressed: true }); }
      }
    }
  }
  versions.sort_by(|first, second| {
    let parse = |value: &str| value.split('.').filter_map(|part| part.parse::<u32>().ok()).collect::<Vec<_>>();
    parse(&second.version).cmp(&parse(&first.version))
  });
  Ok(versions)
}

pub fn verify_selected(base: &Path, path: &str) -> Result<String, String> {
  verify_selected_with(base, path, &mut version_output)
}

fn verify_selected_with(base: &Path, path: &str, read_version: &mut impl FnMut(&Path) -> Result<String, String>) -> Result<String, String> {
  let path = Path::new(path);
  let expected_root = base.join("adb");
  normal_directory(&expected_root)?;
  normal_directory(path.parent().ok_or(INVALID)?)?;
  let metadata = fs::symlink_metadata(path).map_err(|_| INVALID)?;
  if is_link(&metadata) || !metadata.is_file() { return Err(INVALID.into()); }
  let expected_root = expected_root.canonicalize().map_err(|_| INVALID)?;
  let path = path.canonicalize().map_err(|_| INVALID)?;
  let root = path.parent().and_then(Path::parent).ok_or(INVALID)?;
  let same_path = |first: &Path, second: &Path| {
    #[cfg(windows)]
    { first.to_string_lossy().eq_ignore_ascii_case(&second.to_string_lossy()) }
    #[cfg(not(windows))]
    { first == second }
  };
  if !root.parent().is_some_and(|parent| same_path(parent, &expected_root)) || !same_path(&adb_path(root), &path) { return Err(INVALID.into()); }
  let record = receipt(root)?;
  if !matches_version(&read_version(&path)?, &record.version) { return Err(INVALID.into()); }
  Ok(record.version)
}

fn cache_root(base: &Path, version: &str) -> Result<PathBuf, String> {
  if !revision(version) { return Err("Invalid retained ADB revision.".into()); }
  Ok(base.join("adb").join(".retained").join(version))
}

fn cached(base: &Path, version: &str) -> Result<CachedReceipt, String> {
  normal_directory(&base.join("adb"))?;
  normal_directory(&base.join("adb/.retained"))?;
  let root = cache_root(base, version)?;
  normal_directory(&root)?;
  let cached: CachedReceipt = serde_json::from_str(&bounded_text(&root.join("manifest.json"), MAX_RECEIPT + 16384)?).map_err(|_| INVALID)?;
  let record = &cached.receipt;
  if cached.schema != 1 || record.schema != 1 || record.version != version || record.host_os != std::env::consts::OS
    || record.host_arch != std::env::consts::ARCH || record.files.is_empty() || record.files.len() >= LIMITS.entries
    || !hexadecimal(&cached.archive_sha256, 64) { return Err(INVALID.into()); }
  let release = Release { version: record.version.clone(), url: record.url.clone(), release_url: String::new(), checksum: record.checksum.clone(), checksum_type: record.checksum_type.clone(), size: record.archive_size };
  validate_adb(&release)?;
  let archive_path = root.join("archive.zip");
  let (size, digest) = hash_file(&archive_path, MAX_DOWNLOAD)?;
  if size != cached.archive_size || digest != cached.archive_sha256 { return Err("The retained ADB archive is damaged. Its files were kept.".into()); }
  let mut zip = zip::ZipArchive::new(fs::File::open(&archive_path).map_err(|_| INVALID)?).map_err(|_| INVALID)?;
  if zip.len() != record.files.len() + 1 { return Err(INVALID.into()); }
  let mut names = HashSet::new();
  let mut total = 0u64;
  let mut embedded = false;
  for index in 0..zip.len() {
    let mut entry = zip.by_index(index).map_err(|_| INVALID)?;
    let name = entry.name().map_err(|_| INVALID)?.to_string();
    if entry.is_dir() || !names.insert(name.to_ascii_lowercase()) || entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) { return Err(INVALID.into()); }
    if name == RECEIPT {
      if entry.size() > MAX_RECEIPT { return Err(INVALID.into()); }
      let mut text = String::new();
      entry.by_ref().take(MAX_RECEIPT + 1).read_to_string(&mut text).map_err(|_| INVALID)?;
      if text.len() as u64 > MAX_RECEIPT { return Err(INVALID.into()); }
      let decoded: Receipt = serde_json::from_str(&text).map_err(|_| INVALID)?;
      if serde_json::to_vec(&decoded).map_err(|_| INVALID)? != serde_json::to_vec(record).map_err(|_| INVALID)? { return Err(INVALID.into()); }
      embedded = true;
    } else {
      relative_path(&name)?;
      let item = record.files.iter().find(|file| file.path == name).ok_or(INVALID)?;
      if entry.size() != item.size || item.size > LIMITS.file || !hexadecimal(&item.sha256, 64) { return Err(INVALID.into()); }
      total = total.checked_add(item.size).ok_or(INVALID)?;
      if total > LIMITS.total { return Err(INVALID.into()); }
    }
  }
  if !embedded { return Err(INVALID.into()); }
  Ok(cached)
}

fn write_cache(source: &Path, record: &Receipt, stage: &Path) -> Result<CachedReceipt, String> {
  let path = stage.join("archive.zip");
  let file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
  let mut archive = zip::ZipWriter::new(file);
  for item in &record.files {
    let source = source.join(relative_path(&item.path)?);
    let mut options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; options = options.unix_permissions(fs::metadata(&source).map_err(|_| INVALID)?.permissions().mode() & 0o777); }
    #[cfg(not(unix))]
    let _ = &mut options;
    archive.start_file(&item.path, options).map_err(|_| "Could not create the retained ADB archive.")?;
    let count = std::io::copy(&mut fs::File::open(&source).map_err(|_| INVALID)?, &mut archive).map_err(|_| "Could not compress the inactive ADB files.")?;
    if count != item.size { return Err(INVALID.into()); }
  }
  archive.start_file(RECEIPT, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(|_| INVALID)?;
  archive.write_all(&serde_json::to_vec(record).map_err(|_| INVALID)?).map_err(|_| INVALID)?;
  archive.finish().map_err(|_| INVALID)?.sync_all().map_err(|_| "Could not persist the retained ADB archive.")?;
  let (archive_size, archive_sha256) = hash_file(&path, MAX_DOWNLOAD)?;
  let cached = CachedReceipt { schema: 1, archive_size, archive_sha256, receipt: record.clone() };
  let bytes = serde_json::to_vec(&cached).map_err(|_| INVALID)?;
  if bytes.len() as u64 > MAX_RECEIPT + 16384 { return Err(INVALID.into()); }
  let mut manifest = OpenOptions::new().write(true).create_new(true).open(stage.join("manifest.json")).map_err(|e| e.to_string())?;
  manifest.write_all(&bytes).and_then(|_| manifest.sync_all()).map_err(|_| "Could not persist the retained ADB manifest.")?;
  Ok(cached)
}

fn restore_cached(base: &Path, version: &str, mut read_version: Option<&mut dyn FnMut(&Path) -> Result<String, String>>) -> Result<Value, String> {
  let retained = cached(base, version)?;
  let root = base.join("adb");
  let destination = root.join(version);
  if fs::symlink_metadata(&destination).is_ok() {
    let record = receipt(&destination)?;
    if record.version != retained.receipt.version { return Err(INVALID.into()); }
    if let Some(reader) = &mut read_version { if !matches_version(&reader(&adb_path(&destination))?, version) { return Err(INVALID.into()); } }
    return Ok(json!({ "adb_path": adb_path(&destination).to_string_lossy(), "version": version, "reused": true, "restored": false }));
  }
  normal_directory(&root)?;
  let stage = TemporaryDirectory::new(&root)?;
  let archive = cache_root(base, version)?.join("archive.zip");
  extract_zip(&fs::read(archive).map_err(|_| INVALID)?, &stage.path)?;
  let record = receipt_at(&stage.path, version)?;
  if serde_json::to_vec(&record).map_err(|_| INVALID)? != serde_json::to_vec(&retained.receipt).map_err(|_| INVALID)? { return Err(INVALID.into()); }
  if let Some(reader) = &mut read_version { if !matches_version(&reader(&adb_path(&stage.path))?, version) { return Err(INVALID.into()); } }
  if fs::symlink_metadata(&destination).is_ok() { return Err("This retained version was restored by another operation. Its files were kept.".into()); }
  fs::rename(&stage.path, &destination).map_err(|_| "The retained version could not be restored. The compressed copy was kept.")?;
  Ok(json!({ "adb_path": adb_path(&destination).to_string_lossy(), "version": version, "reused": false, "restored": true }))
}

pub fn restore(base: &Path, version: &str) -> Result<Value, String> {
  let _installing = INSTALLING.try_lock().map_err(|_| "An ADB installation is already in progress.")?;
  restore_cached(base, version, Some(&mut version_output))
}

fn within(path: &Path, root: &Path) -> bool {
  #[cfg(windows)]
  { let path = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase(); let root = root.to_string_lossy().replace('/', "\\").to_ascii_lowercase(); path == root || path.starts_with(&(root + "\\")) }
  #[cfg(not(windows))]
  { path.starts_with(root) }
}

fn protected(root: &Path, paths: &[PathBuf]) -> Result<bool, String> {
  let root = root.canonicalize().map_err(|_| INVALID)?;
  Ok(paths.iter().any(|path| within(path, &root)))
}

fn file_locks(root: &Path, record: &Receipt) -> Result<Vec<fs::File>, String> {
  #[cfg(windows)]
  {
    use std::os::windows::fs::OpenOptionsExt;
    let mut handles = Vec::new();
    for path in record.files.iter().map(|file| root.join(&file.path)).chain(Some(root.join(RECEIPT))) {
      // A loaded/locked executable must not be retired. DELETE sharing permits
      // the subsequent directory rename, while new readers/writers are denied.
      let file = OpenOptions::new().read(true).write(true).share_mode(4).open(path)
        .map_err(|_| "An inactive ADB file is still in use or cannot be retired. The full copy was kept.")?;
      handles.push(file);
    }
    Ok(handles)
  }
  #[cfg(not(windows))]
  { let _ = (root, record); Err("Inactive ADB compression is currently limited to Windows, where loaded-file protection is verified.".into()) }
}

fn compact_one(base: &Path, version: &str, paths: &[PathBuf], rename: impl Fn(&Path, &Path) -> std::io::Result<()>, remove: impl Fn(&Path) -> std::io::Result<()>) -> Result<Value, String> {
  let root = base.join("adb");
  let source = root.join(version);
  let record = receipt(&source)?;
  if protected(&source, paths)? { return Ok(json!({ "version": version, "compressed": false, "reason": "selected_or_running" })); }
  let retained_root = root.join(".retained");
  normal_directory(&retained_root)?;
  fs::create_dir_all(&retained_root).map_err(|e| e.to_string())?;
  let destination = cache_root(base, version)?;
  let pending = if fs::symlink_metadata(&destination).is_ok() { None } else { Some(TemporaryDirectory::new(&retained_root)?) };
  let retained = if let Some(stage) = &pending { write_cache(&source, &record, &stage.path)? } else { cached(base, version)? };
  if serde_json::to_vec(&record).map_err(|_| INVALID)? != serde_json::to_vec(&retained.receipt).map_err(|_| INVALID)? { return Err("The compressed and full ADB copies differ. Both were kept.".into()); }
  let uncompressed = record.files.iter().try_fold(0u64, |total, item| total.checked_add(item.size).ok_or(INVALID))?
    .checked_add(fs::metadata(source.join(RECEIPT)).map_err(|_| INVALID)?.len()).ok_or(INVALID)?;
  let cache_path = pending.as_ref().map(|stage| stage.path.as_path()).unwrap_or(destination.as_path());
  let footprint = retained.archive_size.checked_add(fs::metadata(cache_path.join("manifest.json")).map_err(|_| INVALID)?.len()).ok_or(INVALID)?;
  let savings = 1.0 - footprint as f64 / uncompressed.max(1) as f64;
  if savings < 0.30 { return Ok(json!({ "version": version, "compressed": false, "reason": "insufficient_savings", "savings_percent": savings * 100.0 })); }
  let archive = pending.as_ref().map(|stage| stage.path.join("archive.zip")).unwrap_or_else(|| destination.join("archive.zip"));
  let round_trip = TemporaryDirectory::new(&root)?;
  let started = Instant::now();
  extract_zip(&fs::read(&archive).map_err(|_| INVALID)?, &round_trip.path)?;
  let restored = receipt_at(&round_trip.path, version)?;
  if serde_json::to_vec(&restored).map_err(|_| INVALID)? != serde_json::to_vec(&record).map_err(|_| INVALID)? { return Err(INVALID.into()); }
  let restore_ms = started.elapsed().as_secs_f64() * 1000.0;
  if restore_ms > 2000.0 { return Ok(json!({ "version": version, "compressed": false, "reason": "restore_too_slow", "restore_ms": restore_ms })); }
  if let Some(stage) = &pending {
    if fs::symlink_metadata(&destination).is_ok() { return Err("The retained cache changed during compression. The full copy was kept.".into()); }
    fs::rename(&stage.path, &destination).map_err(|_| "Could not commit the retained cache. The full copy was kept.")?;
  }
  // Recheck the original after the round trip and only then reserve its files.
  let current = receipt(&source)?;
  if serde_json::to_vec(&current).map_err(|_| INVALID)? != serde_json::to_vec(&record).map_err(|_| INVALID)? {
    return Err("The inactive ADB copy changed during compression. Its complete directory was kept.".into());
  }
  if protected(&source, paths)? { return Ok(json!({ "version": version, "compressed": false, "reason": "selected_or_running" })); }
  let locks = file_locks(&source, &record)?;
  let retired = root.join(format!(".retired-{}-{}", std::process::id(), NEXT_STAGE.fetch_add(1, Ordering::Relaxed)));
  if fs::symlink_metadata(&retired).is_ok() { return Err("Could not reserve ADB retirement. The full copy was kept.".into()); }
  // Windows refuses the parent-directory move while our own write probes are
  // open. Release only those probes, then retire the entire directory atomically;
  // an external lock that prevents retirement leaves the full original in place.
  drop(locks);
  rename(&source, &retired).map_err(|error| format!("The inactive ADB directory could not be retired. The full copy was kept: {}", error))?;
  normal_directory(&retired)?;
  if retired.canonicalize().map_err(|_| INVALID)?.parent() != Some(root.canonicalize().map_err(|_| INVALID)?.as_path()) { return Err(INVALID.into()); }
  if remove(&retired).is_err() {
    // Recursive deletion can fail partway through. Reconstruct the complete
    // original directory from the verified cache rather than return a partial
    // working copy; any retirement remnants remain outside the version list.
    restore_cached(base, version, None)?;
    return Ok(json!({ "version": version, "compressed": false, "reason": "cleanup_failed_full_copy_restored", "cache_kept": true }));
  }
  Ok(json!({ "version": version, "compressed": true, "savings_percent": savings * 100.0, "restore_ms": restore_ms }))
}

pub fn compact_inactive(base: &Path, protected_paths: &[String]) -> Result<Value, String> {
  let _installing = INSTALLING.try_lock().map_err(|_| "An ADB installation is already in progress.")?;
  if protected_paths.is_empty() { return Err("The selected and running ADB paths must be known before compression.".into()); }
  let mut paths = Vec::new();
  for path in protected_paths {
    if !Path::new(path).is_absolute() { return Err("A protected ADB path could not be resolved. Compression was skipped.".into()); }
    paths.push(Path::new(path).canonicalize().map_err(|_| "A protected ADB path is unavailable. Compression was skipped.")?);
  }
  if !cfg!(windows) { return Ok(json!({ "results": [], "reason": "compression_not_supported_on_this_platform" })); }
  let versions = installed(base)?;
  let mut results = Vec::new();
  let mut attempted = 0;
  let mut remaining = 0;
  for item in versions.into_iter().filter(|item| !item.compressed) {
    let source = base.join("adb").join(&item.version);
    if protected(&source, &paths).unwrap_or(true) {
      results.push(json!({ "version": item.version, "compressed": false, "reason": "selected_or_running" }));
      continue;
    }
    if attempted >= 2 { remaining += 1; continue; }
    attempted += 1;
    match compact_one(base, &item.version, &paths, |from, to| fs::rename(from, to), |path| fs::remove_dir_all(path)) {
      Ok(result) => results.push(result),
      Err(error) => results.push(json!({ "version": item.version, "compressed": false, "reason": "kept", "error": error })),
    }
  }
  Ok(json!({ "results": results, "remaining": remaining }))
}

#[cfg(test)]
mod tests {
  use super::*;
  static COMPACTION_TESTS: Mutex<()> = Mutex::new(());

  struct TestDirectory(PathBuf);
  impl TestDirectory {
    fn new() -> Self {
      let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
      let path = std::env::temp_dir().join(format!("scrcpy-plus-adb-installer-{}-{}", std::process::id(), nonce));
      fs::create_dir(&path).unwrap(); Self(path)
    }
  }
  impl Drop for TestDirectory { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

  fn native_header() -> Vec<u8> {
    let mut bytes = vec![0; 256];
    match std::env::consts::OS {
      "windows" => { bytes[..2].copy_from_slice(b"MZ"); bytes[60..64].copy_from_slice(&128u32.to_le_bytes()); bytes[128..132].copy_from_slice(b"PE\0\0"); bytes[132..134].copy_from_slice(&0x8664u16.to_le_bytes()); },
      "linux" => { bytes[..6].copy_from_slice(b"\x7fELF\x02\x01"); bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); },
      "macos" => { bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); let cpu = if std::env::consts::ARCH == "aarch64" { 0x0100000cu32 } else { 0x01000007u32 }; bytes[4..8].copy_from_slice(&cpu.to_le_bytes()); },
      _ => {},
    }
    bytes
  }

  fn archive(missing: Option<&str>, source_version: &str) -> Vec<u8> {
    archive_with_payload(missing, source_version, &vec![b'x'; 65536])
  }

  fn archive_with_payload(missing: Option<&str>, source_version: &str, payload: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let executable = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut files = vec![(executable, native_header()), ("NOTICE.txt", b"synthetic required notices".to_vec()), ("source.properties", format!("Pkg.Revision = {}\n", source_version).into_bytes()), ("optional-empty", Vec::new()), ("retained-payload", payload.to_vec())];
    if cfg!(windows) { for name in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] { files.push((name, b"synthetic DLL".to_vec())); } }
    for (name, bytes) in files {
      if missing == Some(name) { continue; }
      archive.start_file(format!("platform-tools/{}", name), zip::write::SimpleFileOptions::default()).unwrap();
      archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
  }

  fn release(version: &str, bytes: &[u8]) -> Release {
    Release { version: version.into(), url: format!("{}platform-tools_r{}-{}.zip", GOOGLE, version, platform().unwrap()), release_url: "https://developer.android.com/tools/releases/platform-tools".into(),
      checksum: hex(&Sha256::digest(bytes)), checksum_type: "sha256".into(), size: bytes.len() as u64 }
  }

  fn fake_version(_: &Path) -> Result<String, String> { Ok("Android Debug Bridge version 1.0.41\nVersion 37.0.3-1234\n".into()) }
  fn install_fixture(base: &Path) -> Value {
    let bytes = archive(None, "37.0.3");
    install(base, &release("37.0.3", &bytes), bytes, &mut fake_version, |from, to| fs::rename(from, to)).unwrap()
  }

  #[test]
  fn official_url_revision_and_publisher_digest_are_required() {
    let mut metadata = release("37.0.3", b"archive");
    assert!(validate_adb(&metadata).is_ok());
    for bad in ["", "../37.0.3", "v37.0.3", "37.0", "37.0.3-rc1", "037.0.3"] { metadata.version = bad.into(); assert!(validate_adb(&metadata).is_err()); }
    metadata = release("37.0.3", b"archive");
    for bad in ["https://evil.example/platform-tools_r37.0.3-win.zip", "https://dl.google.com.evil.example/android/repository/platform-tools_r37.0.3-win.zip", "https://dl.google.com/android/repository/platform-tools-latest-windows.zip", "https://dl.google.com/android/repository/platform-tools_r37.0.3-win.zip?extra=1"] { metadata.url = bad.into(); assert!(validate_adb(&metadata).is_err()); }
    metadata = release("37.0.3", b"archive"); metadata.checksum.clear(); assert!(validate_adb(&metadata).is_err());
    metadata = release("37.0.3", b"archive"); metadata.size = MAX_DOWNLOAD + 1; assert!(validate_adb(&metadata).is_err());
  }

  #[test]
  fn downloaded_size_digest_and_interruption_are_verified_before_extraction() {
    let bytes = b"synthetic archive";
    let mut metadata = release("37.0.3", bytes);
    assert_eq!(read_archive(Cursor::new(bytes), &metadata).unwrap(), bytes);
    metadata.checksum_type = "sha1".into(); metadata.checksum = hex(&Sha1::digest(bytes));
    assert_eq!(read_archive(Cursor::new(bytes), &metadata).unwrap(), bytes);
    assert!(read_archive(Cursor::new(b"wrong size"), &metadata).is_err());
    metadata.checksum = "0".repeat(40); assert!(read_archive(Cursor::new(bytes), &metadata).is_err());
    struct Interrupted;
    impl Read for Interrupted { fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> { Err(std::io::ErrorKind::ConnectionReset.into()) } }
    assert!(read_archive(Interrupted, &metadata).unwrap_err().contains("interrupted"));
  }

  #[test]
  fn archive_paths_cannot_escape_or_target_windows_special_files() {
    for name in ["../outside", "/absolute", "platform-tools/../outside", "platform-tools\\adb.exe", "platform-tools/file:stream", "platform-tools/NUL.txt", "platform-tools/CON", "platform-tools/adb.exe.", "platform-tools/a//b", "platform-tools/./adb", "different/adb"] { assert!(relative_path(name).is_err(), "accepted {}", name); }
    assert_eq!(relative_path("platform-tools/lib64/libc++.so").unwrap(), Path::new("platform-tools/lib64/libc++.so"));
  }

  #[test]
  fn extraction_rejects_duplicate_link_and_oversized_entries() {
    let root = TestDirectory::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["platform-tools/adb", "platform-tools/ADB"] { zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(b"contents").unwrap(); }
    assert!(extract(zip.finish().unwrap().into_inner(), &root.0, LIMITS).is_err());
    let root = TestDirectory::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink("platform-tools/linked", "outside", zip::write::SimpleFileOptions::default()).unwrap();
    assert!(extract(zip.finish().unwrap().into_inner(), &root.0, LIMITS).is_err());
    let root = TestDirectory::new();
    assert!(extract(archive(None, "37.0.3"), &root.0, Limits { file: 4, ..LIMITS }).is_err());
    let root = TestDirectory::new();
    assert!(extract(archive(None, "37.0.3"), &root.0, Limits { entries: 1, ..LIMITS }).is_err());
    let root = TestDirectory::new();
    assert!(extract(archive(None, "37.0.3"), &root.0, Limits { total: 4, ..LIMITS }).is_err());
  }

  #[test]
  fn complete_matching_runtime_is_staged_and_receipted_without_touching_other_tools() {
    let root = TestDirectory::new();
    fs::write(root.0.join("scrcpy-override"), b"keep other tools").unwrap();
    let result = install_fixture(&root.0);
    assert_eq!(result["version"], "37.0.3");
    assert_eq!(fs::read(root.0.join("scrcpy-override")).unwrap(), b"keep other tools");
    let versions = installed(&root.0).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version, "37.0.3");
    assert_eq!(versions[0].adb_path, result["adb_path"]);
    assert_eq!(fs::read_dir(root.0.join("adb")).unwrap().count(), 1);
  }

  #[test]
  fn managed_selection_aliases_validate_the_complete_receipt_before_execution() {
    let root = TestDirectory::new(); install_fixture(&root.0);
    let directory = root.0.join("adb/37.0.3");
    let aliased_base = root.0.join("adb/..");
    let alias = directory.join("platform-tools/..").join("platform-tools").join(if cfg!(windows) { "adb.exe" } else { "adb" });
    assert_eq!(verify_selected_with(&aliased_base, alias.to_str().unwrap(), &mut fake_version).unwrap(), "37.0.3");
    #[cfg(windows)]
    assert_eq!(verify_selected_with(&root.0, &alias.to_string_lossy().to_uppercase(), &mut fake_version).unwrap(), "37.0.3");
    let receipt_path = directory.join(RECEIPT);
    let mut record: Value = serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
    record["files"][0]["sha256"] = "0".repeat(64).into();
    fs::write(&receipt_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(verify_selected_with(&aliased_base, alias.to_str().unwrap(), &mut |_| panic!("A damaged managed receipt must not execute")).is_err());
    #[cfg(windows)]
    assert!(verify_selected_with(&root.0, &alias.to_string_lossy().to_uppercase(), &mut |_| panic!("A case alias must not bypass managed verification")).is_err());
  }

  #[test]
  fn missing_notices_runtime_dependencies_and_wrong_revision_never_publish() {
    let mut names = vec!["NOTICE.txt", "source.properties", if cfg!(windows) { "adb.exe" } else { "adb" }];
    if cfg!(windows) { names.extend(["AdbWinApi.dll", "AdbWinUsbApi.dll"]); }
    for missing in names {
      let root = TestDirectory::new(); let bytes = archive(Some(missing), "37.0.3");
      assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut |_| panic!("Incomplete runtime must not execute"), |from, to| fs::rename(from, to)).is_err());
      assert_eq!(fs::read_dir(root.0.join("adb")).unwrap().count(), 0);
    }
    let root = TestDirectory::new(); let bytes = archive(None, "37.0.2");
    assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut |_| panic!("Wrong revision must not execute"), |from, to| fs::rename(from, to)).is_err());
  }

  #[test]
  fn runtime_version_is_checked_before_publishing_and_protocol_is_not_a_package_revision() {
    assert!(matches_version("Android Debug Bridge version 1.0.41\nVersion 37.0.3-16887417\n", "37.0.3"));
    for output in ["Android Debug Bridge version 1.0.41", "Android Debug Bridge version 1.0.41\nVersion 37.0.2-1234", "Android Debug Bridge version 1.0.41\nVersion 37.0.30-1234", "Android Debug Bridge version 1.0.41\nVersion 37.0.3\nVersion 37.0.3"] { assert!(!matches_version(output, "37.0.3")); }
    let root = TestDirectory::new(); let bytes = archive(None, "37.0.3");
    assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut |_| Ok("Android Debug Bridge version 1.0.41\nVersion 37.0.2-1234".into()), |from, to| fs::rename(from, to)).is_err());
    assert_eq!(fs::read_dir(root.0.join("adb")).unwrap().count(), 0);
  }

  #[test]
  fn altered_extra_missing_files_and_forged_receipt_paths_are_not_listed() {
    for kind in ["altered", "extra", "missing", "receipt"] {
      let root = TestDirectory::new(); install_fixture(&root.0);
      let destination = root.0.join("adb/37.0.3");
      match kind {
        "altered" => fs::write(adb_path(&destination), b"modified tool").unwrap(),
        "extra" => fs::write(destination.join("extra-file"), b"unrecorded").unwrap(),
        "missing" => fs::remove_file(destination.join("platform-tools/NOTICE.txt")).unwrap(),
        _ => { let path = destination.join(RECEIPT); let mut record: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap(); record["files"][0]["path"] = "../outside".into(); fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap(); },
      }
      assert!(installed(&root.0).unwrap().is_empty(), "listed {} runtime", kind);
    }
  }

  #[test]
  fn same_version_is_reused_without_overwriting_and_different_metadata_is_rejected() {
    let root = TestDirectory::new(); let first = install_fixture(&root.0);
    let bytes = archive(None, "37.0.3"); let mut metadata = release("37.0.3", &bytes);
    let again = install(&root.0, &metadata, Vec::new(), &mut fake_version, |_, _| panic!("Existing version must not be overwritten")).unwrap();
    assert_eq!(again["adb_path"], first["adb_path"]); assert_eq!(again["reused"], true);
    metadata.checksum = "0".repeat(64);
    assert!(existing(&root.0, &metadata, &mut fake_version).is_err());
    assert_eq!(installed(&root.0).unwrap().len(), 1);
  }

  #[test]
  fn publish_failure_keeps_prior_versions_and_removes_its_staging_directory() {
    let root = TestDirectory::new(); let previous = root.0.join("adb/37.0.2"); fs::create_dir_all(&previous).unwrap(); fs::write(previous.join("working-copy"), b"old working tools").unwrap();
    let bytes = archive(None, "37.0.3");
    assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut fake_version, |_, _| Err(std::io::ErrorKind::PermissionDenied.into())).is_err());
    assert_eq!(fs::read(previous.join("working-copy")).unwrap(), b"old working tools");
    assert_eq!(fs::read_dir(root.0.join("adb")).unwrap().count(), 1);
  }

  #[test]
  fn an_existing_invalid_destination_is_preserved() {
    let root = TestDirectory::new(); let destination = root.0.join("adb/37.0.3"); fs::create_dir_all(&destination).unwrap(); fs::write(destination.join("working-copy"), b"keep existing").unwrap();
    let bytes = archive(None, "37.0.3");
    assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut fake_version, |from, to| fs::rename(from, to)).is_err());
    assert_eq!(fs::read(destination.join("working-copy")).unwrap(), b"keep existing");
  }

  #[test]
  fn corrupt_packages_and_wrong_binary_headers_are_rejected_without_execution() {
    let root = TestDirectory::new();
    let previous = root.0.join("adb/37.0.2");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("working-copy"), b"keep previous tools").unwrap();
    let bytes = b"not a zip archive".to_vec();
    assert!(install(&root.0, &release("37.0.3", &bytes), bytes, &mut |_| panic!("A corrupt package must never execute"), |from, to| fs::rename(from, to)).is_err());
    assert_eq!(fs::read(previous.join("working-copy")).unwrap(), b"keep previous tools");
    assert_eq!(fs::read_dir(root.0.join("adb")).unwrap().count(), 1);
    let executable = root.0.join("untrusted-binary");
    fs::write(&executable, b"#!not-a-native-executable").unwrap();
    assert!(runtime_header(&executable).is_err());
    let mut header = native_header();
    match std::env::consts::OS {
      "windows" => header[132..134].copy_from_slice(&0x14cu16.to_le_bytes()),
      "linux" => header[18..20].copy_from_slice(&0xb7u16.to_le_bytes()),
      "macos" => header[4..8].copy_from_slice(&0u32.to_le_bytes()),
      _ => {},
    }
    fs::write(&executable, header).unwrap();
    assert!(runtime_header(&executable).is_err());
  }

  #[test]
  fn generic_zip_extraction_keeps_scrcpy_layout_and_the_same_safety_limits() {
    let root = TestDirectory::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("scrcpy-v5/scrcpy.exe", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"synthetic, never executed").unwrap();
    extract_zip(&zip.finish().unwrap().into_inner(), &root.0).unwrap();
    assert_eq!(fs::read(root.0.join("scrcpy-v5/scrcpy.exe")).unwrap(), b"synthetic, never executed");
    let root = TestDirectory::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("../outside", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"unsafe").unwrap();
    assert!(extract_zip(&zip.finish().unwrap().into_inner(), &root.0).is_err());
  }

  fn selected_outside_versions(base: &Path) -> Vec<String> {
    let path = base.join("selected-adb");
    fs::write(&path, b"separate selected tool; never executed").unwrap();
    vec![path.to_string_lossy().to_string()]
  }

  #[cfg(windows)]
  #[test]
  fn selected_client_and_running_server_versions_stay_unpacked() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    let result = install_fixture(&root.0);
    let mut paths = selected_outside_versions(&root.0);
    paths.push(result["adb_path"].as_str().unwrap().to_string());
    let result = compact_inactive(&root.0, &paths).unwrap();
    assert_eq!(result["results"][0]["reason"], "selected_or_running");
    assert!(!installed(&root.0).unwrap()[0].compressed);
    assert!(!root.0.join("adb/.retained").exists());
    assert!(compact_inactive(&root.0, &[]).is_err());
    assert!(compact_inactive(&root.0, &["missing-or-relative".into()]).is_err());
  }

  #[cfg(windows)]
  #[test]
  fn inactive_versions_compress_and_restore_offline_with_verified_receipts() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    let original = install_fixture(&root.0);
    let result = compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    assert_eq!(result["results"][0]["compressed"], true, "{}", result);
    assert!(result["results"][0]["savings_percent"].as_f64().unwrap() >= 30.0);
    assert!(result["results"][0]["restore_ms"].as_f64().unwrap() <= 2000.0);
    assert!(!root.0.join("adb/37.0.3").exists());
    let versions = installed(&root.0).unwrap();
    assert_eq!(versions.len(), 1);
    assert!(versions[0].compressed && versions[0].adb_path.is_empty());
    let restored = restore_cached(&root.0, "37.0.3", Some(&mut fake_version)).unwrap();
    assert_eq!(restored["adb_path"], original["adb_path"]);
    assert_eq!(restored["restored"], true);
    assert!(!installed(&root.0).unwrap()[0].compressed);
    assert!(cached(&root.0, "37.0.3").is_ok());
    let again = compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    assert_eq!(again["results"][0]["compressed"], true);
    assert_eq!(installed(&root.0).unwrap().len(), 1);
  }

  #[cfg(windows)]
  #[test]
  fn damaged_retained_archives_are_unselectable_and_never_deleted() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    install_fixture(&root.0);
    compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    let archive = root.0.join("adb/.retained/37.0.3/archive.zip");
    let mut bytes = fs::read(&archive).unwrap();
    bytes[0] ^= 1;
    fs::write(&archive, &bytes).unwrap();
    assert!(installed(&root.0).unwrap().is_empty());
    assert!(restore_cached(&root.0, "37.0.3", Some(&mut fake_version)).is_err());
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    assert!(archive.parent().unwrap().join("manifest.json").exists());
    assert!(!root.0.join("adb/37.0.3").exists());
  }

  #[cfg(windows)]
  #[test]
  fn loaded_or_locked_adb_files_keep_the_complete_original_directory() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    use std::os::windows::fs::OpenOptionsExt;
    let root = TestDirectory::new();
    let result = install_fixture(&root.0);
    // Reads are permitted, as for a loaded program, while write/deletion are not.
    let held = OpenOptions::new().read(true).share_mode(1).open(result["adb_path"].as_str().unwrap()).unwrap();
    let result = compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    assert_eq!(result["results"][0]["compressed"], false);
    assert!(result["results"][0]["error"].as_str().unwrap().contains("in use"));
    assert!(receipt(&root.0.join("adb/37.0.3")).is_ok());
    assert!(!installed(&root.0).unwrap()[0].compressed);
    drop(held);
  }

  #[cfg(windows)]
  #[test]
  fn failed_retirement_keeps_the_full_copy_and_partial_cleanup_restores_it() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    install_fixture(&root.0);
    let paths = selected_outside_versions(&root.0).into_iter().map(|path| PathBuf::from(path).canonicalize().unwrap()).collect::<Vec<_>>();
    assert!(compact_one(&root.0, "37.0.3", &paths, |_, _| Err(std::io::ErrorKind::PermissionDenied.into()), |path| fs::remove_dir_all(path)).is_err());
    assert!(receipt(&root.0.join("adb/37.0.3")).is_ok());
    assert!(cached(&root.0, "37.0.3").is_ok());
    let result = compact_one(&root.0, "37.0.3", &paths, |from, to| fs::rename(from, to), |retired| {
      fs::remove_file(retired.join("platform-tools/NOTICE.txt"))?;
      Err(std::io::ErrorKind::PermissionDenied.into())
    }).unwrap();
    assert_eq!(result["reason"], "cleanup_failed_full_copy_restored");
    assert!(receipt(&root.0.join("adb/37.0.3")).is_ok());
    assert_eq!(fs::read(root.0.join("adb/37.0.3/platform-tools/NOTICE.txt")).unwrap(), b"synthetic required notices");
    assert!(!installed(&root.0).unwrap()[0].compressed);
  }

  #[cfg(windows)]
  #[test]
  fn incompressible_packages_remain_full_without_committing_an_archive() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    let mut seed = 0x12345678u32;
    let payload = (0..65536).map(|_| { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; seed as u8 }).collect::<Vec<_>>();
    let bytes = archive_with_payload(None, "37.0.3", &payload);
    install(&root.0, &release("37.0.3", &bytes), bytes, &mut fake_version, |from, to| fs::rename(from, to)).unwrap();
    let result = compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    assert_eq!(result["results"][0]["reason"], "insufficient_savings");
    assert!(receipt(&root.0.join("adb/37.0.3")).is_ok());
    assert!(!root.0.join("adb/.retained/37.0.3").exists());
  }

  #[cfg(windows)]
  #[test]
  fn retained_manifest_overhead_counts_against_the_savings_gate() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    let bytes = archive_with_payload(None, "37.0.3", &[b'x'; 2048]);
    install(&root.0, &release("37.0.3", &bytes), bytes, &mut fake_version, |from, to| fs::rename(from, to)).unwrap();
    let source = root.0.join("adb/37.0.3");
    let record = receipt(&source).unwrap();
    let stage = TemporaryDirectory::new(&root.0.join("adb")).unwrap();
    let retained = write_cache(&source, &record, &stage.path).unwrap();
    let uncompressed = record.files.iter().map(|file| file.size).sum::<u64>() + fs::metadata(source.join(RECEIPT)).unwrap().len();
    let manifest_size = fs::metadata(stage.path.join("manifest.json")).unwrap().len();
    assert!(1.0 - retained.archive_size as f64 / uncompressed as f64 >= 0.30, "This fixture must benefit if the new manifest is incorrectly omitted.");
    assert!(1.0 - (retained.archive_size + manifest_size) as f64 / (uncompressed as f64) < 0.30);
    let result = compact_inactive(&root.0, &selected_outside_versions(&root.0)).unwrap();
    assert_eq!(result["results"][0]["reason"], "insufficient_savings");
    assert!(receipt(&source).is_ok());
    assert!(!root.0.join("adb/.retained/37.0.3").exists());
  }

  #[cfg(windows)]
  #[test]
  fn compaction_batches_at_most_two_eligible_versions_without_network() {
    let _serial = COMPACTION_TESTS.lock().unwrap_or_else(|error| error.into_inner());
    let root = TestDirectory::new();
    for version in ["37.0.1", "37.0.2", "37.0.3"] {
      let bytes = archive(None, version);
      install(&root.0, &release(version, &bytes), bytes, &mut |_| Ok(format!("Android Debug Bridge version 1.0.41\nVersion {}-synthetic\n", version)), |from, to| fs::rename(from, to)).unwrap();
    }
    let paths = selected_outside_versions(&root.0);
    let first = compact_inactive(&root.0, &paths).unwrap();
    assert_eq!(first["remaining"], 1);
    assert_eq!(installed(&root.0).unwrap().iter().filter(|item| item.compressed).count(), 2);
    assert!(receipt(&root.0.join("adb/37.0.1")).is_ok());
    let second = compact_inactive(&root.0, &paths).unwrap();
    assert_eq!(second["remaining"], 0);
    assert_eq!(installed(&root.0).unwrap().iter().filter(|item| item.compressed).count(), 3);
  }

  #[test]
  #[ignore = "Requires explicit official-package download authorization; installs only into a disposable test directory, runs --version only, and never changes selected tools or starts ADB"]
  fn download_official_adb_into_disposable_directory() {
    assert_eq!(std::env::var("SCRCPY_PLUS_TEST_ADB_DOWNLOAD").as_deref(), Ok("1"));
    let root = TestDirectory::new();
    let releases = crate::tool_catalog::adb_releases().unwrap();
    let latest = releases.first().expect("Google did not publish a supported stable release.");
    let result = download(&root.0, latest).unwrap();
    assert_eq!(verify_selected(&root.0, result["adb_path"].as_str().unwrap()).unwrap(), latest.version);
    let versions = installed(&root.0).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version, latest.version);
    println!("Verified official Platform-Tools {} in a disposable directory, including published digest, native version, complete runtime, and offline receipt listing.", latest.version);
    let path = root.0.clone();
    drop(root);
    assert!(!path.exists(), "The disposable installation could not be cleaned up.");
  }

  #[test]
  #[ignore = "Benchmarks explicit local ADB runtime files using disposable copies only; never downloads, executes tools, or changes selected versions"]
  fn benchmark_local_adb_compression() {
    let source = PathBuf::from(std::env::var("SCRCPY_PLUS_TEST_ADB_COMPRESSION_SOURCE").expect("Select the local benchmark source."));
    let report = PathBuf::from(std::env::var("SCRCPY_PLUS_TEST_ADB_COMPRESSION_REPORT").expect("Select the task-owned benchmark report path."));
    let names = ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "adb-NOTICE.txt"];
    let mut source_files = Vec::new();
    let mut uncompressed = 0u64;
    for name in names {
      let path = source.join(name);
      let (size, digest) = hash_file(&path, LIMITS.file).unwrap();
      uncompressed += size;
      source_files.push((name, path, size, digest));
    }
    let temporary = TestDirectory::new();
    let mut trials = Vec::new();
    for trial in 0..3 {
      let archive_path = temporary.0.join(format!("trial-{}.zip", trial));
      let start = Instant::now();
      let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
      for (name, path, _, _) in &source_files {
        archive.start_file(format!("platform-tools/{}", name), zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
        std::io::copy(&mut fs::File::open(path).unwrap(), &mut archive).unwrap();
      }
      archive.finish().unwrap().sync_all().unwrap();
      let compression_ms = start.elapsed().as_secs_f64() * 1000.0;
      let compressed = fs::metadata(&archive_path).unwrap().len();
      let start = Instant::now();
      let restored = temporary.0.join(format!("restored-{}", trial));
      fs::create_dir(&restored).unwrap();
      extract_zip(&fs::read(&archive_path).unwrap(), &restored).unwrap();
      for (name, _, size, digest) in &source_files {
        let actual = hash_file(&restored.join("platform-tools").join(name), LIMITS.file).unwrap();
        assert_eq!(actual.0, *size);
        assert_eq!(&actual.1, digest);
      }
      let restore_ms = start.elapsed().as_secs_f64() * 1000.0;
      trials.push(json!({ "trial": trial + 1, "uncompressed_bytes": uncompressed, "compressed_bytes": compressed,
        "savings_percent": 100.0 * (1.0 - compressed as f64 / uncompressed as f64), "compression_ms": compression_ms, "restore_and_verify_ms": restore_ms }));
    }
    let result = json!({ "source_kind": "ADB runtime subset bundled with preview v18; not a complete Platform-Tools SDK package",
      "files": names, "compression": "zip 9.8.2 Deflate defaults", "trials": trials,
      "timing": "monotonic wall-clock; compression includes source reads, archive writes and sync; restoration includes extraction and SHA-256 file verification",
      "selected_tools_changed": false, "executables_run": false, "network_used": false });
    fs::write(&report, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    println!("{}", serde_json::to_string(&result).unwrap());
    let path = temporary.0.clone();
    drop(temporary);
    assert!(!path.exists(), "The disposable benchmark files could not be cleaned up.");
  }
}
