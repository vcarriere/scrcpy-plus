use serde::{ Deserialize, Serialize };
use std::sync::Mutex;
use zeroize::{ Zeroize, Zeroizing };

const SCHEMA: u32 = 1;
const METHOD: &str = "password";
const INVALID: &str = "The saved unlock record is unavailable or invalid. Remove it and enroll this device again.";
const METHOD_CHANGED: &str = "The phone's unlock method changed. Enroll its password again before enabling automatic unlock.";
const UNSUPPORTED: &str = "Protected automatic-unlock storage is currently supported on Windows only.";
const MAX_RECORD: usize = 2560;
static STORE: Mutex<()> = Mutex::new(());

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub identity: String,
    pub user: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub binding: Binding,
    pub generation: String,
    pub enabled: bool,
    pub blocked: bool,
    pub invalidated: bool,
}

pub struct Secret { pub value: String }

impl Drop for Secret {
    fn drop(&mut self) { self.value.zeroize(); }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateRecord {
    schema: u32,
    method: String,
    metadata: Metadata,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PasswordRecord {
    schema: u32,
    method: String,
    binding: Binding,
    generation: String,
    password: String,
}

impl Drop for PasswordRecord {
    fn drop(&mut self) { self.password.zeroize(); }
}

fn valid_binding(binding: &Binding) -> bool {
    !binding.identity.is_empty() && binding.identity.len() <= 1024
        && !binding.identity.chars().any(char::is_control)
}

fn valid_password(password: &str) -> bool {
    (4..=64).contains(&password.len()) && password.bytes().all(|byte| (32..=126).contains(&byte))
}

fn valid_generation(generation: &str) -> bool {
    generation.len() == 32 && generation.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes { result.push(DIGITS[usize::from(byte >> 4)] as char); result.push(DIGITS[usize::from(byte & 15)] as char); }
    result
}

fn target(binding: &Binding, kind: &str) -> Result<String, String> {
    if !valid_binding(binding) { return Err(INVALID.into()); }
    // Public device identifiers label a vault slot; they are not encryption keys.
    Ok(format!("SCRCPY+/automatic-unlock/v1/{}/{}/{}", binding.user, hex(binding.identity.as_bytes()), kind))
}

trait Vault {
    fn read(&mut self, target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String>;
    fn write(&mut self, target: &str, bytes: &[u8]) -> Result<(), String>;
    fn remove(&mut self, target: &str) -> Result<(), String>;
    fn generation(&mut self) -> Result<String, String>;
}

fn encode(record: &impl Serialize) -> Result<Zeroizing<Vec<u8>>, String> {
    // The bounded records fit this allocation, so writing the password does not
    // leave a partially filled buffer behind through Vec growth/reallocation.
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_RECORD));
    serde_json::to_writer(&mut *bytes, record).map_err(|_| INVALID)?;
    if bytes.len() > MAX_RECORD { return Err(INVALID.into()); }
    Ok(bytes)
}

struct Store<V: Vault> { vault: V }

impl<V: Vault> Store<V> {
    fn metadata(&mut self, binding: &Binding) -> Result<Option<Metadata>, String> {
        let Some(bytes) = self.vault.read(&target(binding, "state")?)? else { return Ok(None); };
        if bytes.len() > MAX_RECORD { return Err(INVALID.into()); }
        let record: StateRecord = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        if record.schema != SCHEMA || record.method != METHOD || record.metadata.binding != *binding
            || !valid_generation(&record.metadata.generation)
            || record.metadata.invalidated && (record.metadata.enabled || !record.metadata.blocked) { return Err(INVALID.into()); }
        Ok(Some(record.metadata))
    }

    fn write_metadata(&mut self, metadata: &Metadata) -> Result<(), String> {
        if !valid_binding(&metadata.binding) || !valid_generation(&metadata.generation)
            || metadata.invalidated && (metadata.enabled || !metadata.blocked) { return Err(INVALID.into()); }
        let bytes = encode(&StateRecord { schema: SCHEMA, method: METHOD.into(), metadata: metadata.clone() })?;
        self.vault.write(&target(&metadata.binding, "state")?, &bytes)
    }

