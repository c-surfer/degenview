//! Surfer license verification and hardware ID (HWID) generator.
//!
//! Handles license key storage in the system keychain/secure backend,
//! stable device fingerprinting, and verification via the Google Apps Script Web App.

#![allow(dead_code)]

pub mod crypto;
#[cfg(test)]
mod integration_tests;

use crate::lock::lock;
use crate::service::keychain::api_keys::{KeyBackend, default_backend};
use crate::service::keychain::webhook_secrets::Scope;
use crate::service::webhook::delivery::SecretStoreError;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock, RwLock};

const SURFER_SERVICE: &str = "es.nico.degenview.surfer";
const SURFER_ACCOUNT: &str = "license-v1";

const SURFER_SCOPE: Scope = Scope {
    service: SURFER_SERVICE,
    shared_group: false,
};

/// Default Google Apps Script Web App URL for Surfer license verification.
pub const DEFAULT_GAS_URL: &str = "https://script.google.com/macros/s/AKfycbyM7uY4v5wCVLKUIXP5aRbZ2g4J-RKNV7eeyldHTkm6BkcgLTMMKfSygjvPsqpcbiPAzw/exec";

/// Environment variable overriding the Google Apps Script Web App endpoint URL.
pub const SURFER_URL_ENV: &str = "DEGENVIEW_SURFER_URL";

/// Environment variable overriding the Surfer license key.
pub const SURFER_LICENSE_ENV: &str = "DEGENVIEW_SURFER_LICENSE";

/// Environment variable overriding the fallback license file path.
pub const SURFER_FILE_ENV: &str = "DEGENVIEW_SURFER_FILE";

/// Environment variable overriding the hardware ID (useful for tests and automation).
pub const SURFER_HWID_ENV: &str = "DEGENVIEW_SURFER_HWID";

// MARK: Hardware ID (HWID)

static HWID_CACHE: OnceLock<String> = OnceLock::new();

/// Generates or retrieves a stable, cached hardware ID (HWID) for this machine.
///
/// Checks in order:
/// 1. `DEGENVIEW_SURFER_HWID` environment variable (if non-empty).
/// 2. Cached persistent ID file in `data_dir()/machine_id`.
/// 3. OS-level machine ID (`/etc/machine-id` or `/var/lib/dbus/machine-id` on Linux).
/// 4. Generated device ID fallback saved persistently to `data_dir()/machine_id`.
pub fn hwid() -> String {
    HWID_CACHE.get_or_init(resolve_hwid).clone()
}

fn resolve_hwid() -> String {
    if let Ok(env_hwid) = std::env::var(SURFER_HWID_ENV) {
        let trimmed = env_hwid.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }

    let hwid_file = crate::paths::data_dir().map(|dir| dir.join("machine_id"));

    if let Some(path) = &hwid_file
        && let Ok(content) = std::fs::read_to_string(path)
    {
        let trimmed = content.trim();
        if trimmed.len() >= 16 {
            return trimmed.to_owned();
        }
    }

    let raw_id = read_system_machine_id().unwrap_or_else(crate::store::sync::new_device_id);

    let final_hwid = crate::model::encoding::sha256_hex(raw_id.trim().as_bytes());

    if let Some(path) = &hwid_file {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, final_hwid.as_bytes());
    }

    final_hwid
}

fn read_system_machine_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        for path in &["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(id) = std::fs::read_to_string(path) {
                let trimmed = id.trim();
                if !trimmed.is_empty() && trimmed != "uninitialized" {
                    return Some(trimmed.to_owned());
                }
            }
        }
    }
    None
}

// MARK: Surfer License State

/// The state of a Surfer license verification response.
#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurferLicenseState {
    pub success: bool,
    pub decrypt_key: Option<String>,
    pub end_date: Option<String>,
    pub nickname: Option<String>,
    pub user_id: Option<String>,
    pub error: Option<String>,
}

