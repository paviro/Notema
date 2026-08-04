use crate::files::{PrivateFileExposure, atomic_write_private};
use crate::key_command::KeyCommand;
use crate::signing::{generate_signing_key, signing_public};
use crate::{
    EncryptionError, KeyPaths, Recipient, Result, cipher, files, key_command, keyring, recipients,
};
use age::secrecy::{ExposeSecret, SecretString};
use age::x25519;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path, str::FromStr};
use zeroize::Zeroizing;

const IDENTITY_SCHEMA_VERSION: u32 = 1;
const SECRET_BUNDLE_SCHEMA_VERSION: u32 = 1;

/// This device's decrypted keypair: the age identity that reads encrypted
/// entries and the Ed25519 signing key that authorizes roster ops.
#[derive(Clone)]
pub struct UnlockedIdentity {
    pub(crate) identity: x25519::Identity,
    pub(crate) signing: SigningKey,
}

impl UnlockedIdentity {
    pub(crate) fn recipient(&self) -> x25519::Recipient {
        self.identity.to_public()
    }

    /// This identity's age public key string, for matching against recipients.
    pub fn public_key(&self) -> String {
        self.identity.to_public().to_string()
    }

    /// This device's Ed25519 signing public key, `ed25519:<hex>`.
    pub fn signing_public(&self) -> String {
        signing_public(&self.signing)
    }
}

/// Which store keeps this device's key. Independent of whether it is
/// passphrase-encrypted: every store holds either form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStore {
    /// Inline in `identity.toml`, mode 0600.
    File,
    /// An item in the OS keychain.
    Keyring,
    /// Fetched by running a command.
    Command,
}

impl KeyStore {
    /// Completes the sentence "this device's key is …".
    pub fn whereabouts(self) -> &'static str {
        match self {
            Self::File => "kept in the identity file",
            Self::Keyring => "kept in the OS keychain",
            Self::Command => "fetched by an external command",
        }
    }
}

/// The non-secret facts about this device's stored identity, readable without a
/// passphrase and without retrieving the key: how it labels itself, where the key
/// lives, and whether unlocking needs a passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentityInfo {
    pub name: String,
    pub store: KeyStore,
    pub passphrase_protected: bool,
    /// For a command-backed key, the fetch command as recorded — no permission
    /// check can tell an identity file you wrote from one that arrived with your
    /// dotfiles, so the line is shown rather than only obeyed.
    pub fetch_command: Option<String>,
    /// Set when the identity file's permissions let someone other than its owner
    /// reach it. Reported here; refused where it matters, in
    /// [`fetch_key_material`] and the rest of the read path.
    pub exposure: Option<PrivateFileExposure>,
}

/// The secret key material for a device, serialized inside the (optionally
/// scrypt-wrapped) key bundle: the age private key and the Ed25519 signing seed.
/// Kept together so both are protected by the same passphrase choice.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretBundle {
    schema_version: u32,
    x25519: Zeroizing<String>,
    ed25519: Zeroizing<String>,
}

/// Where the stored key bytes come from. Fetching one of these yields the bundle
/// in whichever form it was stored; see [`StoredKey::encrypted`] for which.
enum KeyLocation {
    /// Held in `identity.toml` itself.
    Inline(Zeroizing<String>),
    /// Held in the OS keychain under this account label.
    Keyring(String),
    /// Printed on stdout by `read`, and written by `write` — without which the
    /// key can be fetched but never replaced.
    Command {
        read: KeyCommand,
        write: Option<KeyCommand>,
    },
}

impl KeyLocation {
    fn store(&self) -> KeyStore {
        match self {
            Self::Inline(_) => KeyStore::File,
            Self::Keyring(_) => KeyStore::Keyring,
            Self::Command { .. } => KeyStore::Command,
        }
    }

    /// Put `material` where this location keeps it.
    ///
    /// `Inline` is a no-op: its bytes travel in the identity file the caller is
    /// about to write, rather than anywhere separate.
    fn write(&self, material: &Zeroizing<String>) -> Result<()> {
        match self {
            Self::Inline(_) => Ok(()),
            Self::Keyring(account) => keyring::store(account, material),
            Self::Command {
                write: Some(write), ..
            } => key_command::store(write, material.as_bytes()),
            Self::Command { read, write: None } => Err(EncryptionError::KeyStoreReadOnly {
                command: read.label(),
            }),
        }
    }