    fn enroll(&mut self, binding: &Binding, password: &str) -> Result<Metadata, String> {
        if !valid_binding(binding) { return Err(INVALID.into()); }
        if !valid_password(password) { return Err("Use 4–64 printable ASCII characters for password unlock.".into()); }
        let generation = self.vault.generation()?;
        if !valid_generation(&generation) { return Err(INVALID.into()); }
        let mut metadata = Metadata { binding: binding.clone(), generation: generation.clone(), enabled: false, blocked: true, invalidated: false };
        let record = PasswordRecord { schema: SCHEMA, method: METHOD.into(), binding: binding.clone(), generation, password: password.into() };
        let bytes = encode(&record)?;
        // Invalidate the previous generation before replacing its secret. An
        // interrupted enrollment leaves disabled, blocked state, never permission
        // to replay either the previous password or an incompletely written one.
        self.write_metadata(&metadata)?;
        self.vault.write(&target(binding, "secret")?, &bytes)?;
        metadata.blocked = false;
        self.write_metadata(&metadata)?;
        Ok(metadata)
    }

    fn set_enabled(&mut self, binding: &Binding, enabled: bool) -> Result<Metadata, String> {
        let mut metadata = self.metadata(binding)?.ok_or(INVALID)?;
        if enabled && metadata.invalidated { return Err(METHOD_CHANGED.into()); }
        // An explicit enable action checks secret integrity. Ordinary status and
        // already-unlocked monitoring read metadata only.
        if enabled { drop(self.read_password(&metadata)?); }
        metadata.enabled = enabled;
        self.write_metadata(&metadata)?;
        Ok(metadata)
    }

    fn set_blocked(&mut self, binding: &Binding, blocked: bool) -> Result<Metadata, String> {
        let mut metadata = self.metadata(binding)?.ok_or(INVALID)?;
        if metadata.invalidated { return Err(METHOD_CHANGED.into()); }
        if blocked && metadata.blocked { return Err("Automatic unlock is waiting for a verified manual unlock.".into()); }
        metadata.blocked = blocked;
        self.write_metadata(&metadata)?;
        Ok(metadata)
    }

    fn invalidate(&mut self, binding: &Binding) -> Result<Metadata, String> {
        let mut metadata = self.metadata(binding)?.ok_or(INVALID)?;
        metadata.enabled = false;
        metadata.blocked = true;
        metadata.invalidated = true;
        self.write_metadata(&metadata)?;
        Ok(metadata)
    }

    fn read_password(&mut self, metadata: &Metadata) -> Result<Secret, String> {
        if metadata.invalidated { return Err(METHOD_CHANGED.into()); }
        let bytes = self.vault.read(&target(&metadata.binding, "secret")?)?.ok_or(INVALID)?;
        if bytes.len() > MAX_RECORD { return Err(INVALID.into()); }
        let mut record: PasswordRecord = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        if record.schema != SCHEMA || record.method != METHOD || record.binding != metadata.binding
            || record.generation != metadata.generation || !valid_password(&record.password) { return Err(INVALID.into()); }
        Ok(Secret { value: std::mem::take(&mut record.password) })
    }

    fn password(&mut self, metadata: &Metadata) -> Result<Secret, String> {
        let current = self.metadata(&metadata.binding)?.ok_or(INVALID)?;
        if current.generation != metadata.generation || !current.enabled || !current.blocked
            || !metadata.enabled || !metadata.blocked || current.invalidated || metadata.invalidated { return Err(INVALID.into()); }
        self.read_password(&current)
    }

    fn remove(&mut self, binding: &Binding) -> Result<(), String> {
        let generation = self.vault.generation()?;
        if !valid_generation(&generation) { return Err(INVALID.into()); }
        // Keep a disabled tombstone until secret deletion succeeds. Failure at
        // any point cannot leave an enabled record behind.
        self.write_metadata(&Metadata { binding: binding.clone(), generation, enabled: false, blocked: true, invalidated: true })?;
        self.vault.remove(&target(binding, "secret")?)?;
        self.vault.remove(&target(binding, "state")?)
    }
}

struct NativeVault;

impl Vault for NativeVault {
    fn read(&mut self, target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        #[cfg(windows)]
        { native::read(target) }
        #[cfg(not(windows))]
        { let _ = target; Err(UNSUPPORTED.into()) }
    }