impl std::fmt::Debug for SurferLicenseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let masked_key = self.decrypt_key.as_ref().map(|_| "[REDACTED]");
        f.debug_struct("SurferLicenseState")
            .field("success", &self.success)
            .field("decrypt_key", &masked_key)
            .field("end_date", &self.end_date)
            .field("nickname", &self.nickname)
            .field("user_id", &self.user_id)
            .field("error", &self.error)
            .finish()
    }
}

static CURRENT_LICENSE_STATE: RwLock<Option<SurferLicenseState>> = RwLock::new(None);

#[cfg(test)]
pub(crate) static TEST_LICENSE_LOCK: Mutex<()> = Mutex::new(());

/// Returns whether Surfer Pro features are active (valid decryption key is present).
pub fn is_pro_active() -> bool {
    get_decrypt_key().is_some()
}

/// Returns the active decryption key if verified and present.
pub fn get_decrypt_key() -> Option<String> {
    let guard = crate::lock::read(&CURRENT_LICENSE_STATE);
    guard
        .as_ref()
        .filter(|state| state.success)
        .and_then(|state| state.decrypt_key.as_ref())
        .filter(|key| !key.trim().is_empty())
        .cloned()
}

/// Returns a clone of the currently cached license state.
pub fn current_license_state() -> Option<SurferLicenseState> {
    crate::lock::read(&CURRENT_LICENSE_STATE).clone()
}

/// Sets the global cached license state (used by verification and testing).
pub fn set_cached_license_state(state: Option<SurferLicenseState>) {
    *crate::lock::write(&CURRENT_LICENSE_STATE) = state;
}

// MARK: Surfer Key Store

pub fn fallback_license_path() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var(SURFER_FILE_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(std::path::PathBuf::from(trimmed));
        }
    }
    if let Some(dir) = crate::paths::data_dir() {
        return Some(dir.join("surfer_license.dat"));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".config/degenview/surfer.key"))
}

pub fn read_fallback_license() -> Option<String> {
    let path = fallback_license_path()?;
    read_fallback_license_at(&path)
}

pub fn write_fallback_license(license: &str) {
    if let Some(path) = fallback_license_path() {
        write_fallback_license_at(&path, license);
    }
}

pub fn delete_fallback_license() {
    if let Some(path) = fallback_license_path() {
        delete_fallback_license_at(&path);
    }
}