    /// This location with `material` swapped in, for the locations that carry it.
    fn with_material(&self, material: &Zeroizing<String>) -> Self {
        match self {
            Self::Inline(_) => Self::Inline(material.clone()),
            Self::Keyring(account) => Self::Keyring(account.clone()),
            Self::Command { read, write } => Self::Command {
                read: read.clone(),
                write: write.clone(),
            },
        }
    }
}

/// How this device's key is stored: where the bytes live, and whether they are
/// scrypt-wrapped. The two are independent — any location holds either form.
struct StoredKey {
    location: KeyLocation,
    encrypted: bool,
}

/// This device's stored identity: the label it stores itself under and how to get
/// at its key. Built from [`StoredIdentityWire`], which enforces that exactly one
/// location is named.
struct StoredIdentity {
    device_name: String,
    key: StoredKey,
    /// How exposed the file this was read from is — `None` when only its owner
    /// can reach it, and always `None` off Unix.
    exposure: Option<PrivateFileExposure>,
}

/// The on-disk shape of `identity.toml`: `device_name`, exactly one key location,
/// and — for the locations that don't say so themselves — whether what's stored
/// there is scrypt-wrapped.
///
/// `encrypted_keys` and `plain_keys` are self-describing, so they must not be
/// paired with `keys_encrypted`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIdentityWire {
    schema_version: u32,
    device_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encrypted_keys: Option<Zeroizing<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plain_keys: Option<Zeroizing<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keyring_account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keys_command: Option<KeyCommand>,
    /// Given the key on stdin whenever it is replaced. Without it `keys_command`
    /// is read-only, and rotating or re-wrapping the key has nowhere to write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keys_store_command: Option<KeyCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keys_encrypted: Option<bool>,
}