    fn write(&mut self, target: &str, bytes: &[u8]) -> Result<(), String> {
        #[cfg(windows)]
        { native::write(target, bytes) }
        #[cfg(not(windows))]
        { let _ = (target, bytes); Err(UNSUPPORTED.into()) }
    }

    fn remove(&mut self, target: &str) -> Result<(), String> {
        #[cfg(windows)]
        { native::remove(target) }
        #[cfg(not(windows))]
        { let _ = target; Err(UNSUPPORTED.into()) }
    }

    fn generation(&mut self) -> Result<String, String> {
        #[cfg(windows)]
        { native::generation() }
        #[cfg(not(windows))]
        { Err(UNSUPPORTED.into()) }
    }
}

// The Windows mutex is owned by its acquiring thread. Hold this lease on one
// blocking worker for the complete attempt, including identity/privacy checks,
// persisted attempt state, credential delivery, and verified completion.
pub struct AttemptLock {
    #[cfg(windows)]
    _gate: native::Gate,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn attempt_lock(binding: &Binding) -> Result<AttemptLock, String> {
    if !valid_binding(binding) { return Err(INVALID.into()); }
    #[cfg(windows)]
    { Ok(AttemptLock { _gate: native::Gate::attempt(binding)?, _thread: std::marker::PhantomData }) }
    #[cfg(not(windows))]
    { Err(UNSUPPORTED.into()) }
}

fn transaction<T>(binding: Option<&Binding>, operation: impl FnOnce(&mut Store<NativeVault>) -> Result<T, String>) -> Result<T, String> {
    if !supported() { return Err(UNSUPPORTED.into()); }
    // Mutations take the device lease before the vault transaction lock.
    // Acquiring on its owning replay thread is reentrant; other writers fail
    // immediately instead of changing enrollment while a password is in flight.
    let _attempt = binding.map(attempt_lock).transpose()?;
    let _guard = STORE.lock().map_err(|_| "Unlock storage is unavailable until SCRCPY+ is restarted.")?;
    #[cfg(windows)]
    let _cross_process = native::Gate::acquire()?;
    operation(&mut Store { vault: NativeVault })
}

pub fn supported() -> bool { cfg!(windows) }
pub fn metadata(binding: &Binding) -> Result<Option<Metadata>, String> { transaction(None, |store| store.metadata(binding)) }
pub fn enroll(binding: &Binding, password: &str) -> Result<Metadata, String> { transaction(Some(binding), |store| store.enroll(binding, password)) }
pub fn set_enabled(binding: &Binding, enabled: bool) -> Result<Metadata, String> { transaction(Some(binding), |store| store.set_enabled(binding, enabled)) }
pub fn set_blocked(binding: &Binding, blocked: bool) -> Result<Metadata, String> { transaction(Some(binding), |store| store.set_blocked(binding, blocked)) }
pub fn invalidate(binding: &Binding) -> Result<Metadata, String> { transaction(Some(binding), |store| store.invalidate(binding)) }
pub fn password(metadata: &Metadata) -> Result<Secret, String> { transaction(Some(&metadata.binding), |store| store.password(metadata)) }
pub fn remove(binding: &Binding) -> Result<(), String> { transaction(Some(binding), |store| store.remove(binding)) }

#[cfg(windows)]
mod native {
    use super::*;
    use windows_sys::Win32::Foundation::{ CloseHandle, GetLastError, ERROR_NOT_FOUND, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0 };
    use windows_sys::Win32::Security::{ GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER };
    use windows_sys::Win32::Security::Credentials::{ CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC };
    use windows_sys::Win32::Security::Cryptography::{ BCryptCloseAlgorithmProvider, BCryptGenRandom, BCryptHash, BCryptOpenAlgorithmProvider, BCRYPT_SHA256_ALGORITHM, BCRYPT_USE_SYSTEM_PREFERRED_RNG };
    use windows_sys::Win32::System::Threading::{ CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex, WaitForSingleObject };

    const VAULT_ERROR: &str = "Windows protected unlock storage is unavailable. No password was sent.";
    struct Handle(HANDLE);
    impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

    pub struct Gate(Handle);

    impl Gate {
        pub fn acquire() -> Result<Self, String> {
            Self::named("vault", 3000)
        }

