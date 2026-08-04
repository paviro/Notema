#![forbid(unsafe_code)]

//! Journal's encryption layer: per-device age keypairs, a signed append-only
//! device roster, passphrase-wrapped identities, and the helpers that turn
//! journal bytes into age ciphertext and back.
//!
//! It owns all of the app's cryptography and knows nothing about how entries or
//! assets are laid out on disk: it works on a [`KeyPaths`] and byte streams, and
//! the storage layer decides which files they belong to.
//!
//! Scope: this layer provides **confidentiality** (and, through the roster,
//! authenticated device membership) but **not** per-entry authenticity or author
//! attribution — entries and assets are encrypted, not signed. See the roster
//! module's "Residual threats" notes.

mod cipher;
mod error;
mod files;
mod identity;
mod key_command;
mod keyring;
mod paths;
mod pending;
mod recipients;
mod roster;
mod signing;

#[cfg(test)]
mod tests;

pub use age::secrecy::{ExposeSecret, SecretString};
pub use zeroize::Zeroizing;

pub use cipher::{
    CiphertextBytes, EncryptionRecipients, PlaintextBytes, decrypt_bytes, decrypt_file_bytes,
    decrypt_file_reader, encrypt_bytes, encrypt_new_entry, encrypt_to_file,
    encrypted_plaintext_len,
};
pub use error::{CommandStderr, EncryptionError, Result};
pub use files::{atomic_write, atomic_write_private, atomic_write_with, sync_parent_dir};
pub use identity::{
    DeviceIdentityInfo, FetchedKey, IdentitySnapshot, KeyStore, KeyTarget, UnlockedIdentity,
    check_key_is_writable, device_identity_info, export_identity, fetch_key_material,
    read_identity_file_bytes, restore_identity, restore_identity_file, set_identity_passphrase,
    set_key_store, snapshot_identity, unlock_fetched, unlock_identity,
};
pub use key_command::KeyCommand;

/// Whether this build can reach an OS keychain here, so a new identity knows
/// whether keeping its key there is an option.
pub fn keyring_available() -> bool {
    keyring::is_available()
}
pub use paths::{IDENTITY_FILE_NAME, KeyPaths};
pub use pending::{
    PendingRequest, read_pending, remove_pending, renew_store_access, request_store_access,
};
pub use recipients::{
    Recipient, add_recipient, advance_trust_pins, commit_rotated_identity, drop_old_recipient,
    identity_is_recipient, initialize_store_identity, read_recipients, rename_recipient,
    revoke_recipient, revoked_recipient_keys, rotate_add_new_key,
};