impl StoredIdentity {
    /// Build from the on-disk shape, enforcing that exactly one key location is
    /// named. `exposure` is how far the file it came from has drifted from
    /// owner-only.
    fn from_wire(wire: StoredIdentityWire, exposure: Option<PrivateFileExposure>) -> Result<Self> {
        if wire.schema_version != IDENTITY_SCHEMA_VERSION {
            return Err(EncryptionError::UnsupportedSchema {
                kind: "device identity file",
                version: wire.schema_version,
            });
        }

        // Exactly one location may be named; collecting them lets the error name
        // the offenders. `implied` is the format a field's own name states.
        let mut present: Vec<(&'static str, KeyLocation, Option<bool>)> = Vec::with_capacity(4);
        if let Some(armor) = wire.encrypted_keys {
            present.push(("encrypted_keys", KeyLocation::Inline(armor), Some(true)));
        }
        if let Some(plain) = wire.plain_keys {
            present.push(("plain_keys", KeyLocation::Inline(plain), Some(false)));
        }
        if let Some(account) = wire.keyring_account {
            present.push(("keyring_account", KeyLocation::Keyring(account), None));
        }
        let store_command = wire.keys_store_command;
        if let Some(read) = wire.keys_command {
            present.push((
                "keys_command",
                KeyLocation::Command {
                    read,
                    write: store_command,
                },
                None,
            ));
        } else if store_command.is_some() {
            // Only meaningful as the write half of `keys_command`; alone it
            // would be dropped here and erased by the next write.
            return Err(EncryptionError::OrphanedStoreCommand);
        }

        if present.len() > 1 {
            let fields: Vec<&str> = present.iter().map(|(name, _, _)| *name).collect();
            return Err(EncryptionError::AmbiguousKeyLocation {
                fields: fields.join(", "),
            });
        }
        let (field, location, implied) = present.pop().ok_or(EncryptionError::NoKeyLocation)?;

        let encrypted = match (implied, wire.keys_encrypted) {
            (Some(_), Some(_)) => return Err(EncryptionError::RedundantKeysEncrypted { field }),
            (Some(implied), None) => implied,
            (None, Some(explicit)) => explicit,
            // Defaulting would guess, and a wrong guess reads as corruption:
            // armor parsed as a bundle is "malformed key material".
            (None, None) => return Err(EncryptionError::MissingKeyFormat { field }),
        };

        Ok(Self {
            device_name: wire.device_name,
            key: StoredKey {
                location,
                encrypted,
            },
            exposure,
        })
    }
}

/// This device's key material as it is stored, before any passphrase is applied.
///
/// Retrieving it is separate from opening it because the two can need the
/// terminal at the same time: a `keys_command` may prompt on the TTY, while a
/// passphrase-protected identity prompts inside the TUI.
#[derive(Clone)]
pub struct FetchedKey {
    material: Zeroizing<String>,
    encrypted: bool,
}

impl FetchedKey {
    pub fn passphrase_protected(&self) -> bool {
        self.encrypted
    }
}

/// This device's stored identity label, where its key lives, and whether it is
/// passphrase-protected — without retrieving or decrypting anything. `None` when
/// no identity file exists here.
pub fn device_identity_info(paths: &KeyPaths) -> Result<Option<DeviceIdentityInfo>> {
    if !paths.identity_file.exists() {
        return Ok(None);
    }
    // Inspect, not read: this is the report that says a file is too loose to run
    // a command out of, so it must not be refused for being one.
    let stored = inspect_stored_identity(&paths.identity_file)?;
    Ok(Some(DeviceIdentityInfo {
        name: stored.device_name,
        store: stored.key.location.store(),
        passphrase_protected: stored.key.encrypted,
        fetch_command: match &stored.key.location {
            KeyLocation::Command { read, .. } => Some(read.display_line()),
            _ => None,
        },
        exposure: stored.exposure,
    }))
}

/// Retrieve this device's stored key material from wherever it lives.
///
/// Never needs a passphrase, and may run a command that wants the terminal — so
/// callers must invoke this before putting the terminal into raw mode.
pub fn fetch_key_material(paths: &KeyPaths) -> Result<FetchedKey> {
    fetch_stored(&read_stored_identity(&paths.identity_file)?.key)
}

fn fetch_stored(key: &StoredKey) -> Result<FetchedKey> {
    let material = match &key.location {
        KeyLocation::Inline(text) => text.clone(),
        KeyLocation::Keyring(account) => keyring::fetch(account)?,
        KeyLocation::Command { read, .. } => bytes_to_utf8(key_command::run(read)?.to_vec())?,
    };
    Ok(FetchedKey {
        material,
        encrypted: key.encrypted,
    })
}

/// Load this device's identity so encrypted entries can be read and written.
/// `passphrase` must be `Some` for a passphrase-protected identity and is
/// ignored for a plaintext one.
pub fn unlock_identity(
    paths: &KeyPaths,
    passphrase: Option<&SecretString>,
) -> Result<UnlockedIdentity> {
    unlock_fetched(paths, &fetch_key_material(paths)?, passphrase)
}

/// Open key material already retrieved by [`fetch_key_material`], then run the
/// same validation [`unlock_identity`] does.
pub fn unlock_fetched(
    paths: &KeyPaths,
    fetched: &FetchedKey,
    passphrase: Option<&SecretString>,
) -> Result<UnlockedIdentity> {
    let unlocked = unwrap_key(fetched, passphrase)?;

    // Validate via a self round-trip (encrypt to our own public key, decrypt with
    // the identity). Unlike checking against the shared roster, this holds even
    // before this device has been approved as a store recipient.
    let recipient = unlocked.recipient();
    let probe = cipher::PlaintextBytes::copy_from_slice(b"notema identity check");
    let encrypted = cipher::encrypt_to_recipients(std::slice::from_ref(&recipient), &probe)?;
    if cipher::decrypt_bytes_with_identity(&encrypted, &unlocked.identity)?.as_bytes()
        != probe.as_bytes()
    {
        return Err(EncryptionError::IdentityCheckFailed);
    }

    // Trust-on-first-use / advance the roster pins now that we're in at rest, so a
    // later rollback of anything seen up to now is detectable.
    recipients::refresh_trust_pins(paths);

    Ok(unlocked)
}

/// Unwrap fetched key material into a usable identity: scrypt-decrypt it when it
/// is stored encrypted, then parse the bundle.
fn unwrap_key(fetched: &FetchedKey, passphrase: Option<&SecretString>) -> Result<UnlockedIdentity> {
    // The decrypted secret bundle lives in this string; zeroize it on drop so it
    // doesn't linger in freed heap after we parse it into keys.
    let bundle_toml: Zeroizing<String> = if fetched.encrypted {
        let passphrase = passphrase.ok_or(EncryptionError::PassphraseRequired)?;
        let identity = age::scrypt::Identity::new(passphrase.clone());
        bytes_to_utf8(age::decrypt(&identity, fetched.material.as_bytes())?)?
    } else {
        fetched.material.clone()
    };
    parse_bundle(&bundle_toml)
}

fn parse_bundle(bundle_toml: &str) -> Result<UnlockedIdentity> {
    // A bare age secret key is what a secret manager most often already holds,
    // and what someone hand-editing the identity file reaches for, so say what's
    // actually wrong instead of "malformed". Captures nothing: the offending text
    // is the secret.
    if bundle_toml.trim_start().starts_with("AGE-SECRET-KEY-") {
        return Err(EncryptionError::BareAgeKeyNotBundle);
    }
    // toml::de::Error's Display echoes the offending input line — here the
    // decrypted secret bundle — and would retain it unzeroized; report the
    // plain malformed-identity error instead, like the UTF-8 guard above.
    let bundle: SecretBundle =
        toml::from_str(bundle_toml).map_err(|_| EncryptionError::MalformedStoredIdentity)?;
    if bundle.schema_version != SECRET_BUNDLE_SCHEMA_VERSION {
        return Err(EncryptionError::UnsupportedSchema {
            kind: "secret identity bundle",
            version: bundle.schema_version,
        });
    }
    let identity = x25519::Identity::from_str(bundle.x25519.trim())
        .map_err(|_| EncryptionError::MalformedStoredIdentity)?;
    let seed_bytes = Zeroizing::new(hex::decode(bundle.ed25519.trim())?);
    let seed = <[u8; 32]>::try_from(seed_bytes.as_slice())
        .map_err(|_| EncryptionError::MalformedStoredIdentity)?;
    Ok(UnlockedIdentity {
        identity,
        signing: SigningKey::from_bytes(&seed),
    })
}

/// On invalid UTF-8, `FromUtf8Error` would carry the secret bundle unzeroized
/// (and expose it via `Debug`); drop those bytes inside a `Zeroizing` and report
/// a plain malformed-identity error instead.
pub(crate) fn bytes_to_utf8(bytes: Vec<u8>) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(String::from_utf8(bytes).map_err(
        |error| {
            drop(Zeroizing::new(error.into_bytes()));
            EncryptionError::MalformedStoredIdentity
        },
    )?))
}

