//! The one test that talks to a real OS keychain.
//!
//! Everything else that exercises keychain-backed keys runs against the
//! in-memory double in `notema-encryption`, which is right for the logic built
//! on top but leaves the platform backends themselves — and the mapping from
//! `keyring::Error` onto ours — completely uncovered.
//!
//! Opt in with `NOTEMA_TEST_REAL_KEYRING=1`. It is off by default because it
//! writes to whichever keychain the running user has: on a developer's machine
//! that is their login keychain, and on macOS an unsigned test binary has no
//! stable code identity, so reading back what it just wrote raises a GUI prompt
//! that would hang the suite. CI runs it on Linux against a throwaway
//! `gnome-keyring` inside `dbus-run-session`.

use notema_encryption::{KeyStore, KeyTarget};
use notema_storage::JournalStore;

fn enabled() -> bool {
    std::env::var("NOTEMA_TEST_REAL_KEYRING").is_ok_and(|value| value == "1")
}

/// A full round trip through the real backend: store, read back, and clean up.
///
/// Deliberately goes through the public store API rather than poking the keyring
/// module, so it covers the path a user actually takes — including the readback
/// verification that has to fetch from the keychain to succeed.
#[test]
fn a_real_keychain_round_trips_and_cleans_up_after_itself() {
    if !enabled() {
        eprintln!("skipping: set NOTEMA_TEST_REAL_KEYRING=1 to run against a real keychain");
        return;
    }

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
fn a_missing_keychain_item_reads_as_missing_not_malformed() {
    if !enabled() {
        eprintln!("skipping: set NOTEMA_TEST_REAL_KEYRING=1 to run against a real keychain");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let mut store = JournalStore::new(dir.path().join("journals"), dir.path());
    store.ensure().unwrap();
    store.initialize_encryption("laptop", None).unwrap();
    store.unlock(None).unwrap();
    store.set_key_store(&KeyTarget::Keyring, None).unwrap();

    // Point the file at an account nothing ever stored.
    let identity_file = dir.path().join("identity.toml");
    let pointer = std::fs::read_to_string(&identity_file).unwrap();
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

    // Clean up the item the first move created.
    std::fs::write(&identity_file, &pointer).unwrap();
    let cleanup = JournalStore::new(dir.path().join("journals"), dir.path());
    cleanup.ensure().unwrap();
    cleanup.set_key_store(&KeyTarget::File, None).unwrap();
}
