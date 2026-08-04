use crate::AppResult;

use super::{NewKeyStore, prompts};
use anyhow::bail;
use indicatif::{ProgressBar, ProgressStyle};
use notema_encryption::SecretString;
use notema_storage::JournalStore;

/// A progress sink for CLI migrations that drives an `indicatif` bar. A fresh
/// bar is created at the start of each pass (a `(0, total)` tick) — so a
/// two-pass operation like rotation shows a bar per pass — and cleared when the
/// pass completes. `unit` labels the counter (e.g. `files`, `entries`).
pub(crate) fn cli_progress(unit: &'static str) -> impl FnMut(usize, usize) {
    let mut bar: Option<ProgressBar> = None;
    move |done, total| {
        if done == 0 {
            let fresh = ProgressBar::new(total as u64);
            fresh.set_style(
                ProgressStyle::with_template(&format!("{{bar:40}} {{pos}}/{{len}} {unit}"))
                    .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
            bar = Some(fresh);
        }
        if let Some(bar) = &bar {
            bar.set_position(done as u64);
            if total == 0 || done >= total {
                bar.finish_and_clear();
            }
        }
    }
}

/// Where a newly minted key goes: an explicit `--key-store` wins, otherwise ask
/// (and, with no terminal to ask on, keep it in the identity file).
pub(crate) fn resolve_key_store(explicit: Option<NewKeyStore>) -> AppResult<NewKeyStore> {
    match explicit {
        Some(store) => Ok(store),
        None if prompts::prompt_keyring_choice(notema_encryption::keyring_available())? => {
            Ok(NewKeyStore::Keyring)
        }
        None => Ok(NewKeyStore::File),
    }
}

/// Move a freshly minted key into the OS keychain.
///
/// Minted inline then moved, so the move can verify the keychain hands the key
/// back. `requested` is whether the user named the keychain themselves: an
/// instruction that cannot be carried out fails the command, while a prompted or
/// defaulted choice falls back to the identity file.
pub(crate) fn move_key_to_keyring(
    store: &JournalStore,
    passphrase: Option<&SecretString>,
    requested: bool,
) -> AppResult<()> {
    let Err(error) = store.set_key_store(&notema_encryption::KeyTarget::Keyring, passphrase) else {
        return Ok(());
    };
    if requested {
        return Err(error);
    }
    eprintln!(
        "Could not move this device's key to the keychain: {error}\nIt is in {} instead; move it later with `{}`.",
        store.identity_path().display(),
        crate::KEY_STORE_KEYRING_CMD,
    );
    Ok(())
}

/// Tell the user what to back up. Naming the identity file for a key that only
/// points at the keychain would hand them a backup that decrypts nothing.
pub(crate) fn print_backup_advice(store: &JournalStore) -> AppResult<()> {
    match store.this_device()? {
        Some(info) if info.store != notema_encryption::KeyStore::File => println!(
            "This device's key is {}. Back it up with `{} <path>`; without it encrypted journal files cannot be decrypted.",
            info.store.whereabouts(),
            crate::EXPORT_KEY_CMD,
        ),
        _ => println!(
            "Identity file: {}. Back it up; without it encrypted journal files cannot be decrypted.",
            store.identity_path().display()
        ),
    }
    Ok(())
}

pub(crate) fn encrypt_store(
    store: &JournalStore,
    device_name: Option<&str>,
    no_passphrase: bool,
    key_store: Option<NewKeyStore>,
) -> AppResult<()> {
    let (recipient, warnings, minted_without_passphrase) = if store.encryption_enabled() {
        if !store.unlock_available() {
            bail!(
                "this journal is already encrypted for other devices, but this one has no key at {}; run `{}` to request access instead",
                store.identity_path().display(),
                crate::ENROLL_CMD,
            );
        }
        let recipient = store.public_recipient()?;
        let summary = store.encrypt_store(cli_progress("files"))?;
        (recipient, summary.warnings, false)
    } else if store.has_encrypted_entries()? {
        // Encrypted entries but no roster to encrypt more against — surface the
        // storage layer's own typed error rather than restating its message here.
        return Err(notema_encryption::EncryptionError::RecipientsMissing {
            path: store.device_roster_path().to_path_buf(),
        }
        .into());
    } else {
        if store.unlock_available() {
            bail!(
                "a device key already exists at {} but this journal has no device roster; enabling encryption would overwrite it. If this device is waiting to join an encrypted journal, let the journal folder finish syncing, then run `{}`; otherwise move the key file aside and re-run.",
                store.identity_path().display(),
                crate::ENROLL_CMD,
            );
        }
        println!("No journal encryption identity configured; generating an age identity.");
        let (name, passphrase) = prompts::resolve_new_identity_options(device_name, no_passphrase)?;
        let target = resolve_key_store(key_store)?;
        let summary = store.enable_encryption(&name, passphrase.as_ref(), cli_progress("files"))?;
        if target == NewKeyStore::Keyring {
            move_key_to_keyring(store, passphrase.as_ref(), key_store.is_some())?;
        }
        (summary.recipient, summary.warnings, passphrase.is_none())
    };

    println!("Encrypted journal store at {}", store.root().display());
    println!("Encryption recipient: {recipient}.");
    print_backup_advice(store)?;
    if minted_without_passphrase {
        println!("This key has no passphrase — keep this device and its backups secure.");
    }
    super::print_warnings(&warnings);
    Ok(())
}

/// Decrypt every entry and retire this device's key. The caller has already
/// validated the encryption state, confirmed, and unlocked the store.
pub(crate) fn decrypt_store(store: JournalStore) -> AppResult<()> {
    let summary = store.decrypt_store(cli_progress("files"))?;
    println!("Decrypted journal store at {}", store.root().display());
    if let Some(backup) = summary.backup_path {
        println!("Backup written to {}", backup.display());
    }
    println!(
        "Disabled age identity at {}",
        summary.disabled_identity_file.display()
    );
    if let Some(trust) = summary.disabled_trust_file {
        println!("Retired device trust pins to {}", trust.display());
    }
    if summary.key_left_in_command_store {
        println!(
            "The key is also still wherever the fetch command read it from; remove it there if you no longer want it."
        );
    }
    super::print_warnings(&summary.warnings);
    Ok(())
}