/// Reject an empty passphrase before it wraps any key material. An empty string
/// would silently degrade to plaintext-equivalent scrypt, so both write paths
/// route through this guard.
fn reject_empty_passphrase(passphrase: Option<&SecretString>) -> Result<()> {
    if matches!(passphrase, Some(passphrase) if passphrase.expose_secret().is_empty()) {
        return Err(EncryptionError::EmptyPassphrase);
    }
    Ok(())
}

/// Where this device's key is kept right now, so replacing it can put the new
/// material back in the same place. A brand-new identity has no location yet and
/// starts inline.
fn current_location(paths: &KeyPaths) -> Result<Option<KeyLocation>> {
    if !paths.identity_file.exists() {
        return Ok(None);
    }
    Ok(Some(
        read_stored_identity(&paths.identity_file)?.key.location,
    ))
}

/// Check that new key material could be written where this device's key is kept,
/// without writing any.
///
/// For refusing early: before a passphrase prompt, and before a rotation appends
/// the roster op it would otherwise have to roll back. Only a `keys_command` with
/// no store command can't take a write.
pub fn check_key_is_writable(paths: &KeyPaths) -> Result<()> {
    match current_location(paths)? {
        Some(KeyLocation::Command { read, write: None }) => {
            Err(EncryptionError::KeyStoreReadOnly {
                command: read.label(),
            })
        }
        _ => Ok(()),
    }
}

