use std::fmt;
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, EncryptionError>;

/// Whether a run of text carries something that reads like key material.
///
/// Used to keep command output and command lines out of error messages. Not a
/// security boundary — neither is expected to hold the secret in the first
/// place — but the obvious shapes are free to catch.
pub(crate) fn looks_secret(text: &str) -> bool {
    text.contains("AGE-SECRET-KEY-") || text.contains("-----BEGIN AGE")
}

/// The tail of a key command's stderr, carried in an error so a failure can say
/// what the command complained about.
///
/// `Display` prints it; `Debug` prints only its size. [`EncryptionError`] derives
/// `Debug`, so a command that echoed key material to stderr would otherwise leak
/// it through `{:?}` — the same rule the identity parse guards follow.
#[derive(Clone, PartialEq, Eq)]
pub struct CommandStderr(String);

impl CommandStderr {
    /// Keep the last `limit` bytes (the actual complaint is usually last) and drop
    /// lines that look like key material. Not a security boundary — stderr is the
    /// command's own output and isn't expected to carry the secret — but the
    /// obvious case is free to catch.
    pub(crate) fn new(raw: &str, limit: usize) -> Self {
        let tail = if raw.len() <= limit {
            raw
        } else {
            let mut start = raw.len() - limit;
            while !raw.is_char_boundary(start) {
                start += 1;
            }
            &raw[start..]
        };
        let kept: Vec<&str> = tail.lines().filter(|line| !looks_secret(line)).collect();
        Self(kept.join("\n").trim().to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for CommandStderr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            Ok(())
        } else {
            write!(f, ": {}", self.0)
        }
    }
}

impl fmt::Debug for CommandStderr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CommandStderr(<{} bytes>)", self.0.len())
    }
}