fn read_fallback_license_at(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn write_fallback_license_at(path: &std::path::Path, license: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(path, license.trim().as_bytes()).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

fn delete_fallback_license_at(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

#[derive(Default)]
struct SurferCache {
    loaded: bool,
    license: Option<String>,
    exists: Option<bool>,
}

/// Secure storage for the Surfer license key, mirroring CoinMarketCapKeyStore,
/// with dual storage (KeychainBackend + local file fallback) for 100% Flatpak persistence.
pub struct SurferKeyStore {
    backend: Box<dyn KeyBackend>,
    fallback_path: Option<std::path::PathBuf>,
    cache: Mutex<SurferCache>,
}

impl SurferKeyStore {
    pub fn with_backend(backend: Box<dyn KeyBackend>) -> Self {
        Self::with_backend_and_path(backend, fallback_license_path())
    }

    pub fn with_backend_and_path(
        backend: Box<dyn KeyBackend>,
        fallback_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            backend,
            fallback_path,
            cache: Mutex::new(SurferCache::default()),
        }
    }

    /// The application-wide shared key store.
    pub fn shared() -> &'static Self {
        static STORE: OnceLock<SurferKeyStore> = OnceLock::new();
        STORE.get_or_init(|| Self::with_backend(default_backend()))
    }

    fn read_fallback(&self) -> Option<String> {
        let path = self.fallback_path.as_deref()?;
        read_fallback_license_at(path)
    }

    fn write_fallback(&self, license: &str) {
        if let Some(path) = self.fallback_path.as_deref() {
            write_fallback_license_at(path, license);
        }
    }

    fn delete_fallback(&self) {
        if let Some(path) = self.fallback_path.as_deref() {
            delete_fallback_license_at(path);
        }
    }

    /// The saved license, or None. The first read may prompt for Keychain access.
    pub fn license(&self) -> Option<String> {
        let mut cache = lock(&self.cache);
        if !cache.loaded {
            cache.loaded = true;
            let mut key = self
                .backend
                .read(SURFER_SCOPE, SURFER_ACCOUNT)
                .filter(|k| !k.is_empty());
            if key.is_none() {
                key = self.read_fallback();
                if let Some(ref fallback_key) = key {
                    let _ = self
                        .backend
                        .write(SURFER_SCOPE, SURFER_ACCOUNT, fallback_key);
                }
            }
            cache.license = key;
        }
        cache.license.clone()
    }

    /// Whether a license is saved, without reading it (no Keychain prompt).
    pub fn is_configured(&self) -> bool {
        let mut cache = lock(&self.cache);
        if cache.loaded {
            return cache.license.is_some();
        }
        if let Some(exists) = cache.exists {
            return exists;
        }
        let mut found = self.backend.exists(SURFER_SCOPE, SURFER_ACCOUNT);
        if !found {
            found = self.read_fallback().is_some();
        }
        cache.exists = Some(found);
        found
    }

    /// Saves the trimmed license; an empty one removes it.
    pub fn save(&self, license: &str) -> Result<(), SecretStoreError> {
        let value = license.trim();
        if value.is_empty() {
            set_cached_license_state(None);
            return self.remove();
        }
        self.write_fallback(value);
        let backend_res = self.backend.write(SURFER_SCOPE, SURFER_ACCOUNT, value);
        let mut cache = lock(&self.cache);
        *cache = SurferCache {
            loaded: true,
            license: Some(value.to_owned()),
            exists: Some(true),
        };
        if backend_res.is_err() {
            Ok(())
        } else {
            backend_res
        }
    }

    /// Removes the license from the store.
    pub fn remove(&self) -> Result<(), SecretStoreError> {
        self.delete_fallback();
        let _ = self.backend.delete(SURFER_SCOPE, SURFER_ACCOUNT);
        let mut cache = lock(&self.cache);
        *cache = SurferCache {
            loaded: true,
            license: None,
            exists: Some(false),
        };
        drop(cache);
        set_cached_license_state(None);
        Ok(())
    }
}

/// Looks up the Surfer license key, preferring the saved key and falling back to the environment.
pub fn surfer_license() -> Option<String> {
    SurferKeyStore::shared().license().or_else(|| {
        std::env::var(SURFER_LICENSE_ENV)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    })
}

// MARK: Verification & Network

/// Returns the effective Google Apps Script Web App URL.
pub fn gas_url() -> String {
    std::env::var(SURFER_URL_ENV)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_GAS_URL.to_owned())
}