/// Re-wrap this device's stored identity with a different passphrase state:
/// `current` unlocks it as stored now, `new` chooses how to store it going
/// forward (`Some` = scrypt-wrapped, `None` = plaintext mode-0600). Only rewrites
/// the local identity file; the keypair and all entries are untouched.
pub fn set_identity_passphrase(
    paths: &KeyPaths,
    current: Option<&SecretString>,
    new: Option<&SecretString>,
) -> Result<()> {
    reject_empty_passphrase(new)?;
    // Ahead of the unlock, so a read-only key store is refused before anyone is
    // asked for a passphrase that was never going to be used.
    check_key_is_writable(paths)?;
    let stored = read_stored_identity(&paths.identity_file)?;
    let fetched = fetch_stored(&stored.key)?;
    let identity = unlock_fetched(paths, &fetched, current)?;

    // An external store takes the re-wrapped key before the identity file
    // records its new format, so a failure between the two would leave the file
    // describing the old one and the key unopenable. Inline has no such gap: the
    // file is the only thing written, and it is written atomically.
    let before = if matches!(stored.key.location, KeyLocation::Inline(_)) {
        None
    } else {
        Some(IdentitySnapshot {
            wire: Zeroizing::new(fs::read(&paths.identity_file)?),
            device_name: stored.device_name.clone(),
            location: stored.key.location,
            material: fetched.material.clone(),
            encrypted: fetched.encrypted,
        })
    };

    let written = write_stored_identity(paths, &stored.device_name, &identity, new);
    match (written, before) {
        (Err(original), Some(before)) => Err(match restore_identity(paths, &before) {
            Ok(()) => original,
            Err(rollback) => EncryptionError::IdentityRollbackFailed {
                original: original.to_string(),
                rollback: rollback.to_string(),
            },
        }),
        (written, _) => written,
    }
}

/// Which store to move this device's key to.
///
/// Switching store never changes format: what was scrypt-wrapped stays
/// wrapped, and [`set_identity_passphrase`] remains the only thing that changes
/// that.
pub enum KeyTarget {
    /// Inline in `identity.toml`, mode 0600.
    File,
    /// An item in the OS keychain.
    Keyring,
    Command {
        /// Prints the stored key on stdout whenever it is needed.
        read: KeyCommand,
        /// Given the key on its stdin whenever it changes. Piping rather than
        /// passing an argument is the point: argv is world-readable.
        write: Option<KeyCommand>,
    },
}

/// Move this device's key to `target`, keeping its current format.
///
/// The new location is written and read back before the old copy is dropped, so
/// a store that silently didn't take can't leave the device without a key.
/// `passphrase` is needed only when the identity is passphrase-protected.
pub fn set_key_store(
    paths: &KeyPaths,
    target: &KeyTarget,
    passphrase: Option<&SecretString>,
) -> Result<()> {
    let stored = read_stored_identity(&paths.identity_file)?;
    let fetched = fetch_stored(&stored.key)?;
    // Prove we can open what's there now, before moving it anywhere.
    let current = unwrap_key(&fetched, passphrase)?;

    let moved = match target {
        KeyTarget::File => KeyLocation::Inline(fetched.material.clone()),
        KeyTarget::Keyring => {
            let account = new_keyring_account()?;
            keyring::store(&account, &fetched.material)?;
            KeyLocation::Keyring(account)
        }
        KeyTarget::Command { read, write } => {
            if let Some(write) = write {
                key_command::store(write, fetched.material.as_bytes())?;
            }
            KeyLocation::Command {
                read: read.clone(),
                write: write.clone(),
            }
        }
    };
    let moved = StoredKey {
        location: moved,
        encrypted: fetched.encrypted,
    };

    // Read it back from where it now lives and confirm the same key comes out.
    // Until the identity file names the new location, a failure would leave
    // the key somewhere nothing points at.
    let verified = (|| {
        let readback = unwrap_key(&fetch_stored(&moved)?, passphrase)?;
        if readback.public_key() != current.public_key()
            || readback.signing_public() != current.signing_public()
        {
            return Err(EncryptionError::KeyStoreMismatch);
        }
        write_identity_file(&paths.identity_file, &wire_for(&stored.device_name, &moved))
    })();
    if let Err(error) = verified {
        // Minted above and never recorded, so nothing else would collect it.
        if let KeyLocation::Keyring(account) = &moved.location {
            keyring::delete(account);
        }
        return Err(error);
    }

    // Nothing references the old keychain item now.
    if let KeyLocation::Keyring(account) = &stored.key.location
        && !matches!(&moved.location, KeyLocation::Keyring(new) if new == account)
    {
        keyring::delete(account);
    }
    Ok(())
}

