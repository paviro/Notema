//! This device's key in the OS keychain.
//!
//! The account is a random label stored in `identity.toml`; the service is
//! constant. Nothing here is reachable on every platform, so every entry point
//! has a fallback that says so and points at `keys_command`.

use crate::{EncryptionError, Result};
use zeroize::Zeroizing;

pub(crate) fn fetch(account: &str) -> Result<Zeroizing<String>> {
    let _ = account;
    Err(unavailable())
}

pub(crate) fn store(account: &str, material: &str) -> Result<()> {
    let (_, _) = (account, material);
    Err(unavailable())
}

/// Remove this device's item. Best effort: a keychain we can't reach is not a
/// reason to fail the operation that is retiring the key.
pub(crate) fn delete(account: &str) {
    let _ = account;
}

fn unavailable() -> EncryptionError {
    EncryptionError::KeyringUnavailable {
        detail: "this build has no OS keychain support; use keys_command instead".to_string(),
    }
}
