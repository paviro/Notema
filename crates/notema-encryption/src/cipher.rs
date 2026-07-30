use crate::identity::UnlockedIdentity;
use crate::recipients::{Recipient, read_recipients};
use crate::{EncryptionError, KeyPaths, Result};
use age::x25519;
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    str::FromStr,
    string::FromUtf8Error,
};
use zeroize::Zeroizing;

/// Decrypted bytes. The inner buffer is zeroized on drop and intentionally does
/// not implement `Clone`, `Debug`, `Display`, or serde traits.
pub struct PlaintextBytes(Zeroizing<Vec<u8>>);

impl PlaintextBytes {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        Self::from_vec(bytes.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn copy_to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Convert decrypted UTF-8 bytes into a `String`. The returned `String` is
    /// plaintext and is not zeroized by this wrapper.
    pub fn into_string(self) -> std::result::Result<String, FromUtf8Error> {
        String::from_utf8(self.copy_to_vec())
    }
}

impl AsRef<[u8]> for PlaintextBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Encrypted age payload bytes. Kept distinct from plaintext so callers cannot
/// accidentally pass decrypted data where ciphertext is expected, or vice versa.
pub struct CiphertextBytes(Vec<u8>);

impl CiphertextBytes {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for CiphertextBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

pub fn encrypt_to_file(paths: &KeyPaths, plaintext: &PlaintextBytes, output: &Path) -> Result<()> {
    EncryptionRecipients::for_store(paths)?.encrypt_to_file(plaintext, output)?;
    Ok(())
}

/// Decrypt ciphertext the caller has already read, so one read can serve both
/// decryption and whatever else the caller needs the stored bytes for.
pub fn decrypt_bytes(
    identity: &UnlockedIdentity,
    ciphertext: &CiphertextBytes,
) -> Result<PlaintextBytes> {
    decrypt_bytes_with_identity(ciphertext, &identity.identity)
}

/// Decrypt an encrypted file into memory. Used both for reading encrypted entry
/// text and for viewing encrypted binary assets (e.g. images) without ever
/// writing a plaintext copy to disk.
pub fn decrypt_file_bytes(identity: &UnlockedIdentity, input: &Path) -> Result<PlaintextBytes> {
    decrypt_bytes(identity, &CiphertextBytes::from_vec(fs::read(input)?))
}

/// Open an encrypted file as a streaming plaintext reader, decrypting on demand
/// as the caller reads. Unlike [`decrypt_file_bytes`], the full plaintext is
/// never buffered in memory, so this stays constant-memory for large files.
///
/// Zeroization: `age` decrypts each chunk into a `SecretSlice` that is zeroized
/// on drop, so the plaintext held *inside* the reader is scrubbed. What is not
/// scrubbed is whatever buffer the caller reads bytes *into* — this returns a
/// plain `impl Read`, not a [`PlaintextBytes`], so the caller owns that. Use it
/// for copy-through work (re-encryption, migration) where the destination is
/// itself another encrypted stream; prefer [`decrypt_file_bytes`] when you need
/// the decrypted bytes to live in a zeroized buffer.
pub fn decrypt_file_reader(identity: &UnlockedIdentity, input: &Path) -> Result<impl Read> {
    let file = io::BufReader::new(fs::File::open(input)?);
    let decryptor = age::Decryptor::new_buffered(file)?;
    let reader = decryptor.decrypt(std::iter::once(&identity.identity as &dyn age::Identity))?;
    Ok(reader)
}

const AGE_V1_MAGIC: &[u8] = b"age-encryption.org/v1\n";
/// STREAM payload layout: a 16-byte nonce, then 64 KiB plaintext chunks each
/// followed by a 16-byte Poly1305 tag; the final chunk may be short, and is
/// empty only when the whole plaintext is empty.
const STREAM_NONCE_LEN: u64 = 16;
const CHUNK_TAG_LEN: u64 = 16;
const CHUNK_CIPHERTEXT_LEN: u64 = 64 * 1024 + CHUNK_TAG_LEN;
/// A binary age header is a few hundred bytes per recipient stanza; a header
/// that hasn't ended within this window is not one this store wrote.
const HEADER_SCAN_LIMIT: u64 = 64 * 1024;

/// Exact plaintext length of a binary age v1 file, derived from the header and
/// the ciphertext length alone — no identity, no decryption, and at most one
/// 64 KiB read.
///
/// # Errors
///
/// Fails on I/O errors and on any file that is not well-formed binary age v1
/// (wrong magic, unterminated header, or a payload too short to hold the
/// mandatory final chunk); callers needing the size of such a file must fall
/// back to decrypting it.
pub fn encrypted_plaintext_len(path: &Path) -> Result<u64> {
    let malformed = |detail| EncryptionError::MalformedAgeFile { detail };
    let mut file = fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut head = Vec::with_capacity(HEADER_SCAN_LIMIT.min(file_len) as usize);
    Read::take(&mut file, HEADER_SCAN_LIMIT).read_to_end(&mut head)?;
    if !head.starts_with(AGE_V1_MAGIC) {
        return Err(malformed("missing age v1 magic"));
    }
    // The header ends with the MAC line `--- <b64>`. No other header line can
    // start that way: stanzas open with `-> ` and their body lines are base64,
    // which has no `-`.
    let mut line_start = AGE_V1_MAGIC.len();
    let header_len = loop {
        let Some(newline) = head[line_start..].iter().position(|&b| b == b'\n') else {
            return Err(malformed("unterminated header"));
        };
        let next = line_start + newline + 1;
        if head[line_start..next].starts_with(b"--- ") {
            break next as u64;
        }
        line_start = next;
    };
    let payload = file_len
        .checked_sub(header_len + STREAM_NONCE_LEN)
        .ok_or(malformed("payload shorter than the STREAM nonce"))?;
    if payload < CHUNK_TAG_LEN {
        return Err(malformed("payload shorter than the mandatory final chunk"));
    }
    payload
        .div_ceil(CHUNK_CIPHERTEXT_LEN)
        .checked_mul(CHUNK_TAG_LEN)
        .and_then(|tags| payload.checked_sub(tags))
        .ok_or(malformed("payload inconsistent with chunk layout"))
}

/// Encrypt bytes to every store recipient.
pub fn encrypt_bytes(paths: &KeyPaths, plaintext: &PlaintextBytes) -> Result<CiphertextBytes> {
    EncryptionRecipients::for_store(paths)?.encrypt(plaintext)
}

/// Encrypt a freshly created entry to every store recipient, plus this device's
/// own key when unlocked — so the authoring device can always re-read what it
/// wrote, even a joining device whose key isn't yet an approved recipient.
pub fn encrypt_new_entry(
    paths: &KeyPaths,
    plaintext: &PlaintextBytes,
    identity: Option<&UnlockedIdentity>,
) -> Result<CiphertextBytes> {
    EncryptionRecipients::for_new_entry(paths, identity)?.encrypt(plaintext)
}

/// Store recipients parsed into age recipient keys, ready for repeated
/// encryptions without rereading the roster.
pub struct EncryptionRecipients {
    recipients: Vec<x25519::Recipient>,
}

impl EncryptionRecipients {
    pub fn for_store(paths: &KeyPaths) -> Result<Self> {
        Ok(Self {
            recipients: recipient_keys(&read_recipients(paths)?)?,
        })
    }

