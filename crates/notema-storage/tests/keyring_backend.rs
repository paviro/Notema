//! The one test that talks to a real OS keychain.
//!
//! Everything else that exercises keychain-backed keys runs against the
//! in-memory double in `notema-encryption`, which is right for the logic built
//! on top but leaves the platform backends themselves — and the mapping from
//! `keyring::Error` onto ours — completely uncovered.
//!
//! These are `#[ignore]`d and additionally need `NOTEMA_TEST_REAL_KEYRING=1`,
//! because they write to whichever keychain the running user has: on a
//! developer's machine that is their login keychain, and on macOS an unsigned
//! test binary has no stable code identity, so reading back what it just wrote
//! raises a GUI prompt that would hang the suite. CI runs them on Linux against
//! a throwaway `gnome-keyring` inside `dbus-run-session`.

use notema_encryption::{KeyStore, KeyTarget};
use notema_storage::JournalStore;
use std::path::PathBuf;

/// Refuse rather than skip. `#[ignore]` already means nothing runs these unless
/// something asked for them by name, so a missing opt-in is a misconfigured
/// runner — and a runner that silently stopped setting it would otherwise report
/// a green keychain job having tested nothing.
fn require_opt_in() {
    assert!(
        std::env::var("NOTEMA_TEST_REAL_KEYRING").is_ok_and(|value| value == "1"),
        "set NOTEMA_TEST_REAL_KEYRING=1 to confirm you meant to write to this machine's keychain"
    );
}

/// Puts the key back in the identity file however the test ends. Without this a
/// failed assertion strands a real keychain item under a random hex account that
/// nothing afterwards can name.
struct KeychainCleanup {
    config_dir: PathBuf,
    pointer: String,
}

impl Drop for KeychainCleanup {
    fn drop(&mut self) {
        let identity_file = self.config_dir.join("identity.toml");
        // The key is already inline, so the test's own move-out ran and took the
        // item with it.
        if std::fs::read_to_string(&identity_file)
            .is_ok_and(|current| current.contains("AGE-SECRET-KEY-"))
        {
            return;
        }
        let _ = std::fs::write(&identity_file, &self.pointer);
        let store = JournalStore::new(self.config_dir.join("journals"), &self.config_dir);
        if store.ensure().is_ok() {
            let _ = store.set_key_store(&KeyTarget::File, None);
        }
    }
}

/// A full round trip through the real backend: store, read back, and clean up.
///
/// Deliberately goes through the public store API rather than poking the keyring
/// module, so it covers the path a user actually takes — including the readback
/// verification that has to fetch from the keychain to succeed.
#[test]
#[ignore = "writes to this machine's real OS keychain; run with --ignored"]
fn a_real_keychain_round_trips_and_cleans_up_after_itself() {
    require_opt_in();
    assert!(
        notema_encryption::keyring_available(),
        "no keychain reachable, so there is nothing to test against"
    );

    let dir = tempfile::tempdir().unwrap();
    let mut store = JournalStore::new(dir.path().join("journals"), dir.path());
    store.ensure().unwrap();
    store.initialize_encryption("laptop", None).unwrap();
    store.unlock(None).unwrap();
    let recipient = store.public_recipient().unwrap();

    store.set_key_store(&KeyTarget::Keyring, None).unwrap();

    let identity_file = dir.path().join("identity.toml");
    let pointer = std::fs::read_to_string(&identity_file).unwrap();
    let _cleanup = KeychainCleanup {
        config_dir: dir.path().to_path_buf(),
        pointer: pointer.clone(),
    };
    assert!(
        !pointer.contains("AGE-SECRET-KEY-"),
        "the key should have left the file: {pointer}"
    );
    assert!(
        pointer.contains("keyring_account"),
        "the file should point at the keychain item: {pointer}"
    );
    assert_eq!(
        store.this_device().unwrap().unwrap().store,
        KeyStore::Keyring
    );

    // A fresh store proves the key really comes back out of the keychain rather
    // than out of anything cached in the one that put it there.
    let mut reopened = JournalStore::new(dir.path().join("journals"), dir.path());
    reopened.ensure().unwrap();
    reopened.prefetch_key_material().unwrap();
    reopened.unlock(None).unwrap();
    assert_eq!(reopened.public_recipient().unwrap(), recipient);

    // Moving back out removes the item, so the test leaves no litter behind.
    reopened.set_key_store(&KeyTarget::File, None).unwrap();
    assert_eq!(
        reopened.this_device().unwrap().unwrap().store,
        KeyStore::File
    );
    assert!(
        std::fs::read_to_string(&identity_file)
            .unwrap()
            .contains("AGE-SECRET-KEY-"),
        "the key should be back in the file"
    );
}

/// A pointer at a keychain item that isn't there must read as missing, not as
/// corruption — the difference between "restore your backup" and "unlock your
/// keychain", which is the whole point of the error mapping.
#[test]
#[ignore = "writes to this machine's real OS keychain; run with --ignored"]
fn a_missing_keychain_item_reads_as_missing_not_malformed() {
    require_opt_in();
    let dir = tempfile::tempdir().unwrap();
    let mut store = JournalStore::new(dir.path().join("journals"), dir.path());
    store.ensure().unwrap();
    store.initialize_encryption("laptop", None).unwrap();
    store.unlock(None).unwrap();
    store.set_key_store(&KeyTarget::Keyring, None).unwrap();

    // Point the file at an account nothing ever stored.
    let identity_file = dir.path().join("identity.toml");
    let pointer = std::fs::read_to_string(&identity_file).unwrap();
    let _cleanup = KeychainCleanup {
        config_dir: dir.path().to_path_buf(),
        pointer: pointer.clone(),
    };
    let repointed: String = pointer
        .lines()
        .map(|line| {
            if line.starts_with("keyring_account") {
                "keyring_account = \"0000000000000000deadbeef00000000\"".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&identity_file, format!("{repointed}\n")).unwrap();

    let mut orphaned = JournalStore::new(dir.path().join("journals"), dir.path());
    orphaned.ensure().unwrap();
    let error = orphaned
        .prefetch_key_material()
        .expect_err("a missing keychain item must not look like a working key");
    let message = error.to_string();
    assert!(
        message.contains("no key for this device"),
        "expected a missing-item message, got: {message}"
    );
    assert!(
        !message.contains("malformed"),
        "a missing item is not corruption: {message}"
    );
    // `_cleanup` removes the item the first move created.
}