/// Percent-encodes characters for query string values.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Parses the JSON response received from the Google Apps Script Web App.
pub fn parse_license_response(json: &serde_json::Value) -> Result<SurferLicenseState, String> {
    let obj = json
        .as_object()
        .ok_or_else(|| "Response is not a JSON object".to_string())?;

    let success = obj
        .get("success")
        .and_then(|v| v.as_bool())
        .or_else(|| {
            obj.get("success").and_then(|v| match v {
                serde_json::Value::String(s) => s.parse::<bool>().ok(),
                serde_json::Value::Number(n) => n.as_i64().map(|i| i == 1),
                _ => None,
            })
        })
        .unwrap_or(false);

    let decrypt_key = obj
        .get("decrypt_key")
        .or_else(|| obj.get("decryptKey"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let end_date = obj
        .get("end_date")
        .or_else(|| obj.get("endDate"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let nickname = obj
        .get("nickname")
        .or_else(|| obj.get("userName"))
        .or_else(|| obj.get("username"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let user_id = obj
        .get("user_id")
        .or_else(|| obj.get("userId"))
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.trim().to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty());

    let error = obj
        .get("error")
        .or_else(|| obj.get("message"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(SurferLicenseState {
        success,
        decrypt_key,
        end_date,
        nickname,
        user_id,
        error,
    })
}

/// Tests the Surfer license connection by contacting the Google Apps Script Web App.
///
/// Upon success (valid license with decryption key), caches `SurferLicenseState` in memory.
pub fn test_connection(license: &str) -> Result<SurferLicenseState, String> {
    let license = license.trim();
    if license.is_empty() {
        return Err("License key cannot be empty".to_string());
    }

    let base_url = gas_url();
    if base_url.contains("YOUR_SCRIPT_ID") {
        return Err("Surfer endpoint URL не настроен. Укажите действительный URL Google Apps Script в DEGENVIEW_SURFER_URL.".to_string());
    }

    let current_hwid = hwid();
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let url = format!(
        "{base_url}{separator}license={}&hwid={}",
        percent_encode(license),
        percent_encode(&current_hwid)
    );

    let body = crate::service::net::http::get_text(
        &url,
        &[("Accept", "application/json")],
        crate::service::net::http::Retry::Transient,
    )
    .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse JSON response: {e}"))?;

    let state = parse_license_response(&json)?;

    if state.success && state.decrypt_key.is_some() {
        set_cached_license_state(Some(state.clone()));
    }

    Ok(state)
}

// MARK: Unit Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::keychain::api_keys::MemoryBackend;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TrackingBackend {
        inner: Arc<MemoryBackend>,
        reads: Arc<AtomicU64>,
    }

    impl KeyBackend for TrackingBackend {
        fn read(&self, scope: Scope, account: &str) -> Option<String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.read(scope, account)
        }
        fn exists(&self, scope: Scope, account: &str) -> bool {
            self.inner.exists(scope, account)
        }
        fn write(&self, scope: Scope, account: &str, value: &str) -> Result<(), SecretStoreError> {
            self.inner.write(scope, account, value)
        }
        fn delete(&self, scope: Scope, account: &str) -> Result<(), SecretStoreError> {
            self.inner.delete(scope, account)
        }
    }

    #[test]
    fn hwid_generation_is_stable_and_cached() {
        let first = hwid();
        let second = hwid();
        assert!(!first.is_empty());
        assert_eq!(first, second);
        assert_eq!(first.len(), 64, "HWID should be 64-char sha256 hex");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_license_response_successful() {
        let json_str = r#"{
            "success": true,
            "decrypt_key": "SURFER_SECRET_KEY_123",
            "end_date": "2026-12-31",
            "nickname": "CryptoDegen",
            "user_id": "884422",
            "error": null
        }"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let state = parse_license_response(&value).unwrap();

        assert!(state.success);
        assert_eq!(state.decrypt_key.as_deref(), Some("SURFER_SECRET_KEY_123"));
        assert_eq!(state.end_date.as_deref(), Some("2026-12-31"));
        assert_eq!(state.nickname.as_deref(), Some("CryptoDegen"));
        assert_eq!(state.user_id.as_deref(), Some("884422"));
        assert_eq!(state.error, None);
    }

    #[test]
    fn parse_license_response_with_camel_case_keys() {
        let json_str = r#"{
            "success": true,
            "decryptKey": "CAMEL_KEY_999",
            "endDate": "2027-01-01",
            "userName": "Alice",
            "userId": 12345
        }"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let state = parse_license_response(&value).unwrap();

        assert!(state.success);
        assert_eq!(state.decrypt_key.as_deref(), Some("CAMEL_KEY_999"));
        assert_eq!(state.end_date.as_deref(), Some("2027-01-01"));
        assert_eq!(state.nickname.as_deref(), Some("Alice"));
        assert_eq!(state.user_id.as_deref(), Some("12345"));
        assert_eq!(state.error, None);
    }

    #[test]
    fn parse_license_response_failure() {
        let json_str = r#"{
            "success": false,
            "error": "License expired or HWID mismatch"
        }"#;
        let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let state = parse_license_response(&value).unwrap();

        assert!(!state.success);
        assert_eq!(state.decrypt_key, None);
        assert_eq!(state.end_date, None);
        assert_eq!(state.nickname, None);
        assert_eq!(state.user_id, None);
        assert_eq!(
            state.error.as_deref(),
            Some("License expired or HWID mismatch")
        );
    }

    #[test]
    fn parse_license_response_non_object() {
        let value = serde_json::json!(["not", "an", "object"]);
        assert!(parse_license_response(&value).is_err());
    }

    #[test]
    fn surfer_key_store_saves_trims_and_removes() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        let memory = Arc::new(MemoryBackend::default());
        let reads = Arc::new(AtomicU64::new(0));
        let store = SurferKeyStore::with_backend_and_path(
            Box::new(TrackingBackend {
                inner: memory.clone(),
                reads: reads.clone(),
            }),
            None,
        );

        assert!(!store.is_configured());
        assert_eq!(store.license(), None);

        // Save trimmed license
        store.save("  CS-ALPHA-1234  \n").unwrap();
        assert!(store.is_configured());
        assert_eq!(store.license().as_deref(), Some("CS-ALPHA-1234"));
        assert_eq!(
            memory.read(SURFER_SCOPE, SURFER_ACCOUNT).as_deref(),
            Some("CS-ALPHA-1234")
        );

        // Saving empty string removes the key
        store.save("   ").unwrap();
        assert!(!store.is_configured());
        assert_eq!(store.license(), None);
        assert!(!memory.exists(SURFER_SCOPE, SURFER_ACCOUNT));

        // Save again and test explicit remove()
        store.save("CS-BETA-5678").unwrap();
        assert!(store.is_configured());
        store.remove().unwrap();
        assert!(!store.is_configured());
        assert_eq!(store.license(), None);
        assert!(!memory.exists(SURFER_SCOPE, SURFER_ACCOUNT));
    }

    #[test]
    fn surfer_key_store_answers_is_configured_without_reading_secret() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        let memory = Arc::new(MemoryBackend::default());
        memory
            .write(SURFER_SCOPE, SURFER_ACCOUNT, "CS-STORED-KEY")
            .unwrap();

        let reads = Arc::new(AtomicU64::new(0));
        let store = SurferKeyStore::with_backend_and_path(
            Box::new(TrackingBackend {
                inner: memory,
                reads: reads.clone(),
            }),
            None,
        );

        assert!(store.is_configured(), "Should check existence only");
        assert_eq!(reads.load(Ordering::SeqCst), 0, "No read called yet");

        assert_eq!(store.license().as_deref(), Some("CS-STORED-KEY"));
        assert_eq!(reads.load(Ordering::SeqCst), 1, "Read called once");

        // Subsequent call is served from cache
        assert_eq!(store.license().as_deref(), Some("CS-STORED-KEY"));
        assert_eq!(reads.load(Ordering::SeqCst), 1, "Cached read");
    }

    #[test]
    fn surfer_key_store_dual_storage_fallback_survives_empty_backend() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        static TEST_CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let file_path = std::env::temp_dir().join(format!(
            "dv-surfer-test-{}-{}.dat",
            std::process::id(),
            TEST_CTR.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&file_path);

        let memory = MemoryBackend::default();
        let store =
            SurferKeyStore::with_backend_and_path(Box::new(memory), Some(file_path.clone()));

        // Save
        store.save("CS-DUAL-KEY-999").unwrap();
        assert_eq!(store.license().as_deref(), Some("CS-DUAL-KEY-999"));
        assert!(file_path.exists(), "Fallback file must be created on disk");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&file_path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "License file permissions must be 0600");
        }

        // Simulate keychain wiped / backend empty in Flatpak sandbox restart
        let empty_backend = MemoryBackend::default();
        let store2 =
            SurferKeyStore::with_backend_and_path(Box::new(empty_backend), Some(file_path.clone()));
        assert!(
            store2.is_configured(),
            "Should detect key from fallback file"
        );
        assert_eq!(
            store2.license().as_deref(),
            Some("CS-DUAL-KEY-999"),
            "Should restore key from fallback file"
        );

        // Cleanup
        store2.remove().unwrap();
        assert_eq!(store2.license(), None);
        assert!(
            !file_path.exists(),
            "Fallback file must be cleaned up on remove"
        );
    }

    #[test]
    fn surfer_key_store_env_file_override() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        static TEST_CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
        let file_path = std::env::temp_dir().join(format!(
            "dv-surfer-env-test-{}-{}.dat",
            std::process::id(),
            TEST_CTR.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&file_path);

        unsafe {
            std::env::set_var(SURFER_FILE_ENV, file_path.to_str().unwrap());
        }

        write_fallback_license("CS-ENV-FILE-KEY");
        assert_eq!(read_fallback_license().as_deref(), Some("CS-ENV-FILE-KEY"));
        assert!(file_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&file_path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "License file permissions must be 0600");
        }

        delete_fallback_license();
        assert_eq!(read_fallback_license(), None);
        assert!(!file_path.exists());

        unsafe {
            std::env::remove_var(SURFER_FILE_ENV);
        }
    }

    #[test]
    fn pro_active_state_tracking() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        set_cached_license_state(None);
        assert!(!is_pro_active());
        assert_eq!(get_decrypt_key(), None);

        let active_state = SurferLicenseState {
            success: true,
            decrypt_key: Some("KEY_SEC_1".to_string()),
            end_date: Some("2026-12-31".to_string()),
            nickname: Some("Trader".to_string()),
            user_id: Some("1".to_string()),
            error: None,
        };
        set_cached_license_state(Some(active_state));
        assert!(is_pro_active());
        assert_eq!(get_decrypt_key().as_deref(), Some("KEY_SEC_1"));

        // If success is false, pro is not active even if decrypt_key is somehow set
        let inactive_state = SurferLicenseState {
            success: false,
            decrypt_key: Some("EXPIRED_KEY".to_string()),
            end_date: None,
            nickname: None,
            user_id: None,
            error: Some("Expired".to_string()),
        };
        set_cached_license_state(Some(inactive_state));
        assert!(!is_pro_active());
        assert_eq!(get_decrypt_key(), None);

        set_cached_license_state(None);
        assert!(!is_pro_active());
    }

    #[test]
    fn test_connection_empty_license_fails_early() {
        let err = test_connection("   ").unwrap_err();
        assert!(err.contains("cannot be empty"));
    }

    #[test]
    fn test_connection_detects_placeholder_url() {
        unsafe {
            std::env::set_var(
                SURFER_URL_ENV,
                "https://script.google.com/macros/s/AKfycbwYOUR_SCRIPT_ID/exec",
            );
        }
        let err = test_connection("CS-TEST-LICENSE").unwrap_err();
        unsafe {
            std::env::remove_var(SURFER_URL_ENV);
        }
        assert!(err.contains("Surfer endpoint URL не настроен"));
    }

    #[test]
    fn debug_masks_decrypt_key() {
        let state = SurferLicenseState {
            success: true,
            decrypt_key: Some("SUPER_SECRET_VALUE".to_string()),
            end_date: Some("2026-12-31".to_string()),
            nickname: Some("Trader".to_string()),
            user_id: Some("1".to_string()),
            error: None,
        };
        let debug_str = format!("{state:?}");
        assert!(!debug_str.contains("SUPER_SECRET_VALUE"));
        assert!(debug_str.contains("[REDACTED]"));
    }

    #[test]
    fn key_store_remove_clears_cached_pro_state() {
        let _guard = crate::lock::lock(&TEST_LICENSE_LOCK);
        let memory = Arc::new(MemoryBackend::default());
        let store = SurferKeyStore::with_backend_and_path(
            Box::new(TrackingBackend {
                inner: memory,
                reads: Arc::new(AtomicU64::new(0)),
            }),
            None,
        );
        store.save("CS-VALID").unwrap();
        set_cached_license_state(Some(SurferLicenseState {
            success: true,
            decrypt_key: Some("KEY".into()),
            ..Default::default()
        }));
        assert!(is_pro_active());

        store.remove().unwrap();
        assert!(!is_pro_active());
        assert_eq!(current_license_state(), None);

        // Also test empty save clears state
        set_cached_license_state(Some(SurferLicenseState {
            success: true,
            decrypt_key: Some("KEY".into()),
            ..Default::default()
        }));
        assert!(is_pro_active());
        store.save("   ").unwrap();
        assert!(!is_pro_active());
        assert_eq!(current_license_state(), None);
    }
}
