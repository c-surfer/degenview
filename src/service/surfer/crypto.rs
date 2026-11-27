//! Cryptographic routines for Surfer Pro payload decryption.
//!
//! Provides HMAC-SHA256 CTR keystream decryption of the embedded PRO Pine Script payload.

#![allow(dead_code)]

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Default key returned by the licensing server upon valid subscription.
pub const DEFAULT_PRO_KEY: &str = "SURFER_KEY_777_SECURE_TOKEN";

/// Signature prefix expected at the start of the decrypted PRO payload.
pub const PRO_PAYLOAD_MAGIC: &str = "// [Surfer] PRO Extension";

/// Encrypted binary slice of the PRO Pine Script payload.
pub static ENCRYPTED_PRO_PAYLOAD: &[u8] = include_bytes!("surfer_pro.bin");

/// Encrypts or decrypts bytes using HMAC-SHA256 Keystream CTR mode.
///
/// Because CTR mode is symmetric (keystream is XORed with the data),
/// this single function handles both encryption and decryption.
pub fn crypt_ctr(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    for (counter, chunk) in (0_u64..).zip(data.chunks(32)) {
        let mut mac =
            HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts keys of any size");
        mac.update(&counter.to_be_bytes());
        let keystream = mac.finalize().into_bytes();
        for (&b, &k) in chunk.iter().zip(keystream.iter()) {
            result.push(b ^ k);
        }
    }
    result
}

/// Registers the Surfer PRO extension provider into `model::pine::bundled`.
pub fn init_pro_pine_extension() {
    crate::model::pine::bundled::set_pro_extension_provider(get_pro_pine_extension);
}

/// Helper function to encrypt a plaintext slice with a key.
pub fn encrypt_payload(key: &str, plaintext: &[u8]) -> Vec<u8> {
    crypt_ctr(key.as_bytes(), plaintext)
}

/// Decrypts the embedded PRO payload using `key`.
///
/// Returns the decrypted UTF-8 Pine Script string on success,
/// or `Err` if the key is incorrect or the payload is corrupted.
pub fn decrypt_pro_payload(key: &str) -> Result<String, &'static str> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("Decryption key cannot be empty");
    }

    let decrypted_bytes = crypt_ctr(trimmed.as_bytes(), ENCRYPTED_PRO_PAYLOAD);
    let text = String::from_utf8(decrypted_bytes)
        .map_err(|_| "Decryption failed: invalid UTF-8 (incorrect key)")?;

    if !text.starts_with(PRO_PAYLOAD_MAGIC) {
        return Err("Decryption failed: payload magic signature mismatch (incorrect key)");
    }

    Ok(text)
}

/// Returns the decrypted PRO Pine Script extension if Surfer Pro is active.
pub fn get_pro_pine_extension() -> Option<String> {
    if !crate::service::surfer::is_pro_active() {
        return None;
    }
    let key = crate::service::surfer::get_decrypt_key()?;
    decrypt_pro_payload(&key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypt_ctr_roundtrip() {
        let key = "TEST_SECRET_KEY_12345";
        let message = "indicator('Test Script', overlay=true)\nplot(close)\n";
        let encrypted = encrypt_payload(key, message.as_bytes());
        assert_ne!(encrypted, message.as_bytes());

        let decrypted_bytes = crypt_ctr(key.as_bytes(), &encrypted);
        let decrypted_str = String::from_utf8(decrypted_bytes).unwrap();
        assert_eq!(decrypted_str, message);
    }

    #[test]
    fn test_decrypt_pro_payload_with_valid_key() {
        let result = decrypt_pro_payload(DEFAULT_PRO_KEY);
        assert!(result.is_ok(), "Decryption with valid key should succeed");
        let pro_code = result.unwrap();
        assert!(pro_code.starts_with(PRO_PAYLOAD_MAGIC));
        assert!(pro_code.contains("supersmoother"));
        assert!(pro_code.contains("get_cloud"));
        assert!(pro_code.contains("f_rsi_htf"));
        assert!(pro_code.contains("GetBullishRegularDivergence"));
    }

    #[test]
    fn test_decrypt_pro_payload_with_invalid_key() {
        let result = decrypt_pro_payload("WRONG_INVALID_KEY_999");
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_decrypt_pro_payload_with_empty_key() {
        let result = decrypt_pro_payload("   ");
        assert!(result.is_err(), "Decryption with empty key should fail");
    }

    #[test]
    fn test_get_pro_pine_extension_state_toggle() {
        let _guard = crate::lock::lock(&crate::service::surfer::TEST_LICENSE_LOCK);
        use crate::service::surfer::{SurferLicenseState, set_cached_license_state};

        set_cached_license_state(None);
        assert_eq!(get_pro_pine_extension(), None);

        // Active state with correct key
        set_cached_license_state(Some(SurferLicenseState {
            success: true,
            decrypt_key: Some(DEFAULT_PRO_KEY.to_string()),
            ..Default::default()
        }));
        let ext = get_pro_pine_extension();
        assert!(ext.is_some());
        assert!(ext.unwrap().starts_with(PRO_PAYLOAD_MAGIC));

        // Active state with bad key
        set_cached_license_state(Some(SurferLicenseState {
            success: true,
            decrypt_key: Some("CORRUPT_KEY".to_string()),
            ..Default::default()
        }));
        assert_eq!(get_pro_pine_extension(), None);

        // Reset state
        set_cached_license_state(None);
        assert_eq!(get_pro_pine_extension(), None);
    }

    #[test]
    fn test_combined_free_and_pro_pine_compilation() {
        use crate::pine::compiler;
        use crate::pine::syntax::token::{DiagnosticSeverity, Limits};
        use crate::store::scripts::registry::LibraryRegistry;

        let free_source = include_str!("../../model/pine/bundled/surfer-rsi.pine");
        let pro_code = decrypt_pro_payload(DEFAULT_PRO_KEY).expect("Valid pro decryption");
        let combined = format!("{free_source}\n\n{pro_code}");

        let program = compiler::compile(&combined, Limits::default(), &LibraryRegistry);

        let mut errors = Vec::new();
        for diag in &program.diagnostics {
            if diag.severity == DiagnosticSeverity::Error {
                errors.push(format!("{:?} {}", diag.severity, diag.message));
            }
        }
        assert!(
            errors.is_empty(),
            "Combined Pine script has errors: {errors:#?}"
        );
    }
}