/// Write a standalone copy of this device's identity to `destination`, mode 0600.
///
/// The result is a complete `identity.toml` holding the key in the form it is
/// stored in — age armor when passphrase-protected, so `age --decrypt` still
/// opens it — which makes restoring it a matter of copying the file back into a
/// config directory. Needs no passphrase: the stored form is copied, not opened.
pub fn export_identity(paths: &KeyPaths, destination: &Path) -> Result<()> {
    let stored = read_stored_identity(&paths.identity_file)?;
    let fetched = fetch_stored(&stored.key)?;
    let inline = StoredKey {
        location: KeyLocation::Inline(fetched.material.clone()),
        encrypted: fetched.encrypted,
    };
    write_identity_file(destination, &wire_for(&stored.device_name, &inline))
}

/// A fresh opaque label for this device's keychain item.
///
/// Deliberately not derived from the device name: renaming a device only appends
/// a roster op and leaves the local `device_name` stale, so a derived account
/// would stop resolving.
fn new_keyring_account() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| EncryptionError::Randomness(error.to_string()))?;
    Ok(hex::encode(bytes))
}

/// Everything needed to put this device's identity back as it was: the exact
/// identity file, plus the key itself retrieved from wherever it was kept.
///
/// Both halves matter for a rollback. Restoring the file alone would leave it
/// pointing at a keychain item or secret manager entry that a half-finished
/// rotation had already overwritten with the new key.
pub struct IdentitySnapshot {
    wire: Zeroizing<Vec<u8>>,
    device_name: String,
    location: KeyLocation,
    material: Zeroizing<String>,
    encrypted: bool,
}

impl IdentitySnapshot {
    /// A standalone identity file holding the key inline, for the on-disk rescue
    /// copy a rotation leaves behind. It has to stand on its own: the live file
    /// may only point at the key, which is no use if the rotation is what broke
    /// the thing it points at.
    pub fn portable_bytes(&self) -> Result<Zeroizing<Vec<u8>>> {
        let inline = StoredKey {
            location: KeyLocation::Inline(self.material.clone()),
            encrypted: self.encrypted,
        };
        let text = Zeroizing::new(toml::to_string_pretty(&wire_for(
            &self.device_name,
            &inline,
        ))?);
        Ok(Zeroizing::new(text.as_bytes().to_vec()))
    }

    /// Whether the key is kept somewhere other than the identity file, so a copy
    /// of that file alone would only be a pointer at it.
    pub fn is_external(&self) -> bool {
        !matches!(self.location, KeyLocation::Inline(_))
    }

    /// Which store held the key when this snapshot was taken.
    pub fn store(&self) -> KeyStore {
        self.location.store()
    }

    /// Drop the stored copy this snapshot came from, once the snapshot itself
    /// has been written somewhere self-contained. Best effort: a keychain we
    /// can't reach is not a reason to fail the retirement that called this.
    pub fn forget_stored_key(&self) {
        if let KeyLocation::Keyring(account) = &self.location {
            keyring::delete(account);
        }
    }
}

/// Capture this device's identity before something replaces it.
pub fn snapshot_identity(paths: &KeyPaths) -> Result<IdentitySnapshot> {
    let stored = read_stored_identity(&paths.identity_file)?;
    let fetched = fetch_stored(&stored.key)?;
    Ok(IdentitySnapshot {
        wire: Zeroizing::new(fs::read(&paths.identity_file)?),
        device_name: stored.device_name,
        location: stored.key.location,
        material: fetched.material,
        encrypted: fetched.encrypted,
    })
}

/// Put back an identity captured by [`snapshot_identity`]: the key first, into
/// wherever it was kept, then the file that points at it.
pub fn restore_identity(paths: &KeyPaths, snapshot: &IdentitySnapshot) -> Result<()> {
    snapshot.location.write(&snapshot.material)?;
    atomic_write_private(&paths.identity_file, &snapshot.wire)
}

/// Read this device's identity file verbatim, for snapshotting before a rotation
/// so it can be put back byte-for-byte if the rotation fails.
pub fn read_identity_file_bytes(paths: &KeyPaths) -> Result<Vec<u8>> {
    Ok(fs::read(&paths.identity_file)?)
}

