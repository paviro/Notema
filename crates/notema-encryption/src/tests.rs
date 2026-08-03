use super::*;
use crate::cipher::decrypt_bytes_with_identity;
use std::{fs, path::Path};
use tempfile::tempdir;

fn paths_in(dir: &Path) -> KeyPaths {
    KeyPaths::for_config(&dir.join("config.toml"), &dir.join("journals")).unwrap()
}

#[test]
fn passphrase_identity_round_trips_a_message() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());

    initialize_store_identity(&paths, "laptop", Some(&SecretString::from("secret"))).unwrap();
    let unlocked = unlock_identity(&paths, Some(&SecretString::from("secret"))).unwrap();

    let plaintext = PlaintextBytes::copy_from_slice(b"hello journal");
    let ciphertext = encrypt_bytes(&paths, &plaintext).unwrap();
    assert_eq!(
        decrypt_file_bytes_from(&unlocked, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"hello journal"
    );
}

#[test]
fn wrong_passphrase_is_distinguishable_from_other_unlock_failures() {
    // UnlockedIdentity has no Debug (it's key material), so no unwrap_err.
    fn unlock_error(paths: &KeyPaths, passphrase: Option<&SecretString>) -> EncryptionError {
        match unlock_identity(paths, passphrase) {
            Err(error) => error,
            Ok(_) => panic!("expected the unlock to fail"),
        }
    }

    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", Some(&SecretString::from("secret"))).unwrap();

    let wrong = unlock_error(&paths, Some(&SecretString::from("wrong")));
    assert!(wrong.is_wrong_passphrase());

    let none = unlock_error(&paths, None);
    assert!(!none.is_wrong_passphrase());

    // A mangled identity file is a corruption error, not a bad passphrase.
    fs::write(&paths.identity_file, "not an identity").unwrap();
    let corrupt = unlock_error(&paths, Some(&SecretString::from("secret")));
    assert!(!corrupt.is_wrong_passphrase());
}

#[test]
fn streaming_encryption_round_trips_a_message() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", None).unwrap();
    let unlocked = unlock_identity(&paths, None).unwrap();
    let recipients = EncryptionRecipients::for_store(&paths).unwrap();
    let mut ciphertext = Vec::new();

    recipients
        .encrypt_reader(
            std::io::Cursor::new(b"streamed attachment"),
            &mut ciphertext,
        )
        .unwrap();

    let ciphertext = CiphertextBytes::from_vec(ciphertext);
    assert_eq!(
        decrypt_file_bytes_from(&unlocked, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"streamed attachment"
    );
}

#[test]
fn initialize_store_identity_refuses_an_existing_roster() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());

    initialize_store_identity(&paths, "laptop", None).unwrap();
    // A second genesis on the same roster would brick decryption; it must error.
    let err = initialize_store_identity(&paths, "laptop-again", None).unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn plaintext_identity_unlocks_without_a_passphrase() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());

    initialize_store_identity(&paths, "phone", None).unwrap();
    let info = device_identity_info(&paths).unwrap().unwrap();
    assert!(!info.passphrase_protected);

    let unlocked = unlock_identity(&paths, None).unwrap();
    let plaintext = PlaintextBytes::copy_from_slice(b"no passphrase");
    let ciphertext = encrypt_bytes(&paths, &plaintext).unwrap();
    assert_eq!(
        decrypt_file_bytes_from(&unlocked, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"no passphrase"
    );
}

