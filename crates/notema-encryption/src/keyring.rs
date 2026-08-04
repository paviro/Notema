//! This device's key in the OS keychain.
//!
//! The account is an opaque label stored in `identity.toml`; the service is
//! constant. Not every platform has a keychain, and a Linux build that has the
//! backend may still find no D-Bus session behind it, so each entry point falls
//! back to saying so and pointing at `keys_command`.
//!
//! Unlike the command path, the bytes pass through `keyring`'s own buffers on
//! the way here, which we can't zeroize. What we receive is wrapped immediately;
//! whatever the crate did with it before that is out of our hands.

use crate::Result;
use zeroize::Zeroizing;

/// Whether an OS keychain can be reached from here, so a new identity knows
/// whether the keychain is an option.
///
/// Worth asking rather than assuming: a desktop session has one, the same
/// machine over SSH often doesn't.
pub(crate) fn is_available() -> bool {
    platform::is_available()
}

pub(crate) fn fetch(account: &str) -> Result<Zeroizing<String>> {
    platform::fetch(account)
}

pub(crate) fn store(account: &str, material: &str) -> Result<()> {
    platform::store(account, material)
}

/// Forget this device's item. Best effort: a keychain we can't reach is not a
/// reason to fail the operation that is retiring the key.
pub(crate) fn delete(account: &str) {
    platform::delete(account);
}

/// Whether any item holds this material. Lets a test assert on a move that
/// minted an account it never got to record, without knowing the account —
/// and without counting, which other tests running alongside would disturb.
#[cfg(test)]
pub(crate) fn holds(material: &str) -> bool {
    platform::holds(material)
}

#[cfg(all(
    not(test),
    any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd"
    )
))]
mod platform {
    use crate::{EncryptionError, Result};
    use zeroize::Zeroizing;

    /// The service every item is filed under. Matches the macOS config
    /// directory's bundle identifier.
    const SERVICE: &str = "de.paviro.notema";

    /// Never written, so the probe read is expected to miss.
    const PROBE_ACCOUNT: &str = "availability-probe";

    pub(super) fn is_available() -> bool {
        // A read, because `Entry::new` defers OS contact and would answer `Ok`
        // with no keychain behind it. A miss returns before any unlock or ACL
        // check, so this cannot raise a dialog.
        match keyring::Entry::new(SERVICE, PROBE_ACCOUNT).and_then(|entry| entry.get_secret()) {
            Ok(_) => true,
            Err(keyring::Error::NoEntry) => true,
            // A keychain is still there, and storing into it prompts to
            // unlock. Callers treat a failed move as recoverable.
            Err(keyring::Error::NoStorageAccess(_)) => true,
            Err(_) => false,
        }
    }

    pub(super) fn fetch(account: &str) -> Result<Zeroizing<String>> {
        let secret = entry(account)?
            .get_secret()
            .map_err(|error| missing_or(error, account))?;
        crate::identity::bytes_to_utf8(secret)
    }

    pub(super) fn store(account: &str, material: &str) -> Result<()> {
        entry(account)?
            .set_secret(material.as_bytes())
            .map_err(failed)
    }

    pub(super) fn delete(account: &str) {
        if let Ok(entry) = entry(account) {
            // An item that is already gone is the state we were after.
            let _ = entry.delete_credential();
        }
    }

    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(failed)
    }

    /// A read that found nothing names the account; anything else is a failure.
    fn missing_or(error: keyring::Error, account: &str) -> EncryptionError {
        match error {
            keyring::Error::NoEntry => EncryptionError::KeyringItemMissing {
                account: account.to_string(),
            },
            other => failed(other),
        }
    }

    fn failed(error: keyring::Error) -> EncryptionError {
        match error {
            // Carries the bytes it could not decode — which are the key.
            keyring::Error::BadEncoding(bytes) => {
                drop(Zeroizing::new(bytes));
                EncryptionError::MalformedStoredIdentity
            }
            // `keyring` documents this as the store refusing, typically
            // locked; its Secret Service backend routes Locked/Prompt here.
            keyring::Error::NoStorageAccess(detail) => EncryptionError::KeyringLocked {
                detail: detail.to_string(),
            },
            // A failed D-Bus connect lands here, so this arm names the
            // escape hatch.
            keyring::Error::PlatformFailure(detail) => EncryptionError::KeyringUnavailable {
                detail: detail.to_string(),
            },
            other => EncryptionError::KeyringFailed {
                detail: other.to_string(),
            },
        }
    }
}

/// Everywhere else there is no keychain to talk to.
#[cfg(all(
    not(test),
    not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd"
    ))
))]
mod platform {
    use crate::{EncryptionError, Result};
    use zeroize::Zeroizing;

    pub(super) fn is_available() -> bool {
        false
    }

    pub(super) fn fetch(_account: &str) -> Result<Zeroizing<String>> {
        Err(unavailable())
    }

    pub(super) fn store(_account: &str, _material: &str) -> Result<()> {
        Err(unavailable())
    }

    pub(super) fn delete(_account: &str) {}

    fn unavailable() -> EncryptionError {
        EncryptionError::KeyringUnavailable {
            detail: "this platform has no OS keychain; keep the key in the identity file or fetch it with keys_command".to_string(),
        }
    }
}

/// An in-memory stand-in, so the tests can exercise everything built on top of a
/// keychain without touching a real one. The platform backends above are covered
/// separately by `notema-storage`'s `keyring_backend` tests.
#[cfg(test)]
mod platform {
    use crate::{EncryptionError, Result};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use zeroize::Zeroizing;

    fn items() -> &'static Mutex<HashMap<String, String>> {
        static ITEMS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        ITEMS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(super) fn is_available() -> bool {
        true
    }

    pub(super) fn fetch(account: &str) -> Result<Zeroizing<String>> {
        items()
            .lock()
            .expect("keyring test store")
            .get(account)
            .map(|found| Zeroizing::new(found.clone()))
            .ok_or_else(|| EncryptionError::KeyringItemMissing {
                account: account.to_string(),
            })
    }

    pub(super) fn store(account: &str, material: &str) -> Result<()> {
        items()
            .lock()
            .expect("keyring test store")
            .insert(account.to_string(), material.to_string());
        Ok(())
    }

    pub(super) fn delete(account: &str) {
        items().lock().expect("keyring test store").remove(account);
    }

    pub(super) fn holds(material: &str) -> bool {
        items()
            .lock()
            .expect("keyring test store")
            .values()
            .any(|held| held == material)
    }
}
