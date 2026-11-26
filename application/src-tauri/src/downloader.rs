use serde_json::{ json, Value };
use std::fs;
use std::io::{ Cursor, Read };
use std::path::{ Path, PathBuf };
use std::sync::{ Mutex, atomic::{ AtomicU64, Ordering } };
#[cfg(test)]
use std::time::Duration;
use std::collections::HashSet;

// Only allow downloads from the official scrcpy release host
const ALLOWED_PREFIX: &str = "https://github.com/Genymobile/scrcpy/releases/download/";
#[cfg(test)]
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
static INSTALLING: Mutex<()> = Mutex::new(());
static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory { path: PathBuf, keep: bool }

impl TemporaryDirectory {
  fn new(root: &Path) -> Result<Self, String> {
    for _ in 0..32 {
      let id = TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
      let path = root.join(format!(".install-{}-{}", std::process::id(), id));
      match fs::create_dir(&path) {
        Ok(()) => return Ok(Self { path, keep: false }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error.to_string()),
      }
    }
    Err("Could not create a temporary installation directory.".into())
  }
}

impl Drop for TemporaryDirectory {
  fn drop(&mut self) {
    if !self.keep { let _ = fs::remove_dir_all(&self.path); }
  }
}

fn validate_version(version: &str) -> Result<(), String> {
  if version.len() > 128 || !version.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
    || !version.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
    || !version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
    return Err("Invalid version".into());
  }
  let name = version.split('.').next().unwrap().to_ascii_uppercase();
  if ["CON", "PRN", "AUX", "NUL"].contains(&name.as_str())
    || name.len() == 4 && ["COM", "LPT"].contains(&&name[..3]) && matches!(name.as_bytes()[3], b'1'..=b'9') {
    return Err("Invalid version".into());
  }
  Ok(())
}

fn validate_release_url(url: &str, version: &str) -> Result<(), String> {
  let (tag, file) = url.strip_prefix(ALLOWED_PREFIX).and_then(|path| path.split_once('/')).ok_or("Untrusted download URL")?;
  if tag != version || !file.chars().all(|c| c.is_ascii_alphanumeric() || ['.', '-', '_'].contains(&c))
    || !(file.ends_with(".zip") || file.ends_with(".tar.gz")) {
    return Err("The download URL must name an official archive for the selected version.".into());
  }
  Ok(())
}

fn is_link(metadata: &fs::Metadata) -> bool {
  #[cfg(windows)]
  {
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & 0x400 != 0 { return true; }
  }
  metadata.file_type().is_symlink()
}

fn check_directory(path: &Path) -> Result<(), String> {
  match fs::symlink_metadata(path) {
    Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => Err("The installation path must be a normal directory.".into()),
    Ok(_) => Ok(()),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error.to_string()),
  }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
  for entry in fs::read_dir(dir).ok()?.flatten() {
    let path = entry.path();
    let metadata = fs::symlink_metadata(&path).ok()?;
    if is_link(&metadata) { continue; }
    if metadata.is_dir() {
      if let Some(found) = find_file(&path, name) { return Some(found); }
    } else if metadata.is_file() && metadata.len() > 0 && path.file_name().map_or(false, |n| n == name) {
      return Some(path);
    }
  }
  None
}

fn required_file(path: &Path) -> Result<(), String> {
  match fs::symlink_metadata(path) {
    Ok(metadata) if metadata.is_file() && metadata.len() > 0 && !is_link(&metadata) => Ok(()),
    _ => Err(format!("Required runtime file not found in archive: {}", path.file_name().unwrap_or_default().to_string_lossy())),
  }
}