#[test]
fn two_recipients_both_decrypt_the_same_ciphertext() {
    let dir = tempdir().unwrap();
    let laptop = paths_in(dir.path());
    // A second device with its own identity file but the same shared store.
    let phone = KeyPaths::for_config(
        &dir.path().join("phone").join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();

    initialize_store_identity(&laptop, "laptop", Some(&SecretString::from("pw"))).unwrap();
    let laptop_id = unlock_identity(&laptop, Some(&SecretString::from("pw"))).unwrap();
    let phone_recipient = request_store_access(&phone, "phone", None).unwrap();
    add_recipient(&laptop, &laptop_id, &phone_recipient).unwrap();
    advance_trust_pins(&laptop).unwrap();

    let plaintext = PlaintextBytes::copy_from_slice(b"shared secret");
    let ciphertext = encrypt_bytes(&laptop, &plaintext).unwrap();
    let phone_id = unlock_identity(&phone, None).unwrap();
    assert_eq!(
        decrypt_file_bytes_from(&laptop_id, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"shared secret"
    );
    assert_eq!(
        decrypt_file_bytes_from(&phone_id, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"shared secret"
    );
}

#[test]
fn pending_request_round_trips_and_clears() {
    let dir = tempdir().unwrap();
    let laptop = paths_in(dir.path());
    let phone = KeyPaths::for_config(
        &dir.path().join("phone").join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();

    initialize_store_identity(&laptop, "laptop", Some(&SecretString::from("pw"))).unwrap();
    request_store_access(&phone, "phone", None).unwrap();

    let pending = read_pending(&laptop).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].recipient.name, "phone");

    remove_pending(&laptop, &pending[0].id).unwrap();
    assert!(read_pending(&laptop).unwrap().is_empty());
}

#[test]
fn malformed_pending_request_is_skipped_not_fatal() {
    let dir = tempdir().unwrap();
    let laptop = paths_in(dir.path());
    let phone = KeyPaths::for_config(
        &dir.path().join("phone").join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();

    initialize_store_identity(&laptop, "laptop", Some(&SecretString::from("pw"))).unwrap();
    request_store_access(&phone, "phone", None).unwrap();

    // A junk file dropped into the synced folder (corrupt, forged, or a
    // sync-conflict copy) must not deny access — it is skipped, and the genuine
    // request still lists.
    fs::write(laptop.age_dir.join("pending-bad.toml"), "not valid = [").unwrap();
    let pending = read_pending(&laptop).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].recipient.name, "phone");
}

#[test]
fn add_recipient_rejects_duplicate_key_and_name() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let recipient =
        initialize_store_identity(&paths, "laptop", Some(&SecretString::from("pw"))).unwrap();
    let identity = unlock_identity(&paths, Some(&SecretString::from("pw"))).unwrap();

    // Same key → rejected for the key clash.
    assert!(add_recipient(&paths, &identity, &recipient).is_err());
    // Same name, different (valid) key → rejected for the name clash.
    let same_name_new_key = Recipient {
        encryption_key: "age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsuaxjx".to_string(),
        ..recipient
    };
    assert!(add_recipient(&paths, &identity, &same_name_new_key).is_err());
}

#[test]
fn revoke_recipient_refuses_the_last_one() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", Some(&SecretString::from("pw"))).unwrap();
    let identity = unlock_identity(&paths, Some(&SecretString::from("pw"))).unwrap();

    assert!(revoke_recipient(&paths, &identity, "laptop").is_err());
}

#[test]
fn set_identity_passphrase_toggles_protection_without_changing_the_key() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", None).unwrap();
    let key = unlock_identity(&paths, None).unwrap().public_key();
    assert!(
        !device_identity_info(&paths)
            .unwrap()
            .unwrap()
            .passphrase_protected
    );

    // Add a passphrase.
    set_identity_passphrase(&paths, None, Some(&SecretString::from("pw"))).unwrap();
    assert!(
        device_identity_info(&paths)
            .unwrap()
            .unwrap()
            .passphrase_protected
    );
    assert_eq!(
        unlock_identity(&paths, Some(&SecretString::from("pw")))
            .unwrap()
            .public_key(),
        key
    );

    // The wrong current passphrase is rejected.
    assert!(set_identity_passphrase(&paths, Some(&SecretString::from("wrong")), None).is_err());

    // Remove the passphrase again; the keypair is unchanged throughout.
    set_identity_passphrase(&paths, Some(&SecretString::from("pw")), None).unwrap();
    assert!(
        !device_identity_info(&paths)
            .unwrap()
            .unwrap()
            .passphrase_protected
    );
    assert_eq!(unlock_identity(&paths, None).unwrap().public_key(), key);
}