    pub fn for_new_entry(paths: &KeyPaths, identity: Option<&UnlockedIdentity>) -> Result<Self> {
        let mut recipients = recipient_keys(&read_recipients(paths)?)?;
        if let Some(identity) = identity {
            let own = identity.recipient();
            if !recipients.iter().any(|r| r.to_string() == own.to_string()) {
                recipients.push(own);
            }
        }
        Ok(Self { recipients })
    }

    pub fn encrypt(&self, plaintext: &PlaintextBytes) -> Result<CiphertextBytes> {
        encrypt_to_recipients(&self.recipients, plaintext)
    }

    /// Encrypt a stream to every store recipient without buffering the full
    /// plaintext or ciphertext in memory.
    pub fn encrypt_reader<R, W>(&self, plaintext: R, output: W) -> Result<W>
    where
        R: Read,
        W: Write,
    {
        encrypt_reader_to_recipients(&self.recipients, plaintext, output)
    }

    pub fn encrypt_to_file(&self, plaintext: &PlaintextBytes, output: &Path) -> Result<()> {
        let ciphertext = self.encrypt(plaintext)?;
        crate::files::atomic_write(output, ciphertext.as_bytes())
    }

    /// Encrypt a plaintext stream directly to `output` via an atomic temp+rename,
    /// buffering neither the full plaintext nor the full ciphertext in memory.
    /// The temp file only ever holds ciphertext.
    pub fn encrypt_reader_to_file<R: Read>(&self, plaintext: R, output: &Path) -> Result<()> {
        crate::files::atomic_write_with(output, false, |file| {
            encrypt_reader_to_recipients(&self.recipients, plaintext, file)?;
            Ok(())
        })
    }
}

fn recipient_keys(recipients: &[Recipient]) -> Result<Vec<x25519::Recipient>> {
    if recipients.is_empty() {
        return Err(EncryptionError::NoRecipients);
    }
    recipients
        .iter()
        .map(|recipient| {
            x25519::Recipient::from_str(&recipient.encryption_key).map_err(|_| {
                EncryptionError::InvalidRecipientKey {
                    key: recipient.encryption_key.clone(),
                }
            })
        })
        .collect()
}

pub(crate) fn encrypt_to_recipients(
    recipients: &[x25519::Recipient],
    plaintext: &PlaintextBytes,
) -> Result<CiphertextBytes> {
    let mut ciphertext = Vec::with_capacity(plaintext.as_bytes().len());
    encrypt_reader_to_recipients(recipients, plaintext.as_bytes(), &mut ciphertext)?;
    Ok(CiphertextBytes::from_vec(ciphertext))
}

fn encrypt_reader_to_recipients<R, W>(
    recipients: &[x25519::Recipient],
    mut plaintext: R,
    output: W,
) -> Result<W>
where
    R: Read,
    W: Write,
{
    let refs: Vec<&dyn age::Recipient> = recipients
        .iter()
        .map(|recipient| recipient as &dyn age::Recipient)
        .collect();
    let encryptor = age::Encryptor::with_recipients(refs.into_iter())?;
    let mut writer = encryptor.wrap_output(output)?;
    io::copy(&mut plaintext, &mut writer)?;
    Ok(writer.finish()?)
}

pub(crate) fn decrypt_bytes_with_identity(
    ciphertext: &CiphertextBytes,
    identity: &x25519::Identity,
) -> Result<PlaintextBytes> {
    Ok(PlaintextBytes::from_vec(age::decrypt(
        identity,
        ciphertext.as_bytes(),
    )?))
}