        pub fn attempt(binding: &Binding) -> Result<Self, String> {
            let bytes = serde_json::to_vec(binding).map_err(|_| INVALID)?;
            let mut digest = [0; 32];
            unsafe {
                let mut algorithm = std::ptr::null_mut();
                if BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, std::ptr::null(), 0) < 0 { return Err(VAULT_ERROR.into()); }
                let status = BCryptHash(algorithm, std::ptr::null(), 0, bytes.as_ptr(), bytes.len() as u32, digest.as_mut_ptr(), digest.len() as u32);
                BCryptCloseAlgorithmProvider(algorithm, 0);
                if status < 0 { return Err(VAULT_ERROR.into()); }
            }
            Self::named(&format!("attempt-{}", hex(&digest)), 0)
        }

        fn named(purpose: &str, timeout: u32) -> Result<Self, String> {
            unsafe {
                let mut token = std::ptr::null_mut();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return Err(VAULT_ERROR.into()); }
                let token = Handle(token);
                let mut size = 0;
                GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut size);
                if size == 0 || size > 4096 { return Err(VAULT_ERROR.into()); }
                let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
                if GetTokenInformation(token.0, TokenUser, buffer.as_mut_ptr().cast(), size, &mut size) == 0 { return Err(VAULT_ERROR.into()); }
                let sid = (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid;
                let size = GetLengthSid(sid);
                if size == 0 || size > 256 { return Err(VAULT_ERROR.into()); }
                let sid = std::slice::from_raw_parts(sid.cast::<u8>(), size as usize);
                let name: Vec<u16> = format!("Global\\SCRCPYPlusUnlock-v1-{}-{}", hex(sid), purpose).encode_utf16().chain(Some(0)).collect();
                let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
                if handle.is_null() { return Err(VAULT_ERROR.into()); }
                let handle = Handle(handle);
                if ![WAIT_OBJECT_0, WAIT_ABANDONED].contains(&WaitForSingleObject(handle.0, timeout)) { return Err("Unlock storage is busy. Try again after the other operation finishes.".into()); }
                Ok(Self(handle))
            }
        }
    }

    impl Drop for Gate { fn drop(&mut self) { unsafe { ReleaseMutex(self.0.0); } } }