#[test]
fn stored_identity_rejects_unknown_fields() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", Some(&SecretString::from("secret"))).unwrap();

    let text = fs::read_to_string(&paths.identity_file).unwrap();
    fs::write(
        &paths.identity_file,
        format!("unexpected = \"unused\"\n{text}"),
    )
    .unwrap();

    assert!(unlock_identity(&paths, Some(&SecretString::from("secret"))).is_err());
}

#[test]
fn duplicate_names_make_revoke_and_rename_ambiguous() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", None).unwrap();
    let identity = unlock_identity(&paths, None).unwrap();

    // A second, uniquely named device so revoking "laptop" isn't last-recipient.
    let phone = KeyPaths::for_config(
        &dir.path().join("phone").join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();
    let phone_recipient = request_store_access(&phone, "phone", None).unwrap();
    add_recipient(&paths, &identity, &phone_recipient).unwrap();

    // A rotation that never dropped the old key: the ghost shares "laptop".
    rotate_add_new_key(&paths, &identity).unwrap();

    let error = revoke_recipient(&paths, &identity, "laptop").unwrap_err();
    assert!(matches!(
        error,
        EncryptionError::AmbiguousRecipientName { .. }
    ));
    let error = rename_recipient(&paths, &identity, "laptop", "desk").unwrap_err();
    assert!(matches!(
        error,
        EncryptionError::AmbiguousRecipientName { .. }
    ));

    // A unique name still resolves.
    rename_recipient(&paths, &identity, "phone", "mobile").unwrap();
}

#[test]
fn renew_access_reuses_the_existing_key() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    initialize_store_identity(&paths, "laptop", None).unwrap();

    let phone_dir = dir.path().join("phone");
    let phone =
        KeyPaths::for_config(&phone_dir.join("config.toml"), &dir.path().join("journals")).unwrap();
    let first = request_store_access(&phone, "phone", None).unwrap();

    // The request is denied (deleted) but the identity remains; renewing must
    // produce a verifiable request for the same key.
    let id = read_pending(&paths).unwrap().remove(0).id;
    remove_pending(&paths, &id).unwrap();
    assert!(read_pending(&paths).unwrap().is_empty());

    let unlocked = unlock_identity(&phone, None).unwrap();
    let renewed = renew_store_access(&phone, "phone", &unlocked).unwrap();
    assert_eq!(renewed.encryption_key, first.encryption_key);
    assert_eq!(renewed.signing_key, first.signing_key);

    // read_pending drops requests whose self-signature fails, so surviving the
    // read proves the renewed request verifies.
    let pending = read_pending(&paths).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].recipient.encryption_key, first.encryption_key);
}

#[test]
fn malformed_identity_file_error_does_not_echo_contents() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    fs::create_dir_all(paths.identity_file.parent().unwrap()).unwrap();
    fs::write(
        &paths.identity_file,
        "AGE-SECRET-KEY-MARKER = broken [ toml",
    )
    .unwrap();

    let error = match unlock_identity(&paths, None) {
        Ok(_) => panic!("malformed identity must not unlock"),
        Err(error) => error,
    };
    assert!(matches!(error, EncryptionError::MalformedStoredIdentity));
    assert!(
        !error.to_string().contains("MARKER"),
        "parse error must not echo the identity file: {error}"
    );
}

#[test]
fn malformed_plain_secret_bundle_error_does_not_echo_secret() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    fs::create_dir_all(paths.identity_file.parent().unwrap()).unwrap();
    // A valid wire document whose embedded plaintext bundle is broken TOML:
    // exercises the bundle parse without scrypt.
    fs::write(
        &paths.identity_file,
        "schema_version = 1\ndevice_name = \"laptop\"\nplain_keys = \"x25519 = 'MARKER' broken [ toml\"\n",
    )
    .unwrap();

    let error = match unlock_identity(&paths, None) {
        Ok(_) => panic!("malformed bundle must not unlock"),
        Err(error) => error,
    };
    assert!(matches!(error, EncryptionError::MalformedStoredIdentity));
    assert!(
        !error.to_string().contains("MARKER"),
        "parse error must not echo the secret bundle: {error}"
    );
}