fn activate_directory(stage: &Path, dest: &Path, backup: &mut TemporaryDirectory, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<(), String> {
  let previous = backup.path.join("previous");
  let replacing = dest.exists();
  if replacing {
    rename(dest, &previous).map_err(|e| format!("Could not replace the existing version; close tools using it and try again: {}", e))?;
  }
  if let Err(error) = rename(stage, dest) {
    if replacing {
      if let Err(restore) = rename(&previous, dest) {
        backup.keep = true;
        return Err(format!("Could not activate the new version ({error}) or restore the previous version ({restore}). The previous files remain at {}", previous.display()));
      }
    }
    return Err(format!("Could not activate the new version: {}", error));
  }
  Ok(())
}

#[cfg(test)]
pub(crate) fn install_archive(base: &Path, url: &str, version: &str, bytes: Vec<u8>) -> Result<Value, String> {
  install_archive_with_policy(base, url, version, bytes, true)
}

fn install_archive_with_policy(base: &Path, url: &str, version: &str, bytes: Vec<u8>, replace: bool) -> Result<Value, String> {
  validate_version(version)?;
  let root = base.join("scrcpy");
  let dest = root.join(version);
  check_directory(&root)?;
  check_directory(&dest)?;
  if !replace && dest.exists() { return Err("This version is already kept locally. Reuse the verified copy; existing files will not be overwritten.".into()); }
  fs::create_dir_all(&root).map_err(|e| e.to_string())?;
  let stage = TemporaryDirectory::new(&root)?;

  if url.ends_with(".zip") {
    crate::adb_installer::extract_zip(&bytes, &stage.path)?;
  } else {
    extract_tar(bytes, &stage.path)?;
  }

  let ext = if cfg!(windows) { ".exe" } else { "" };
  let scrcpy = find_file(&stage.path, &format!("scrcpy{}", ext)).ok_or("scrcpy executable not found in archive")?;
  let adb = find_file(&stage.path, &format!("adb{}", ext)).ok_or("adb executable not found in archive")?;
  if !["scrcpy-server", "scrcpy-server.jar"].iter().any(|name| required_file(&scrcpy.parent().unwrap().join(name)).is_ok()) {
    return Err("scrcpy server file not found in archive".into());
  }
  #[cfg(windows)]
  for name in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] { required_file(&adb.parent().unwrap().join(name))?; }

  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    for p in [&scrcpy, &adb] {
      fs::set_permissions(p, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
  }

  let scrcpy = dest.join(scrcpy.strip_prefix(&stage.path).map_err(|e| e.to_string())?);
  let adb = dest.join(adb.strip_prefix(&stage.path).map_err(|e| e.to_string())?);
  if replace {
    let mut backup = TemporaryDirectory::new(&root)?;
    activate_directory(&stage.path, &dest, &mut backup, |from, to| fs::rename(from, to))?;
  } else {
    fs::rename(&stage.path, &dest).map_err(|error| format!("Could not keep the new version. Existing packages were preserved: {}", error))?;
  }

  Ok(json!({
    "scrcpy_path": scrcpy.to_string_lossy(),
    "adb_path":    adb.to_string_lossy()
  }))
}

fn extract_tar(bytes: Vec<u8>, stage: &Path) -> Result<(), String> {
  let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
  let mut archive = tar::Archive::new(decoder.take(1024 * 1024 * 1024 + 1));
  let mut seen = HashSet::new();
  let mut total = 0u64;
  for (index, entry) in archive.entries().map_err(|error| error.to_string())?.enumerate() {
    if index >= 4096 { return Err("The archive contains too many entries.".into()); }
    let mut entry = entry.map_err(|error| error.to_string())?;
    let kind = entry.header().entry_type();
    if !kind.is_file() && !kind.is_dir() { return Err("Archive links and special files are not supported.".into()); }
    let path = entry.path().map_err(|error| error.to_string())?.into_owned();
    if path.components().any(|part| !matches!(part, std::path::Component::Normal(_) | std::path::Component::CurDir))
      || path.to_string_lossy().contains(['\\', ':']) || !seen.insert(path.to_string_lossy().to_lowercase()) {
      return Err("The archive contains an unsafe or duplicate path.".into());
    }
    let size = entry.size();
    total = total.checked_add(size).ok_or("The archive is too large.")?;
    if size > 256 * 1024 * 1024 || total > 1024 * 1024 * 1024 { return Err("The archive exceeds extraction limits.".into()); }
    if !entry.unpack_in(stage).map_err(|error| error.to_string())? { return Err("The archive path is outside its staging directory.".into()); }
  }
  Ok(())
}

#[cfg(test)]
fn read_archive(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
  let mut bytes = Vec::new();
  reader.take(limit + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
  if bytes.len() as u64 > limit { return Err("The release archive exceeds the download limit.".into()); }
  Ok(bytes)
}

#[cfg(test)]
pub fn download_and_extract(base: &Path, url: &str, version: &str) -> Result<Value, String> {
  validate_version(version)?;
  validate_release_url(url, version)?;
  let _installing = INSTALLING.try_lock().map_err(|_| "A scrcpy installation is already in progress.")?;
  let response = ureq::get(url).config().timeout_global(Some(Duration::from_secs(60))).build().call().map_err(|e| e.to_string())?;
  let bytes = read_archive(response.into_body().into_reader(), MAX_DOWNLOAD)?;
  install_archive(base, url, version, bytes)
}

pub fn download_verified(base: &Path, release: &crate::tool_catalog::Release) -> Result<Value, String> {
  validate_version(&release.version)?;
  validate_release_url(&release.url, &release.version)?;
  let _installing = INSTALLING.try_lock().map_err(|_| "A tool installation is already in progress.")?;
  let bytes = crate::adb_installer::download_bytes(release)?;
  install_archive_with_policy(base, &release.url, &release.version, bytes, false)
}

pub fn kept_version(base: &Path, version: &str) -> Result<Option<Value>, String> {
  validate_version(version)?;
  let root = base.join("scrcpy");
  check_directory(&root)?;
  let directory = root.join(version);
  check_directory(&directory)?;
  Ok(kept_package(&directory, version))
}

pub fn kept_versions(base: &Path) -> Result<Vec<Value>, String> {
  let root = base.join("scrcpy");
  check_directory(&root)?;
  if !root.is_dir() { return Ok(Vec::new()); }
  let mut versions = Vec::new();
  for entry in fs::read_dir(&root).map_err(|error| error.to_string())?.take(128).flatten() {
    let version = entry.file_name().to_string_lossy().to_string();
    if validate_version(&version).is_err() || crate::tool_catalog::compare_versions(&version, &version).is_none() { continue; }
    let directory = entry.path();
    if check_directory(&directory).is_err() { continue; }
    if let Some(package) = kept_package(&directory, &version) { versions.push(package); }
  }
  versions.sort_by(|a, b| crate::tool_catalog::compare_versions(b["version"].as_str().unwrap_or_default(), a["version"].as_str().unwrap_or_default()).unwrap_or(std::cmp::Ordering::Equal));
  Ok(versions)
}

fn kept_package(directory: &Path, version: &str) -> Option<Value> {
  let ext = if cfg!(windows) { ".exe" } else { "" };
  let scrcpy = find_file(directory, &format!("scrcpy{}", ext))?;
  let adb = find_file(directory, &format!("adb{}", ext))?;
  let parent = scrcpy.parent()?;
  if !["scrcpy-server", "scrcpy-server.jar"].iter().any(|name| required_file(&parent.join(name)).is_ok()) { return None; }
  #[cfg(windows)]
  if ["AdbWinApi.dll", "AdbWinUsbApi.dll"].iter().any(|name| required_file(&adb.parent().unwrap().join(name)).is_err()) { return None; }
  // Pre-updater packages lack a new receipt. Verify the native executable's
  // actual version; a directory name alone is not evidence of a valid package.
  let actual = crate::system_command::inspect_tool_version("scrcpy", scrcpy.to_str()).ok().flatten()?;
  if crate::tool_catalog::compare_versions(&actual, version) != Some(std::cmp::Ordering::Equal) { return None; }
  Some(json!({ "version": version, "scrcpy_path": scrcpy.to_string_lossy(), "adb_path": adb.to_string_lossy() }))
}

pub fn verify_kept(base: &Path, path: &str) -> Result<(), String> {
  let root = base.join("scrcpy");
  check_directory(&root)?;
  let canonical_root = root.canonicalize().map_err(|_| "The kept scrcpy directory is unavailable.")?;
  let target = Path::new(path).canonicalize().map_err(|_| "The selected scrcpy executable is unavailable.")?;
  let relative = target.strip_prefix(&canonical_root).map_err(|_| "The kept package is outside application storage.")?;
  let version = relative.components().next().and_then(|part| part.as_os_str().to_str()).ok_or("The kept version could not be verified.")?;
  validate_version(version)?;
  let directory = root.join(version);
  check_directory(&directory)?;
  let package = kept_package(&directory, version).ok_or("The kept scrcpy package is incomplete or its version changed.")?;
  if package["scrcpy_path"].as_str().and_then(|path| Path::new(path).canonicalize().ok()).as_ref() != Some(&target) { return Err("The kept scrcpy executable changed.".into()); }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  struct TestDirectory { path: PathBuf }

  impl TestDirectory {
    fn new() -> Self {
      let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
      let path = std::env::temp_dir().join(format!("scrcpy-plus-installer-{}-{}", std::process::id(), nonce));
      fs::create_dir(&path).unwrap();
      Self { path }
    }
  }

  impl Drop for TestDirectory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.path); }
  }

  fn runtime_names() -> Vec<&'static str> {
    let mut files = vec![if cfg!(windows) { "scrcpy.exe" } else { "scrcpy" }, if cfg!(windows) { "adb.exe" } else { "adb" }, "scrcpy-server"];
    if cfg!(windows) { files.extend(["AdbWinApi.dll", "AdbWinUsbApi.dll"]); }
    files
  }

  fn archive(missing: Option<&str>) -> Vec<u8> {
    archive_with_server(missing, "scrcpy-server")
  }

  fn archive_with_server(missing: Option<&str>, server: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in runtime_names() {
      if missing == Some(name) { continue; }
      let name = if name == "scrcpy-server" { server } else { name };
      archive.start_file(format!("package/{}", name), zip::write::SimpleFileOptions::default()).unwrap();
      archive.write_all(b"new runtime").unwrap();
    }
    archive.finish().unwrap().into_inner()
  }

  fn tar_archive() -> Vec<u8> {
    let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    for name in runtime_names() {
      let mut header = tar::Header::new_gnu();
      header.set_mode(0o755);
      header.set_size(11);
      header.set_cksum();
      archive.append_data(&mut header, format!("package/{}", name), &b"new runtime"[..]).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
  }

  fn previous_install(base: &Path) -> PathBuf {
    let dest = base.join("scrcpy/v1.0");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("working-copy"), b"keep previous runtime").unwrap();
    dest
  }

  fn only_previous_remains(base: &Path, dest: &Path) {
    assert_eq!(fs::read(dest.join("working-copy")).unwrap(), b"keep previous runtime");
    assert_eq!(fs::read_dir(base.join("scrcpy")).unwrap().count(), 1);
  }

  #[test]
  fn invalid_versions_never_touch_existing_files() {
    let base = TestDirectory::new();
    let dest = previous_install(&base.path);
    for version in ["", ".", "..", "...", "v1.", "v1/../v2", "v1\\v2", "v1:stream", "CON", "NUL.txt", "COM1", "lpt9"] {
      assert!(download_and_extract(&base.path, &format!("{}v1/package.zip", ALLOWED_PREFIX), version).is_err());
      only_previous_remains(&base.path, &dest);
    }
    assert!(validate_version("v5.0.1").is_ok());
    assert!(validate_version("5.0.1-rc1").is_ok());
  }

  #[test]
  fn verified_update_installation_never_overwrites_a_retained_package() {
    let base = TestDirectory::new();
    let destination = previous_install(&base.path);
    assert!(install_archive_with_policy(&base.path, "package.zip", "v1.0", archive(None), false).is_err());
    only_previous_remains(&base.path, &destination);
  }

  #[test]
  fn a_directory_name_or_incomplete_kept_runtime_is_not_an_installed_version() {
    let base = TestDirectory::new();
    let destination = previous_install(&base.path);
    assert!(kept_version(&base.path, "v1.0").unwrap().is_none());
    assert!(kept_versions(&base.path).unwrap().is_empty());
    only_previous_remains(&base.path, &destination);
  }

  #[test]
  #[ignore = "Copies an explicitly supplied local scrcpy runtime into a disposable version directory and checks offline discovery; only --version is executed"]
  fn existing_runtime_is_listed_and_verified_offline_without_a_new_receipt() {
    let source = PathBuf::from(std::env::var("SCRCPY_PLUS_TEST_KEPT_RUNTIME").expect("Select the local test runtime explicitly."));
    let base = TestDirectory::new();
    let package = base.path.join("scrcpy/v5.0.1/package");
    fs::create_dir_all(&package).unwrap();
    for entry in fs::read_dir(&source).unwrap().flatten() {
      if entry.file_type().unwrap().is_file() { fs::copy(entry.path(), package.join(entry.file_name())).unwrap(); }
    }
    let kept = kept_versions(&base.path).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["version"], "v5.0.1");
    verify_kept(&base.path, kept[0]["scrcpy_path"].as_str().unwrap()).unwrap();
    assert!(kept_version(&base.path, "v5.0.1").unwrap().is_some());
    println!("Retained scrcpy 5.0.1 discovered and validated offline; no download, selected-tool change, ADB server command, or phone access.");
  }

  #[test]
  fn corrupt_archives_preserve_the_previous_install_and_clean_staging() {
    for url in ["package.zip", "package.tar.gz"] {
      let base = TestDirectory::new();
      let dest = previous_install(&base.path);
      assert!(install_archive(&base.path, url, "v1.0", b"invalid archive".to_vec()).is_err());
      only_previous_remains(&base.path, &dest);
    }
  }

  #[test]
  fn incomplete_runtime_preserves_previous_install() {
    for missing in [if cfg!(windows) { "adb.exe" } else { "adb" }, "scrcpy-server"] {
      let base = TestDirectory::new();
      let dest = previous_install(&base.path);
      assert!(install_archive(&base.path, "package.zip", "v1.0", archive(Some(missing))).is_err());
      only_previous_remains(&base.path, &dest);
    }
    #[cfg(windows)]
    for missing in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] {
      let base = TestDirectory::new();
      let dest = previous_install(&base.path);
      assert!(install_archive(&base.path, "package.zip", "v1.0", archive(Some(missing))).is_err());
      only_previous_remains(&base.path, &dest);
    }
  }

  #[test]
  fn valid_archive_activates_only_after_validation() {
    for (url, bytes) in [("package.zip", archive(None)), ("package.tar.gz", tar_archive())] {
      let base = TestDirectory::new();
      let dest = previous_install(&base.path);
      let result = install_archive(&base.path, url, "v1.0", bytes).unwrap();
      let scrcpy = PathBuf::from(result["scrcpy_path"].as_str().unwrap());
      let adb = PathBuf::from(result["adb_path"].as_str().unwrap());
      assert!(scrcpy.starts_with(&dest));
      assert!(adb.starts_with(&dest));
      assert_eq!(fs::read(scrcpy).unwrap(), b"new runtime");
      assert_eq!(fs::read(adb).unwrap(), b"new runtime");
      assert!(!dest.join("working-copy").exists());
      assert_eq!(fs::read_dir(base.path.join("scrcpy")).unwrap().count(), 1);
    }
  }

  #[test]
  fn failed_activation_restores_previous_install() {
    let base = TestDirectory::new();
    let dest = previous_install(&base.path);
    let root = base.path.join("scrcpy");
    let stage = TemporaryDirectory::new(&root).unwrap();
    let mut backup = TemporaryDirectory::new(&root).unwrap();
    let previous = backup.path.join("previous");
    let result = activate_directory(&stage.path, &dest, &mut backup, |from, to| {
      if from == stage.path { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Simulated activation failure")); }
      fs::rename(from, to)
    });
    assert!(result.is_err());
    assert_eq!(fs::read(dest.join("working-copy")).unwrap(), b"keep previous runtime");
    assert!(!previous.exists());
  }

  #[test]
  fn legacy_server_filename_is_accepted() {
    let base = TestDirectory::new();
    let result = install_archive(&base.path, "package.zip", "v1.10", archive_with_server(None, "scrcpy-server.jar")).unwrap();
    assert!(Path::new(result["scrcpy_path"].as_str().unwrap()).parent().unwrap().join("scrcpy-server.jar").is_file());
  }

  #[test]
  fn failed_rollback_retains_previous_files_for_recovery() {
    let base = TestDirectory::new();
    let dest = previous_install(&base.path);
    let root = base.path.join("scrcpy");
    let stage = TemporaryDirectory::new(&root).unwrap();
    let mut backup = TemporaryDirectory::new(&root).unwrap();
    let previous = backup.path.join("previous");
    let result = activate_directory(&stage.path, &dest, &mut backup, |from, to| {
      if from == stage.path || from == previous { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Simulated rename failure")); }
      fs::rename(from, to)
    });
    assert!(result.unwrap_err().contains("previous files remain at"));
    drop(backup);
    assert_eq!(fs::read(previous.join("working-copy")).unwrap(), b"keep previous runtime");
  }

  #[test]
  fn existing_non_directory_is_not_replaced() {
    let base = TestDirectory::new();
    let root = base.path.join("scrcpy");
    fs::create_dir(&root).unwrap();
    let dest = root.join("v1.0");
    fs::write(&dest, b"keep existing file").unwrap();
    assert!(install_archive(&base.path, "package.zip", "v1.0", archive(None)).is_err());
    assert_eq!(fs::read(&dest).unwrap(), b"keep existing file");
  }

  #[test]
  fn download_reads_are_bounded_and_report_interruption() {
    assert_eq!(read_archive(Cursor::new(b"1234"), 4).unwrap(), b"1234");
    assert!(read_archive(Cursor::new(b"12345"), 4).unwrap_err().contains("download limit"));
    struct Interrupted;
    impl Read for Interrupted {
      fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Simulated interrupted download"))
      }
    }
    assert!(read_archive(Interrupted, 4).unwrap_err().contains("interrupted download"));
  }

  #[test]
  fn concurrent_installation_is_rejected_before_network_access() {
    let _installing = INSTALLING.lock().unwrap();
    let base = TestDirectory::new();
    let error = download_and_extract(&base.path, &format!("{}v1.0/package.zip", ALLOWED_PREFIX), "v1.0").unwrap_err();
    assert!(error.contains("already in progress"));
    assert!(fs::read_dir(&base.path).unwrap().next().is_none());
  }

  #[test]
  fn release_urls_cannot_escape_the_official_tag_or_mismatch_the_selection() {
    assert!(validate_release_url(&format!("{}v1.0/scrcpy-win64-v1.0.zip", ALLOWED_PREFIX), "v1.0").is_ok());
    for path in ["v1.0/../../other/release.zip", "v2.0/package.zip", "v1.0/%2e%2e%2fpackage.zip", "v1.0/package.zip?redirect=other", "v1.0/file.exe"] {
      assert!(validate_release_url(&format!("{}{}", ALLOWED_PREFIX, path), "v1.0").is_err());
    }
  }
}