/// A failure in the encryption layer. The first three variants carry state a
/// caller acts on (prompt for a passphrase, refuse to continue); the domain
/// variants are validation failures surfaced to the user; the rest wrap an
/// underlying failure, typed rather than boxed.
#[derive(Debug, Error)]
pub enum EncryptionError {
    /// An encrypted item was accessed without an unlocked identity. `context` is
    /// a caller-supplied label for what needed the identity (e.g. `"entry"`,
    /// `"asset"`, `"approve"`).
    #[error("encrypted {context} requires an unlocked journal encryption identity")]
    Locked { context: &'static str },

    /// Encrypted entries exist but the signed device roster needed to encrypt
    /// more is gone — continuing could leave the store partially encrypted.
    #[error(
        "encrypted entries already exist but the device roster is missing at {}; cannot safely continue encryption",
        .path.display()
    )]
    RecipientsMissing { path: PathBuf },

    /// The signed device roster failed verification: a forged/unauthorized op, a
    /// broken signature chain, a changed genesis, or a rolled-back history. The
    /// store refuses to encrypt or decrypt to an untrusted recipient set rather
    /// than silently trusting the tampered file. `detail` explains which check
    /// failed.
    #[error("device roster failed verification: {detail}")]
    RosterUnverified { detail: String },

    /// A store already has a device roster, so it can't be initialized again
    /// (a second genesis would brick decryption for the existing devices).
    #[error("device roster already exists; use request_store_access to join instead")]
    RosterExists,

    /// An operation needed at least one recipient but the roster is empty.
    #[error("journal encryption recipients file is empty")]
    NoRecipients,

    /// A recipient with this age key is already on the roster.
    #[error("recipient '{name}' is already present")]
    RecipientExists { name: String },

    /// A recipient with this name is already on the roster.
    #[error("a recipient named '{name}' already exists; pick a unique name")]
    RecipientNameTaken { name: String },

    /// No recipient on the roster carries this name.
    #[error("no recipient named '{name}'")]
    UnknownRecipient { name: String },

    /// More than one recipient carries this name (a crashed rotation can leave
    /// a same-named ghost key), so a name-addressed change could pick the wrong
    /// device.
    #[error("more than one recipient is named '{name}'; refusing to guess which key is meant")]
    AmbiguousRecipientName { name: String },

    /// Revoking this recipient would leave the store with none, making it
    /// impossible to re-encrypt.
    #[error("cannot revoke the last recipient; the store would become unreadable")]
    LastRecipient,

    /// This device's key isn't a current recipient, so it can't rotate.
    #[error("this device is not a current recipient; cannot rotate")]
    NotARecipient,

    /// A recipient carries a malformed age (X25519) public key.
    #[error("'{key}' is not a valid age recipient")]
    InvalidRecipientKey { key: String },

    /// A recipient carries a malformed Ed25519 signing key.
    #[error("'{key}' is not a valid signing key")]
    InvalidSigningKey { key: String },

    /// A device name was blank.
    #[error("device name cannot be empty")]
    EmptyDeviceName,

    /// A recipient rename target was blank.
    #[error("recipient name cannot be empty")]
    EmptyRecipientName,

    /// A passphrase was blank.
    #[error("encryption passphrase cannot be empty")]
    EmptyPassphrase,

    /// The stored identity is passphrase-protected but no passphrase was given.
    #[error("journal identity is passphrase-protected; a passphrase is required")]
    PassphraseRequired,

    /// The unlocked identity failed its self round-trip check.
    #[error("journal encryption identity check failed")]
    IdentityCheckFailed,

    /// The stored identity's key material could not be parsed (wrong length or
    /// not a valid age key).
    #[error("journal identity key material is malformed")]
    MalformedStoredIdentity,

    /// The identity file names more than one key location, so there is no
    /// unambiguous key to load. `fields` lists the offending field names.
    #[error(
        "journal identity file names more than one key location ({fields}); it must name exactly one"
    )]
    AmbiguousKeyLocation { fields: String },

    /// The identity file names no key location at all.
    #[error(
        "journal identity file has no key material; expected one of plain_keys, encrypted_keys, keyring_account, keys_command"
    )]
    NoKeyLocation,

    /// `keys_encrypted` was set alongside an inline key field. The inline fields
    /// already say which format they hold, so the pair could contradict itself.
    #[error(
        "journal identity file sets keys_encrypted alongside {field}, which already implies the format"
    )]
    RedundantKeysEncrypted { field: &'static str },

    /// `keys_command` is present but names no program to run.
    #[error("keys_command in the journal identity file names no program to run")]
    EmptyKeyCommand,

    /// The key command could not be started — wrong path, not installed, or not
    /// executable. Kept distinct from a non-zero exit because it is the most
    /// common misconfiguration and needs a different fix.
    #[error("could not run the journal key command '{program}': {detail}")]
    KeyCommandSpawn { program: String, detail: String },

    /// The key command ran and exited non-zero.
    #[error("the journal key command '{program}' failed ({status}){stderr}")]
    KeyCommandFailed {
        program: String,
        status: String,
        stderr: CommandStderr,
    },

    /// The key command printed more than the secret bundle could plausibly be.
    #[error(
        "the journal key command printed more than {limit} bytes; expected the secret key bundle"
    )]
    KeyCommandOutputTooLarge { limit: usize },

    /// The stored key material is a bare age secret key rather than the whole
    /// bundle — the shape a secret manager most often already holds. Carries
    /// nothing: the offending text is the secret.
    #[error(
        "the stored key material is a bare age secret key, but the whole secret bundle is needed (schema_version, x25519, ed25519). Write one with `notema encryption device export-key`"
    )]
    BareAgeKeyNotBundle,

    /// A key-location switch read back a different key than this device uses, so
    /// the local copy was kept rather than stranding the device.
    #[error(
        "the new key location returned a different key than this device uses; refusing to switch"
    )]
    KeySourceMismatch,

    /// The key is fetched by a command with no matching store command, so it can
    /// be read but never replaced.
    #[error(
        "this device's key is fetched by '{command}', which can read it but not replace it. Re-run `notema encryption device key-source command` with `--store` so new key material has somewhere to go, or bring the key back with `notema encryption device key-source file`"
    )]
    KeySourceReadOnly { command: String },

    /// The identity file is not valid TOML, or does not have the shape of an
    /// identity file.
    ///
    /// Deliberately says nothing about *what* is on the line: the file may hold
    /// key material, and a parse that failed proves nothing about where. Distinct
    /// from [`Self::MalformedStoredIdentity`], which is about key material that
    /// was read successfully and then turned out not to be a key — this one is
    /// about the file around it, and is what a hand-edited `keys_command` or a
    /// truncated sync produces.
    #[error(
        "{} is not a readable identity file (parse failed at line {line}). If you edited it by hand, check that line; otherwise restore it from a backup",
        path.display()
    )]
    UnparsableIdentityFile { path: PathBuf, line: usize },

    /// `keys_store_command` without the `keys_command` it writes for.
    #[error(
        "the identity file has `keys_store_command` but no `keys_command`; a store command is the write half of a fetch command and does nothing on its own"
    )]
    OrphanedStoreCommand,

    /// No OS keyring is available on this platform or in this session.
    #[error("no OS keyring is available here: {detail}")]
    KeyringUnavailable { detail: String },

    /// The identity file points at a keyring item that isn't there.
    #[error("the OS keyring has no key for this device (account '{account}')")]
    KeyringItemMissing { account: String },

    /// The OS keyring refused or failed the request.
    #[error("the OS keyring request failed: {detail}")]
    KeyringFailed { detail: String },

    /// The config path has no parent directory to derive key locations from.
    #[error("config path has no parent directory")]
    MissingConfigParent,

    /// The OS randomness source failed while generating a signing key.
    #[error("failed to gather randomness for signing key: {0}")]
    Randomness(String),

    #[error("signed metadata field is too large ({length} bytes)")]
    SignedFieldTooLarge { length: usize },

    #[error("unsupported {kind} schema version {version}; expected 1")]
    UnsupportedSchema { kind: &'static str, version: u32 },
    /// A keyring is there but refused the request, typically because it is
    /// locked. Distinct from [`Self::KeyringUnavailable`]: the key is still
    /// where it should be, and unlocking the keychain is all that is needed.
    #[error("the OS keyring is locked or refused access: {detail}. Unlock it and try again")]
    KeyringLocked { detail: String },


    /// A file expected to be binary age v1 ciphertext has a header or length
    /// that doesn't fit the format, so its plaintext size cannot be derived.
    #[error("not a well-formed binary age v1 file: {detail}")]
    MalformedAgeFile { detail: &'static str },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("age encryption failed: {0}")]
    Encrypt(#[from] age::EncryptError),

    #[error("age decryption failed: {0}")]
    Decrypt(#[from] age::DecryptError),

    #[error("malformed encryption metadata: {0}")]
    TomlRead(#[from] toml::de::Error),

    #[error("could not serialize encryption metadata: {0}")]
    TomlWrite(#[from] toml::ser::Error),

    #[error("invalid hex encoding: {0}")]
    Hex(#[from] hex::FromHexError),
}

impl EncryptionError {
    pub fn is_no_matching_keys(&self) -> bool {
        matches!(self, Self::Decrypt(age::DecryptError::NoMatchingKeys))
    }

    /// Whether this failure means the supplied passphrase didn't open the
    /// scrypt-wrapped identity. Age's MAC can't tell a wrong passphrase from a
    /// corrupted payload, so this also matches the (far rarer) corruption case.
    pub fn is_wrong_passphrase(&self) -> bool {
        matches!(self, Self::Decrypt(age::DecryptError::DecryptionFailed))
    }
}