#[test]
fn a_bare_age_key_instead_of_a_bundle_says_so() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    fs::create_dir_all(paths.identity_file.parent().unwrap()).unwrap();
    // The shape a secret manager already holds, and what someone hand-editing
    // the file reaches for. It deserves better than "malformed".
    fs::write(
        &paths.identity_file,
        "schema_version = 1\ndevice_name = \"laptop\"\nplain_keys = \"AGE-SECRET-KEY-1QQQQQQMARKER\"\n",
    )
    .unwrap();

    let error = unlock_identity(&paths, None)
        .err()
        .expect("must not unlock");
    assert!(matches!(error, EncryptionError::BareAgeKeyNotBundle));
    assert!(
        !error.to_string().contains("MARKER"),
        "the error must not echo the key: {error}"
    );
}

/// Decrypt an in-memory ciphertext with an unlocked identity (test helper;
/// the production path decrypts files, not buffers).
fn decrypt_file_bytes_from(
    identity: &UnlockedIdentity,
    ciphertext: &CiphertextBytes,
) -> Result<PlaintextBytes> {
    decrypt_bytes_with_identity(ciphertext, &identity.identity)
}

#[test]
fn plaintext_len_matches_roundtrip_at_chunk_boundaries() {
    let dir = tempdir().unwrap();
    let keys: Vec<age::x25519::Identity> =
        (0..2).map(|_| age::x25519::Identity::generate()).collect();
    let recipients: Vec<age::x25519::Recipient> = keys.iter().map(|k| k.to_public()).collect();
    // Sizes straddling every STREAM chunk edge: empty, first chunk, second.
    for size in [0usize, 1, 65_535, 65_536, 65_537, 131_072, 131_073] {
        for count in [1, 2] {
            let plaintext = PlaintextBytes::from_vec(vec![b'x'; size]);
            let ciphertext =
                crate::cipher::encrypt_to_recipients(&recipients[..count], &plaintext).unwrap();
            let path = dir.path().join(format!("s{size}-r{count}.age"));
            fs::write(&path, ciphertext.as_bytes()).unwrap();
            assert_eq!(
                encrypted_plaintext_len(&path).unwrap(),
                size as u64,
                "size {size} with {count} recipient(s)"
            );
        }
    }
}

#[test]
fn plaintext_len_rejects_non_age_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("garbage.age");
    fs::write(&path, b"not an age file at all").unwrap();
    assert!(matches!(
        encrypted_plaintext_len(&path),
        Err(EncryptionError::MalformedAgeFile { .. })
    ));
    // A header that never reaches its MAC line must also be refused.
    let truncated = dir.path().join("truncated.age");
    fs::write(&truncated, b"age-encryption.org/v1\n-> X25519 abc\n").unwrap();
    assert!(matches!(
        encrypted_plaintext_len(&truncated),
        Err(EncryptionError::MalformedAgeFile { .. })
    ));
}

// -- key command runner -------------------------------------------------------

/// A portable "print this file" command, so these tests don't need `op` or
/// `pass` installed.
fn cat_command(path: &Path) -> KeyCommand {
    if cfg!(windows) {
        KeyCommand::Argv(vec![
            "cmd".into(),
            "/C".into(),
            "type".into(),
            path.display().to_string(),
        ])
    } else {
        KeyCommand::Argv(vec!["cat".into(), path.display().to_string()])
    }
}

#[test]
fn key_command_returns_stdout_verbatim() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bundle.toml");
    fs::write(&file, b"schema_version = 1\n").unwrap();

    let out = key_command::run(&cat_command(&file)).unwrap();
    assert_eq!(out.as_slice(), b"schema_version = 1\n");
}

#[test]
fn missing_key_command_program_names_the_program() {
    let command = KeyCommand::Argv(vec!["notema-no-such-program-xyz".into()]);
    let error = key_command::run(&command).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("notema-no-such-program-xyz"), "{message}");
    assert!(message.contains("PATH"), "{message}");
}

#[test]
fn empty_key_command_is_rejected() {
    assert!(matches!(
        key_command::run(&KeyCommand::Argv(vec![])).unwrap_err(),
        EncryptionError::EmptyKeyCommand
    ));
    assert!(matches!(
        key_command::run(&KeyCommand::Shell("   ".into())).unwrap_err(),
        EncryptionError::EmptyKeyCommand
    ));
}