/// Restore this device's identity file from bytes captured by
/// [`read_identity_file_bytes`], preserving the private-file mode (0600).
pub fn restore_identity_file(paths: &KeyPaths, bytes: &[u8]) -> Result<()> {
    atomic_write_private(&paths.identity_file, bytes)
}

/// Generate this device's keypair and its public [`Recipient`], writing the
/// private identity (scrypt-wrapped when `passphrase` is `Some`, plaintext
/// mode-0600 otherwise). Shared by the store-creating and the joining device.
pub(crate) fn create_device_identity(
    paths: &KeyPaths,
    name: &str,
    passphrase: Option<&SecretString>,
) -> Result<(Recipient, UnlockedIdentity)> {
    if name.trim().is_empty() {
        return Err(EncryptionError::EmptyDeviceName);
    }
    // The file may be the only pointer at a keyring item or store command, and
    // `write_stored_identity` writes through to whatever it names.
    if paths.has_identity() {
        return Err(EncryptionError::IdentityExists {
            path: paths.identity_file.clone(),
        });
    }

    let identity = UnlockedIdentity {
        identity: x25519::Identity::generate(),
        signing: generate_signing_key()?,
    };
    let recipient = Recipient {
        name: name.to_string(),
        encryption_key: identity.public_key(),
        signing_key: identity.signing_public(),
    };
    write_stored_identity(paths, name, &identity, passphrase)?;
    Ok((recipient, identity))
}

/// Write this device's identity file, scrypt-wrapping the private key material
/// when a passphrase is given and storing it plaintext (mode 0600) otherwise.
/// Both the age key and the Ed25519 signing seed are bundled together so the same
/// passphrase choice protects both.
///
/// The material goes back to whichever store already holds this device's key; a
/// brand-new identity has none yet and starts inline.
pub(crate) fn write_stored_identity(
    paths: &KeyPaths,
    name: &str,
    identity: &UnlockedIdentity,
    passphrase: Option<&SecretString>,
) -> Result<()> {
    reject_empty_passphrase(passphrase)?;
    let material = stored_form(identity, passphrase)?;
    let location = match current_location(paths)? {
        Some(location) => location.with_material(&material.text),
        None => KeyLocation::Inline(material.text.clone()),
    };
    location.write(&material.text)?;
    write_identity_file(
        &paths.identity_file,
        &wire_for(
            name,
            &StoredKey {
                location,
                encrypted: material.encrypted,
            },
        ),
    )
}

/// This device's key bundle in the form it gets stored in: age ASCII armor when a
/// passphrase is given (a standalone age file, so recovery is possible with the
/// `age` CLI), cleartext bundle TOML otherwise.
fn stored_form(
    identity: &UnlockedIdentity,
    passphrase: Option<&SecretString>,
) -> Result<StoredMaterial> {
    let bundle = bundle_toml(identity)?;
    match passphrase {
        Some(passphrase) => Ok(StoredMaterial {
            text: encrypt_secret(bundle.as_bytes(), passphrase)?,
            encrypted: true,
        }),
        None => Ok(StoredMaterial {
            text: bundle,
            encrypted: false,
        }),
    }
}

struct StoredMaterial {
    text: Zeroizing<String>,
    encrypted: bool,
}

fn bundle_toml(identity: &UnlockedIdentity) -> Result<Zeroizing<String>> {
    let bundle = SecretBundle {
        schema_version: SECRET_BUNDLE_SCHEMA_VERSION,
        x25519: Zeroizing::new(identity.identity.to_string().expose_secret().to_string()),
        ed25519: Zeroizing::new(hex::encode(identity.signing.to_bytes())),
    };
    Ok(Zeroizing::new(toml::to_string(&bundle)?))
}