    pub fn read(target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let mut credential = std::ptr::null_mut();
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
                return if GetLastError() == ERROR_NOT_FOUND { Ok(None) } else { Err(VAULT_ERROR.into()) };
            }
            if credential.is_null() { return Err(VAULT_ERROR.into()); }
            let record = &mut *credential;
            let result = if record.Type != CRED_TYPE_GENERIC || record.CredentialBlobSize == 0
                || record.CredentialBlobSize as usize > MAX_RECORD || record.CredentialBlob.is_null() {
                Err(INVALID.into())
            } else {
                let bytes = std::slice::from_raw_parts_mut(record.CredentialBlob, record.CredentialBlobSize as usize);
                let result = Zeroizing::new(bytes.to_vec());
                bytes.zeroize();
                Ok(Some(result))
            };
            CredFree(credential.cast());
            result
        }
    }

    pub fn write(target: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > MAX_RECORD { return Err(INVALID.into()); }
        let mut target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let mut username: Vec<u16> = "SCRCPY+ automatic unlock".encode_utf16().chain(Some(0)).collect();
        let mut bytes = Zeroizing::new(bytes.to_vec());
        let record = CREDENTIALW {
            Type: CRED_TYPE_GENERIC, TargetName: target.as_mut_ptr(), CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_mut_ptr(), Persist: CRED_PERSIST_LOCAL_MACHINE, UserName: username.as_mut_ptr(),
            ..Default::default()
        };
        if unsafe { CredWriteW(&record, 0) } == 0 { return Err(VAULT_ERROR.into()); }
        Ok(())
    }

    pub fn remove(target: &str) -> Result<(), String> {
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
            && unsafe { GetLastError() } != ERROR_NOT_FOUND { return Err(VAULT_ERROR.into()); }
        Ok(())
    }

    pub fn generation() -> Result<String, String> {
        let mut bytes = [0; 16];
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) } < 0 { return Err(VAULT_ERROR.into()); }
        Ok(hex(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeVault { records: HashMap<String, Vec<u8>>, reads: Vec<String>, writes: usize, fail_write: Option<usize>, fail_remove: bool, generation: u128 }

    impl Vault for FakeVault {
        fn read(&mut self, target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
            self.reads.push(target.into());
            Ok(self.records.get(target).cloned().map(Zeroizing::new))
        }
        fn write(&mut self, target: &str, bytes: &[u8]) -> Result<(), String> {
            self.writes += 1;
            if self.fail_write == Some(self.writes) { return Err("synthetic write failure".into()); }
            self.records.insert(target.into(), bytes.to_vec());
            Ok(())
        }
        fn remove(&mut self, target: &str) -> Result<(), String> {
            if self.fail_remove { return Err("synthetic deletion failure".into()); }
            self.records.remove(target);
            Ok(())
        }
        fn generation(&mut self) -> Result<String, String> { self.generation += 1; Ok(format!("{:032x}", self.generation)) }
    }

    fn binding() -> Binding { Binding { identity: "[\"synthetic-serial\",\"synthetic-vendor\",\"synthetic-product\"]".into(), user: 0 } }
    fn store() -> Store<FakeVault> { Store { vault: FakeVault::default() } }

    #[cfg(windows)]
    #[test]
    #[ignore = "Opt-in native vault round trip using a unique synthetic binding; never reads real device records and removes its own test records"]
    fn native_synthetic_record_round_trip_and_cleanup() {
        assert_eq!(std::env::var("SCRCPY_PLUS_TEST_VAULT").as_deref(), Ok("1"));
        let binding = Binding { identity: format!("synthetic-native-vault-test-{}-{}", std::process::id(), native::generation().unwrap()), user: 0 };
        assert!(metadata(&binding).unwrap().is_none());
        struct Cleanup(Binding);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = remove(&self.0); } }
        let _cleanup = Cleanup(binding.clone());
        let enrolled = enroll(&binding, "Synthetic vault test only").unwrap();
        assert!(!enrolled.enabled && !enrolled.blocked && !enrolled.invalidated);
        let enabled = set_enabled(&binding, true).unwrap();
        assert!(enabled.enabled);
        assert!(password(&enabled).is_err());
        let attempt = set_blocked(&binding, true).unwrap();
        let secret = password(&attempt).unwrap();
        assert_eq!(secret.value, "Synthetic vault test only");
        drop(secret);
        set_blocked(&binding, false).unwrap();
        invalidate(&binding).unwrap();
        assert!(set_enabled(&binding, true).is_err());
        remove(&binding).unwrap();
        assert!(metadata(&binding).unwrap().is_none());
        println!("Native Windows vault round trip, attempt block, invalidation, and synthetic-record cleanup verified.");
    }
    fn enroll_enabled(store: &mut Store<FakeVault>, binding: &Binding) -> Metadata {
        store.enroll(binding, "synthetic password").unwrap();
        store.set_enabled(binding, true).unwrap()
    }

    #[test]
    fn metadata_and_already_unlocked_status_never_read_the_secret() {
        let mut store = store();
        let binding = binding();
        let metadata = store.enroll(&binding, "synthetic password").unwrap();
        assert!(!metadata.enabled && !metadata.blocked);
        store.vault.reads.clear();
        assert!(store.metadata(&binding).unwrap().is_some());
        assert!(store.vault.reads.iter().all(|target| target.ends_with("/state")));
    }

    #[test]
    fn password_requires_enabled_current_generation_and_a_persisted_attempt() {
        let mut store = store();
        let binding = binding();
        let metadata = enroll_enabled(&mut store, &binding);
        assert!(store.password(&metadata).is_err());
        let attempt = store.set_blocked(&binding, true).unwrap();
        assert!(store.set_blocked(&binding, true).is_err());
        assert_eq!(store.password(&attempt).unwrap().value, "synthetic password");
        store.set_enabled(&binding, false).unwrap();
        assert!(store.password(&attempt).is_err());
        assert!(store.metadata(&binding).unwrap().unwrap().blocked);
    }

    #[test]
    fn replacement_rejects_stale_metadata_and_requires_explicit_enable() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        let old = store.set_blocked(&binding, true).unwrap();
        let replacement = store.enroll(&binding, "replacement password").unwrap();
        assert!(!replacement.enabled && !replacement.blocked);
        assert_ne!(old.generation, replacement.generation);
        assert!(store.password(&old).is_err());
    }

    #[test]
    fn interrupted_enrollment_and_failed_attempt_write_never_authorize_input() {
        for failing_write in [1, 2, 3] {
            let mut store = store();
            let binding = binding();
            store.vault.fail_write = Some(failing_write);
            assert!(store.enroll(&binding, "synthetic password").is_err());
            if let Some(state) = store.metadata(&binding).unwrap() { assert!(!state.enabled && state.blocked); }
        }
        let mut store = store();
        let binding = binding();
        let metadata = enroll_enabled(&mut store, &binding);
        store.vault.fail_write = Some(store.vault.writes + 1);
        assert!(store.set_blocked(&binding, true).is_err());
        assert!(store.password(&metadata).is_err());
    }

    #[test]
    fn interrupted_replacement_never_enables_the_new_generation() {
        for step in [1, 2, 3] {
            let mut store = store();
            let binding = binding();
            let old = enroll_enabled(&mut store, &binding);
            store.vault.fail_write = Some(store.vault.writes + step);
            assert!(store.enroll(&binding, "replacement synthetic password").is_err());
            let current = store.metadata(&binding).unwrap().unwrap();
            if step == 1 {
                assert_eq!(current.generation, old.generation);
                assert!(current.enabled && !current.blocked);
            } else {
                assert_ne!(current.generation, old.generation);
                assert!(!current.enabled && current.blocked);
            }
            assert!(store.password(&old).is_err());
        }
    }

    #[test]
    fn recovery_write_failure_preserves_the_persistent_block() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        store.set_blocked(&binding, true).unwrap();
        store.vault.fail_write = Some(store.vault.writes + 1);
        assert!(store.set_blocked(&binding, false).is_err());
        assert!(store.metadata(&binding).unwrap().unwrap().blocked);
        assert!(store.set_blocked(&binding, true).is_err());
        store.set_enabled(&binding, true).unwrap();
        assert!(store.metadata(&binding).unwrap().unwrap().blocked);
    }

    #[test]
    fn changed_method_cannot_reenable_the_old_password_after_restart_or_manual_recovery() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        let old = store.set_blocked(&binding, true).unwrap();
        let secret_key = target(&binding, "secret").unwrap();
        let secret_before = store.vault.records[&secret_key].clone();
        let invalidated = store.invalidate(&binding).unwrap();
        assert!(!invalidated.enabled && invalidated.blocked && invalidated.invalidated);
        assert_eq!(store.vault.records[&secret_key], secret_before);
        let mut reopened = Store { vault: store.vault };
        reopened.vault.reads.clear();
        assert!(reopened.metadata(&binding).unwrap().unwrap().invalidated);
        assert!(reopened.set_enabled(&binding, true).is_err());
        assert!(reopened.set_blocked(&binding, false).is_err());
        assert!(reopened.password(&old).is_err());
        assert!(reopened.vault.reads.iter().all(|target| target.ends_with("/state")));
        let disabled = reopened.set_enabled(&binding, false).unwrap();
        assert!(!disabled.enabled && disabled.blocked && disabled.invalidated);
        let reenrolled = reopened.enroll(&binding, "new synthetic password").unwrap();
        assert!(!reenrolled.enabled && !reenrolled.blocked && !reenrolled.invalidated);
        assert_ne!(reenrolled.generation, old.generation);
        reopened.set_enabled(&binding, true).unwrap();
        let attempt = reopened.set_blocked(&binding, true).unwrap();
        assert_eq!(reopened.password(&attempt).unwrap().value, "new synthetic password");
        assert!(reopened.password(&old).is_err());
    }

    #[test]
    fn missing_invalidation_state_or_conflicting_invalidated_flags_fail_closed() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        let state_key = target(&binding, "state").unwrap();
        let original: serde_json::Value = serde_json::from_slice(&store.vault.records[&state_key]).unwrap();
        let mut missing = original.clone();
        missing["metadata"].as_object_mut().unwrap().remove("invalidated");
        store.vault.records.insert(state_key.clone(), serde_json::to_vec(&missing).unwrap());
        assert!(store.metadata(&binding).is_err());
        let mut conflicting = original;
        conflicting["metadata"]["invalidated"] = serde_json::json!(true);
        store.vault.records.insert(state_key, serde_json::to_vec(&conflicting).unwrap());
        assert!(store.metadata(&binding).is_err());
    }

    #[test]
    fn swapped_secret_metadata_and_other_android_user_are_rejected() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        let attempt = store.set_blocked(&binding, true).unwrap();
        let other = Binding { identity: "other synthetic device".into(), user: 10 };
        store.enroll(&other, "other synthetic password").unwrap();
        let other_secret = store.vault.records[&target(&other, "secret").unwrap()].clone();
        store.vault.records.insert(target(&binding, "secret").unwrap(), other_secret);
        assert!(store.password(&attempt).is_err());
        let other_state = store.vault.records[&target(&other, "state").unwrap()].clone();
        store.vault.records.insert(target(&binding, "state").unwrap(), other_state);
        assert!(store.metadata(&binding).is_err());
        assert!(store.metadata(&Binding { identity: binding.identity, user: 10 }).unwrap().is_none());
    }

    #[test]
    fn missing_corrupt_changed_method_and_unknown_fields_fail_closed() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        let attempt = store.set_blocked(&binding, true).unwrap();
        let state_key = target(&binding, "state").unwrap();
        let secret_key = target(&binding, "secret").unwrap();
        let good_state = store.vault.records[&state_key].clone();
        let good_secret = store.vault.records[&secret_key].clone();
        for bytes in [b"invalid".to_vec(), vec![b'x'; MAX_RECORD + 1], br#"{"schema":1,"schema":1}"#.to_vec()] {
            store.vault.records.insert(state_key.clone(), bytes);
            assert!(store.password(&attempt).is_err());
        }
        store.vault.records.insert(state_key.clone(), good_state.clone());
        for field in ["method", "schema", "extra"] {
            let mut value: serde_json::Value = serde_json::from_slice(&good_secret).unwrap();
            value[field] = serde_json::json!("invalid");
            store.vault.records.insert(secret_key.clone(), serde_json::to_vec(&value).unwrap());
            assert!(store.password(&attempt).is_err());
        }
        store.vault.records.remove(&secret_key);
        assert!(store.password(&attempt).is_err());
        store.vault.records.remove(&state_key);
        assert!(store.password(&attempt).is_err());
    }

    #[test]
    fn deleting_a_record_is_fail_closed_and_explicit_reenrollment_recovers_it() {
        let mut store = store();
        let binding = binding();
        enroll_enabled(&mut store, &binding);
        store.vault.fail_remove = true;
        assert!(store.remove(&binding).is_err());
        let state = store.metadata(&binding).unwrap().unwrap();
        assert!(!state.enabled && state.blocked);
        assert!(store.set_enabled(&binding, true).is_err());
        store.vault.fail_remove = false;
        store.remove(&binding).unwrap();
        assert!(store.metadata(&binding).unwrap().is_none());
        assert!(store.enroll(&binding, "new synthetic password").is_ok());
    }

    #[test]
    fn invalid_binding_and_password_never_touch_the_vault() {
        let mut store = store();
        for password in ["123", "", "synthetic\npassword", "synthétique", &"x".repeat(65)] { assert!(store.enroll(&binding(), password).is_err()); }
        for identity in [String::new(), "device\nname".into(), "x".repeat(1025)] {
            assert!(store.enroll(&Binding { identity, user: 0 }, "synthetic password").is_err());
        }
        assert_eq!(store.vault.writes, 0);
        assert!(store.vault.reads.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn attempt_lease_is_reentrant_and_rejects_other_threads_without_reading_the_vault() {
        let binding = binding();
        let first = attempt_lock(&binding).unwrap();
        let nested = attempt_lock(&binding).unwrap();
        let other_binding = binding.clone();
        assert!(std::thread::spawn(move || attempt_lock(&other_binding).is_err()).join().unwrap());
        let different = Binding { identity: "different synthetic lease target".into(), user: 0 };
        assert!(std::thread::spawn(move || attempt_lock(&different).is_ok()).join().unwrap());
        drop(nested);
        drop(first);
        assert!(std::thread::spawn(move || attempt_lock(&binding).is_ok()).join().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn generation_uses_native_randomness_without_reading_the_vault() {
        let first = native::generation().unwrap();
        let second = native::generation().unwrap();
        assert!(valid_generation(&first) && valid_generation(&second));
        assert_ne!(first, second);
    }
}