#[cfg(unix)]
#[test]
fn failing_key_command_surfaces_stderr_without_echoing_key_material() {
    const MARKER: &str = "AGE-SECRET-KEY-1TESTMARKER";
    let command = KeyCommand::Shell(format!(
        "printf '{MARKER}\\nvault is locked\\n' >&2; exit 3"
    ));
    let error = key_command::run(&command).unwrap_err();

    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(display.contains("vault is locked"), "{display}");
    assert!(display.contains("exit status 3"), "{display}");
    assert!(
        !display.contains(MARKER),
        "stderr leaked the key: {display}"
    );
    assert!(!debug.contains(MARKER), "Debug leaked the key: {debug}");
}

#[cfg(unix)]
#[test]
fn key_command_output_is_bounded_rather_than_unbounded() {
    // A single process rather than a shell pipeline, so the kill on overflow
    // lands on the thing producing the output.
    let command = KeyCommand::Argv(vec![
        "head".into(),
        "-c".into(),
        "200000".into(),
        "/dev/zero".into(),
    ]);
    assert!(matches!(
        key_command::run(&command).unwrap_err(),
        EncryptionError::KeyCommandOutputTooLarge { .. }
    ));
}

#[cfg(unix)]
#[test]
fn key_command_never_ending_output_does_not_hang() {
    // `yes` never stops on its own: reaching the cap has to cut it off rather
    // than wait for an EOF that isn't coming.
    let command = KeyCommand::Argv(vec!["yes".into()]);
    assert!(matches!(
        key_command::run(&command).unwrap_err(),
        EncryptionError::KeyCommandOutputTooLarge { .. }
    ));
}

#[cfg(unix)]
#[test]
fn key_command_survives_a_chatty_stderr() {
    // More stderr than a pipe buffer holds: draining it concurrently is what
    // keeps this from deadlocking.
    let command = KeyCommand::Shell("head -c 200000 /dev/zero >&2; printf 'ok'".into());
    let out = key_command::run(&command).unwrap();
    assert_eq!(out.as_slice(), b"ok");
}

#[cfg(unix)]
#[test]
fn key_command_store_pipes_material_to_stdin() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("stored.toml");
    let command = KeyCommand::Shell(format!("cat > {}", file.display()));

    key_command::store(&command, b"schema_version = 1\n").unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"schema_version = 1\n");
}

#[cfg(unix)]
#[test]
fn failing_store_command_reports_its_status() {
    let error = key_command::store(&KeyCommand::Shell("exit 7".into()), b"x").unwrap_err();
    assert!(error.to_string().contains("exit status 7"), "{error}");
}

// -- key locations ------------------------------------------------------------

fn write_wire(paths: &KeyPaths, body: &str) {
    fs::create_dir_all(paths.identity_file.parent().unwrap()).unwrap();
    fs::write(&paths.identity_file, body).unwrap();
}

#[test]
fn naming_two_key_locations_names_both() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    write_wire(
        &paths,
        "schema_version = 1\ndevice_name = \"laptop\"\nplain_keys = \"SECRETMARKER\"\nkeys_command = \"cat /nope\"\n",
    );

    let error = device_identity_info(&paths).unwrap_err();
    let message = error.to_string();
    assert!(matches!(
        error,
        EncryptionError::AmbiguousKeyLocation { .. }
    ));
    assert!(message.contains("plain_keys"), "{message}");
    assert!(message.contains("keys_command"), "{message}");
    // The field names are ours; the values are the secret and must not appear.
    assert!(!message.contains("SECRETMARKER"), "{message}");
}

#[test]
fn naming_no_key_location_says_so() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    write_wire(&paths, "schema_version = 1\ndevice_name = \"laptop\"\n");

    assert!(matches!(
        device_identity_info(&paths).unwrap_err(),
        EncryptionError::NoKeyLocation
    ));
}