fn wire_for(name: &str, key: &StoredKey) -> StoredIdentityWire {
    // One match, so "exactly one of these fields is set" holds by construction.
    // The inline fields say which format they hold, so `keys_encrypted` is
    // theirs to leave absent.
    let mut wire = StoredIdentityWire {
        schema_version: IDENTITY_SCHEMA_VERSION,
        device_name: name.to_string(),
        encrypted_keys: None,
        plain_keys: None,
        keyring_account: None,
        keys_command: None,
        keys_store_command: None,
        keys_encrypted: None,
    };
    match &key.location {
        KeyLocation::Inline(text) if key.encrypted => wire.encrypted_keys = Some(text.clone()),
        KeyLocation::Inline(text) => wire.plain_keys = Some(text.clone()),
        KeyLocation::Keyring(account) => {
            wire.keyring_account = Some(account.clone());
            wire.keys_encrypted = Some(key.encrypted);
        }
        KeyLocation::Command { read, write } => {
            wire.keys_command = Some(read.clone());
            wire.keys_store_command = write.clone();
            wire.keys_encrypted = Some(key.encrypted);
        }
    }
    wire
}

fn write_identity_file(path: &Path, wire: &StoredIdentityWire) -> Result<()> {
    // The serialized document carries the plaintext key bundle in the inline
    // no-passphrase case; zeroize the buffer once it's on disk.
    let serialized = Zeroizing::new(toml::to_string_pretty(wire)?);
    atomic_write_private(path, serialized.as_bytes())
}

/// Read the identity file only to describe it, carrying the permission verdict
/// rather than failing on it: the file worth reporting on is exactly the one too
/// loose to run a command out of, so this must not be the call that refuses.
fn inspect_stored_identity(path: &Path) -> Result<StoredIdentity> {
    let (mut file, exposure) = files::open_private(path)?;
    // The raw file carries the plaintext key bundle in the inline no-passphrase
    // case: zeroize the buffer, and don't let toml::de::Error echo a line of it.
    // Our own field validation runs after the parse, so it reports accurately
    // instead of being flattened into a serde error.
    // Sized up front so it is allocated once — a string that grew by realloc
    // leaves copies `Zeroizing` cannot reach. Read through the handle that was
    // judged, so nothing can be swapped in between.
    let hint = usize::try_from(file.metadata()?.len()).unwrap_or(0);
    let mut raw = Zeroizing::new(String::with_capacity(hint.saturating_add(1)));
    file.read_to_string(&mut raw)?;

    let wire: StoredIdentityWire =
        toml::from_str(&raw).map_err(|error| EncryptionError::UnparsableIdentityFile {
            path: path.to_path_buf(),
            line: error_line(&raw, &error),
        })?;
    StoredIdentity::from_wire(wire, exposure)
}

/// Read it for an operation that may end up running a command it names, refusing
/// when the file's permissions leave that choice open to someone else.
///
/// Everything that fetches, stores or rewrites the key comes through here; only
/// [`device_identity_info`] does not, because describing a file is not running
/// what it says.
fn read_stored_identity(path: &Path) -> Result<StoredIdentity> {
    let stored = inspect_stored_identity(path)?;
    // Only a key command is arbitrary code. Refusing an inline or keychain key
    // would lock the user out over a bit they can fix, while doing nothing about
    // a key already readable.
    match (&stored.key.location, stored.exposure) {
        (
            KeyLocation::Command { .. },
            Some(
                exposure @ (PrivateFileExposure::ForeignOwner { .. }
                | PrivateFileExposure::Writable { .. }),
            ),
        ) => Err(EncryptionError::UnsafeIdentityFile {
            path: path.to_path_buf(),
            exposure,
        }),
        _ => Ok(stored),
    }
}

/// The 1-based line a parse error points at.
///
/// `toml::de::Error`'s `Display` quotes the offending line, which may be the
/// key, so it is never passed on. The span is only an offset.
fn error_line(raw: &str, error: &toml::de::Error) -> usize {
    error.span().map_or(1, |span| {
        // Counting newlines rather than `lines()`: an offset sitting exactly at a
        // line start — where an unknown-key span points — belongs to the line it
        // begins, not the one before. Bytes, so a span landing mid-character
        // cannot panic on a slice boundary.
        raw.as_bytes()[..span.start.min(raw.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1
    })
}

fn encrypt_secret(plaintext: &[u8], passphrase: &SecretString) -> Result<Zeroizing<String>> {
    let recipient = age::scrypt::Recipient::new(passphrase.clone());
    Ok(Zeroizing::new(age::encrypt_and_armor(
        &recipient, plaintext,
    )?))
}