#[test]
fn keys_encrypted_beside_a_self_describing_field_is_rejected() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    write_wire(
        &paths,
        "schema_version = 1\ndevice_name = \"laptop\"\nplain_keys = \"x\"\nkeys_encrypted = true\n",
    );

    assert!(matches!(
        device_identity_info(&paths).unwrap_err(),
        EncryptionError::RedundantKeysEncrypted {
            field: "plain_keys"
        }
    ));
}

#[test]
fn an_unsupported_identity_schema_reports_the_version() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    write_wire(
        &paths,
        "schema_version = 99\ndevice_name = \"laptop\"\nplain_keys = \"x\"\n",
    );

    // Previously flattened into "key material is malformed", which read as
    // corruption rather than a version mismatch.
    assert!(matches!(
        device_identity_info(&paths).unwrap_err(),
        EncryptionError::UnsupportedSchema {
            kind: "device identity file",
            version: 99
        }
    ));
}

/// Move a fresh identity's key out to a command-backed file, the way the CLI
/// does: seed with `--store`, read back with `--read`.
fn move_to_command(paths: &KeyPaths, bundle: &Path, passphrase: Option<&SecretString>) {
    let target = KeyTarget::Command {
        read: cat_command(bundle),
        store: KeyCommand::Shell(format!("cat > {}", bundle.display())).into(),
    };
    set_key_location(paths, &target, passphrase).unwrap();
}

#[cfg(unix)]
#[test]
fn a_command_sourced_identity_unlocks_and_round_trips() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let bundle = dir.path().join("bundle.toml");

    initialize_store_identity(&paths, "laptop", None).unwrap();
    let before = unlock_identity(&paths, None).unwrap().public_key();
    move_to_command(&paths, &bundle, None);

    // The whole point: the key is no longer in the identity file.
    let on_disk = fs::read_to_string(&paths.identity_file).unwrap();
    assert!(!on_disk.contains("AGE-SECRET-KEY-"), "{on_disk}");
    assert!(on_disk.contains("keys_command"), "{on_disk}");

    let info = device_identity_info(&paths).unwrap().unwrap();
    assert_eq!(info.source, KeySource::Command);
    assert!(!info.passphrase_protected);

    let unlocked = unlock_identity(&paths, None).unwrap();
    assert_eq!(unlocked.public_key(), before);
    let ciphertext =
        encrypt_bytes(&paths, &PlaintextBytes::copy_from_slice(b"via command")).unwrap();
    assert_eq!(
        decrypt_file_bytes_from(&unlocked, &ciphertext)
            .unwrap()
            .as_bytes(),
        b"via command"
    );
}

#[cfg(unix)]
#[test]
fn a_command_sourced_identity_keeps_its_passphrase() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let bundle = dir.path().join("bundle.age");
    let pw = SecretString::from("pw");

    initialize_store_identity(&paths, "laptop", Some(&pw)).unwrap();
    move_to_command(&paths, &bundle, Some(&pw));

    // Format and location are independent: moving it kept the scrypt wrap, so
    // what landed outside is armor rather than a bare key.
    let stored = fs::read_to_string(&bundle).unwrap();
    assert!(stored.contains("BEGIN AGE ENCRYPTED FILE"), "{stored}");

    // Reported without fetching anything, so the unlock screen still knows to ask.
    let info = device_identity_info(&paths).unwrap().unwrap();
    assert!(info.passphrase_protected);
    assert_eq!(info.source, KeySource::Command);

    assert!(unlock_identity(&paths, None).is_err());
    assert!(unlock_identity(&paths, Some(&pw)).is_ok());
}

#[cfg(unix)]
#[test]
fn a_passphrase_change_writes_back_through_the_store_command() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let bundle = dir.path().join("bundle.toml");
    let pw = SecretString::from("pw");

    initialize_store_identity(&paths, "laptop", None).unwrap();
    let before = unlock_identity(&paths, None).unwrap().public_key();
    move_to_command(&paths, &bundle, None);

    // There is a store command, so re-wrapping has somewhere to put the result:
    // it must succeed and leave the key where the user chose to keep it.
    set_identity_passphrase(&paths, None, Some(&pw)).unwrap();

    let info = device_identity_info(&paths).unwrap().unwrap();
    assert_eq!(info.source, KeySource::Command);
    assert!(info.passphrase_protected);
    assert!(
        fs::read_to_string(&bundle)
            .unwrap()
            .contains("BEGIN AGE ENCRYPTED FILE"),
        "the re-wrapped key should have gone back through the store command"
    );
    assert!(
        !fs::read_to_string(&paths.identity_file)
            .unwrap()
            .contains("AGE-SECRET-KEY-"),
        "re-wrapping must not quietly pull the key back inline"
    );
    assert_eq!(
        unlock_identity(&paths, Some(&pw)).unwrap().public_key(),
        before
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_key_command_refuses_before_touching_anything() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let bundle = dir.path().join("bundle.toml");

    let recipient = initialize_store_identity(&paths, "laptop", None).unwrap();
    // No `--store`, so the key can be fetched but never replaced.
    set_key_location(
        &paths,
        &KeyTarget::Command {
            read: cat_command(&bundle),
            store: Some(KeyCommand::Shell(format!("cat > {}", bundle.display()))),
        },
        None,
    )
    .unwrap();
    let identity_toml = fs::read_to_string(&paths.identity_file).unwrap();
    fs::write(
        &paths.identity_file,
        identity_toml
            .lines()
            .filter(|line| !line.starts_with("keys_store_command"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let before = fs::read(&paths.identity_file).unwrap();

    let error = set_identity_passphrase(&paths, None, Some(&SecretString::from("pw"))).unwrap_err();
    assert!(matches!(error, EncryptionError::KeySourceReadOnly { .. }));

    let identity = unlock_identity(&paths, None).unwrap();
    assert!(matches!(
        commit_rotated_identity(&paths, &recipient, &identity, None).unwrap_err(),
        EncryptionError::KeySourceReadOnly { .. }
    ));

    assert_eq!(
        fs::read(&paths.identity_file).unwrap(),
        before,
        "a refused rewrite must leave the identity file untouched"
    );
}

#[cfg(unix)]
#[test]
fn a_switch_that_reads_back_a_different_key_is_refused() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let decoy = dir.path().join("decoy.toml");

    initialize_store_identity(&paths, "laptop", None).unwrap();
    let before = fs::read(&paths.identity_file).unwrap();

    // A store command that drops the key on the floor while a read command
    // serves someone else's: the readback check is what stops this becoming a
    // device that can never unlock again.
    let other = paths_in(&dir.path().join("other"));
    initialize_store_identity(&other, "other", None).unwrap();
    let other_bundle = fs::read_to_string(&other.identity_file).unwrap();
    let other_bundle: toml::Value = toml::from_str(&other_bundle).unwrap();
    fs::write(
        &decoy,
        other_bundle.get("plain_keys").unwrap().as_str().unwrap(),
    )
    .unwrap();

    let target = KeyTarget::Command {
        read: cat_command(&decoy),
        store: KeyCommand::Shell("cat > /dev/null".into()).into(),
    };
    assert!(matches!(
        set_key_location(&paths, &target, None).unwrap_err(),
        EncryptionError::KeySourceMismatch
    ));
    assert_eq!(fs::read(&paths.identity_file).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn an_exported_identity_restores_into_a_fresh_config_dir() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    let bundle = dir.path().join("bundle.toml");
    let backup = dir.path().join("backup.toml");

    initialize_store_identity(&paths, "laptop", None).unwrap();
    let before = unlock_identity(&paths, None).unwrap().public_key();
    move_to_command(&paths, &bundle, None);

    // Exporting inlines whatever the key currently is, so the backup stands on
    // its own even though the live identity points at a command.
    export_identity(&paths, &backup).unwrap();

    // Restoring is copying the file back — no import command in the middle.
    let restored = paths_in(&dir.path().join("restored"));
    fs::create_dir_all(restored.identity_file.parent().unwrap()).unwrap();
    fs::copy(&backup, &restored.identity_file).unwrap();

    let info = device_identity_info(&restored).unwrap().unwrap();
    assert_eq!(info.name, "laptop");
    assert_eq!(info.source, KeySource::File);
    assert_eq!(
        unlock_identity(&restored, None).unwrap().public_key(),
        before
    );
}
