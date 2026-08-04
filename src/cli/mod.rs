use crate::{AppResult, config, startup, tui};

mod backfill;
mod encryption;
mod import;
mod location;
mod log;
pub(crate) mod prompts;
#[cfg(feature = "fuse")]
use anyhow::Context;
use anyhow::bail;
use clap::{Args, Parser, Subcommand, ValueEnum};
use notema_encryption::{KeyTarget, PendingRequest, SecretString};
use notema_storage::JournalStore;
use notema_timing as timing;
#[cfg(feature = "fuse")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

#[derive(Debug, Parser)]
#[command(name = "notema")]
#[command(version, disable_version_flag = true)]
#[command(about = "Markdown terminal journal")]
struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long, action = clap::ArgAction::Version)]
    version: (),

    /// Config directory holding config.toml and this device's encryption key;
    /// defaults to $XDG_CONFIG_HOME/notema, else ~/.config/notema (macOS:
    /// ~/Library/Application Support/de.paviro.notema). Global, so it works
    /// before or after a subcommand.
    #[arg(long, value_name = "DIR", global = true, env = "NOTEMA_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Create a journal entry from text or stdin, or compose one in the editor
    Log(LogArgs),
    /// Set the default journal for new entries
    Use {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Import entries from another journaling app
    Import {
        #[command(subcommand)]
        source: ImportSource,
    },
    /// Fill in missing location names, weather, air quality, and celestial data
    /// for existing located entries (fetched on demand, one request per second)
    Backfill,
    /// Manage journal encryption: turn it on or off, the devices that can read
    /// it, and this device's own key
    #[command(alias = "enc")]
    Encryption {
        #[command(subcommand)]
        command: EncryptionCommand,
    },
    /// Show data-source attributions and third-party dependency licenses.
    /// Pass a dependency name to print its full license text.
    Licenses {
        /// Show the full license text for a specific dependency
        #[arg(value_name = "DEPENDENCY")]
        dependency: Option<String>,
    },
    /// Mount the journal as a decrypted, writable filesystem
    #[cfg(feature = "fuse")]
    Mount {
        /// Directory to mount at (created if missing). Omit to use a temporary
        /// directory — on macOS the journal still appears as a drive in Finder.
        #[arg(value_name = "MOUNTPOINT")]
        mountpoint: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum EncryptionCommand {
    /// Turn on encryption for this device (creating its key if needed) and encrypt every plaintext entry
    Enable(NewIdentityArgs),
    /// Decrypt every encrypted entry, turning encryption off
    Disable(ConfirmArgs),
    /// Show whether encryption is on, who can read this journal, and where this device's key is
    Status,
    /// Manage the devices that can read this encrypted journal
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Manage this device's own key: where it is kept, how it is protected
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
}

/// The roster: which devices may read this journal. Everything here is about
/// *other* devices, or about this one's place among them. This device's own key
/// is [`KeyCommand`].
#[derive(Debug, Subcommand)]
enum DeviceCommand {
    /// Request access for this device to an already-encrypted journal (approve it from an existing device)
    Enroll(NewIdentityArgs),
    /// List the devices that can read this journal, plus pending requests
    List,
    /// Revoke a device and re-encrypt all entries to exclude it
    Revoke {
        #[arg(value_name = "NAME")]
        name: String,
        #[command(flatten)]
        confirm: ConfirmArgs,
    },
    /// Rename a device's label (no re-encryption)
    Rename {
        #[arg(value_name = "OLD")]
        old: String,
        #[arg(value_name = "NEW")]
        new: String,
    },
    /// Approve pending device-access requests (add + re-encrypt)
    Approve(RequestSelectionArgs),
    /// Reject pending device-access requests without granting access
    Reject(RequestSelectionArgs),
}

/// This device's key: its *format* (whether a passphrase protects it) and its
/// *store* (which one holds the bytes). Every store holds either format.
///
/// All verbs; `encryption status` does the reporting.
#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// Change where this device's key is kept
    Store(KeyStoreArgs),
    /// Add, remove, or change this device's key passphrase
    Passphrase(PassphraseArgs),
    /// Replace this device's key and re-encrypt, retiring the old key
    Rotate,
    /// Write a standalone copy of this device's key, for safekeeping
    Export(ExportKeyArgs),
}

/// Where a device's key is kept. Independent of whether it is
/// passphrase-protected: every location holds either form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KeyStoreChoice {
    /// Inline in `identity.toml`, readable only by you
    File,
    /// An item in the operating system's keychain
    Keyring,
    /// Fetched by running a command, e.g. `op read` or `pass show`
    Command,
}

#[derive(Debug, Args)]
struct KeyStoreArgs {
    /// Where to keep the key
    #[arg(value_name = "LOCATION")]
    location: KeyStoreChoice,

    /// Command that prints the key on stdout. Required for `command`.
    #[arg(long, value_name = "COMMAND", required_if_eq("location", "command"))]
    read: Option<String>,

    /// Command that stores the key, given it on stdin. Recorded alongside
    /// `--read`; without it the key can be fetched but never replaced, so
    /// `rotate` and `passphrase` have nowhere to write.
    #[arg(long, value_name = "COMMAND", requires = "read")]
    write: Option<String>,
}

#[derive(Debug, Args)]
struct ExportKeyArgs {
    /// File to write the copy to
    #[arg(value_name = "PATH")]
    path: PathBuf,
    #[command(flatten)]
    confirm: ConfirmArgs,
}

#[derive(Debug, Args)]
struct PassphraseArgs {
    /// Remove the passphrase, storing the key unprotected
    #[arg(long)]
    remove: bool,
    #[command(flatten)]
    confirm: ConfirmArgs,
}

/// Shared `--yes`/`-y` flag that skips the confirmation prompt on a destructive
/// operation, for scripting and non-interactive use.
#[derive(Debug, Args)]
struct ConfirmArgs {
    /// Skip the confirmation prompt
    #[arg(long, short = 'y')]
    yes: bool,
}

/// Options for creating a new device identity, shared by `encryption enable`
/// (first key on this device) and `device enroll` (joining an existing store).
#[derive(Debug, Args)]
struct NewIdentityArgs {
    /// Name for this device when creating a new identity (prompted if omitted)
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// Create the key without a passphrase; it opens automatically. Omit to be
    /// asked interactively whether to protect the key with a passphrase.
    #[arg(long)]
    no_passphrase: bool,

    /// Where to keep the new key. Omit to be asked interactively; without a
    /// terminal to ask on, the key stays in the identity file.
    #[arg(long, value_name = "LOCATION")]
    key_store: Option<NewKeyStore>,
}

/// Where a *newly minted* key can go. `command` is absent deliberately: it needs
/// a fetch command to be named, which `notema encryption key store command`
/// exists to do once the key is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum NewKeyStore {
    /// Inline in `identity.toml`, readable only by you
    File,
    /// An item in the operating system's keychain
    Keyring,
}

/// Which pending join requests a command acts on. Shared by `approve` and
/// `reject`: name/id selects one, `--all` selects every queued request.
#[derive(Debug, Args)]
struct RequestSelectionArgs {
    /// Act only on the request whose name or id matches
    #[arg(value_name = "NAME_OR_ID")]
    which: Option<String>,

    /// Act on every pending request
    #[arg(long, conflicts_with = "which")]
    all: bool,
}

#[derive(Debug, Subcommand)]
enum ImportSource {
    /// Import a Day One JSON export (with photos)
    Dayone(DayoneArgs),
}

#[derive(Debug, Args)]
struct DayoneArgs {
    /// Path to the Day One export `.json` file
    #[arg(value_name = "PATH")]
    path: PathBuf,

    /// Journal to import into (created if missing); defaults to the configured journal
    #[arg(long, value_name = "NAME")]
    journal: Option<String>,

    /// Download remote http(s) image links found in entry bodies. Off by
    /// default; when on, unreachable hosts are detected once and skipped rather
    /// than retried for every link. Skipped links are left in place in the body.
    #[arg(long)]
    download_images: bool,
}

#[derive(Debug, Args)]
struct LogArgs {
    #[arg(long, value_name = "NAME")]
    journal: Option<String>,

    #[arg(long, value_name = "TAG")]
    tag: Vec<String>,

    #[arg(long, value_name = "NAME")]
    person: Vec<String>,

    #[arg(long, value_name = "ACTIVITY")]
    activity: Vec<String>,

    #[arg(long, value_name = "LABEL")]
    feeling: Vec<String>,

    #[arg(long, value_name = "SCORE", allow_hyphen_values = true)]
    mood: Option<i8>,

    /// Set a location. Bare `--location` grabs a device GPS fix; a value is used
    /// as `lat,lon` when it parses as coordinates, otherwise as an address to
    /// geocode. Whenever a location resolves, weather/air/celestial are captured.
    #[arg(long, num_args = 0..=1, value_name = "ADDRESS|LAT,LON")]
    location: Option<Option<String>>,

    #[arg(value_name = "TEXT")]
    body: Vec<String>,
}

pub(crate) fn run() -> AppResult<()> {
    let cli = Cli::parse();
    timing::mark("cli:parse");
    let stdin_is_pipe = stdin_has_command_input();

    if let Some(command) = &cli.command {
        return handle_command(&cli, command, stdin_is_pipe);
    }

    if stdin_is_pipe {
        bail!("piped entry text requires `notema log`; run `notema log` with piped stdin");
    }

    let startup::Startup {
        config_path,
        config,
        store,
        discovery,
    } = startup::load_or_setup_with_path(cli.config.as_deref())?;
    tui::run(config_path, config, store, discovery)
}

fn handle_command(cli: &Cli, command: &CliCommand, stdin_is_pipe: bool) -> AppResult<()> {
    match command {
        CliCommand::Log(args) => log::run(cli, args, stdin_is_pipe),
        CliCommand::Use { name } => set_default_journal(cli, name),
        CliCommand::Import { source } => match source {
            ImportSource::Dayone(args) => import::run_dayone(cli, args),
        },
        CliCommand::Backfill => backfill::run(cli),
        CliCommand::Encryption { command } => handle_encryption_command(cli, command),
        CliCommand::Licenses { dependency } => crate::licenses::run(dependency.clone()),
        #[cfg(feature = "fuse")]
        CliCommand::Mount { mountpoint } => mount_command(cli, mountpoint.as_deref()),
    }
}

fn handle_encryption_command(cli: &Cli, command: &EncryptionCommand) -> AppResult<()> {
    match command {
        EncryptionCommand::Enable(args) => {
            let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
            encryption::encrypt_store(
                &store,
                args.name.as_deref(),
                args.no_passphrase,
                args.key_store.map(|store| store == NewKeyStore::Keyring),
            )
        }
        EncryptionCommand::Disable(args) => {
            let startup::Startup { mut store, .. } = startup::load_existing(cli.config.as_deref())?;
            // Everything knowable without secrets is validated before the scary
            // confirmation and the passphrase prompt.
            if !store.encryption_enabled() {
                bail!("this journal is not encrypted; there is nothing to disable");
            }
            require_unlock_available(&store)?;
            if !prompts::confirm(
                "Decrypt every entry and turn encryption off for this journal?",
                args.yes,
            )? {
                println!("Aborted.");
                return Ok(());
            }
            unlock_identity(&mut store)?;
            encryption::decrypt_store(store)
        }
        EncryptionCommand::Status => encryption_status_command(cli),
        EncryptionCommand::Device { command } => handle_device_command(cli, command),
        EncryptionCommand::Key { command } => handle_key_command(cli, command),
    }
}

fn handle_device_command(cli: &Cli, command: &DeviceCommand) -> AppResult<()> {
    match command {
        DeviceCommand::Enroll(args) => device_enroll_command(cli, args),
        DeviceCommand::List => device_list_command(cli),
        DeviceCommand::Revoke { name, confirm } => device_revoke_command(cli, name, confirm.yes),
        DeviceCommand::Rename { old, new } => device_rename_command(cli, old, new),
        DeviceCommand::Approve(args) => device_approve_command(cli, args),
        DeviceCommand::Reject(args) => device_reject_command(cli, args),
    }
}

fn handle_key_command(cli: &Cli, command: &KeyCommand) -> AppResult<()> {
    match command {
        KeyCommand::Store(args) => key_store_command(cli, args),
        KeyCommand::Passphrase(args) => device_passphrase_command(cli, args),
        KeyCommand::Rotate => device_rotate_command(cli),
        KeyCommand::Export(args) => key_export_command(cli, args),
    }
}

/// This device's identity, or a pointer at how to create one.
fn this_device_or_bail(store: &JournalStore) -> AppResult<notema_encryption::DeviceIdentityInfo> {
    match store.this_device()? {
        Some(info) => Ok(info),
        None => bail!(
            "no encryption identity on this device; run `{}` first",
            crate::ENROLL_CMD
        ),
    }
}

/// Where this device's key lives and how it is protected.
///
/// A key with no passphrase is only unprotected in the identity file; a keychain
/// or secret manager guards what it holds on its own.
fn print_key_location(info: &notema_encryption::DeviceIdentityInfo) {
    use notema_encryption::KeyStore;

    println!("This device's key is {}.", info.store.whereabouts());
    if info.passphrase_protected {
        println!("It is protected by a passphrase.");
        return;
    }
    println!(
        "No passphrase, so it opens automatically. {}.",
        match info.store {
            KeyStore::File => "Only the file's permissions protect it",
            KeyStore::Keyring => "The keychain protects it",
            KeyStore::Command => "Whatever it is fetched from is all that protects it",
        }
    );
}

/// Whether encryption is on, where this device's key sits, and who can read the
/// journal.
///
/// Reports rather than fails: a roster that will not verify is part of the
/// state, and the local half stays knowable.
fn encryption_status_command(cli: &Cli) -> AppResult<()> {
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    if !store.encryption_enabled() {
        println!("Encryption is off for this journal.");
        println!("Turn it on with `notema encryption enable`.");
        return Ok(());
    }
    println!("Encryption is on for {}.", store.root().display());
    match store.this_device()? {
        Some(info) => print_key_location(&info),
        None => println!(
            "This device has no key yet; run `{}` to request access.",
            crate::ENROLL_CMD
        ),
    }
    println!();
    if let Err(error) = print_device_roster(&store) {
        println!("Device roster: cannot be read — {error}");
        println!(
            "Entries already on this device stay readable. Approving or revoking a device is blocked until the roster in {} is restored from a device that has a good copy.",
            store.device_roster_path().display()
        );
    }
    Ok(())
}

fn key_store_command(cli: &Cli, args: &KeyStoreArgs) -> AppResult<()> {
    // clap requires `--read` for `command`, but not the reverse. Checked before
    // anything loads so a mistyped invocation fails as an argument error.
    if args.location != KeyStoreChoice::Command && args.read.is_some() {
        bail!("`--read` and `--write` describe a fetch command, so they only apply to `command`");
    }

    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    let info = this_device_or_bail(&store)?;

    let target = match args.location {
        KeyStoreChoice::File => KeyTarget::File,
        KeyStoreChoice::Keyring => KeyTarget::Keyring,
        KeyStoreChoice::Command => KeyTarget::Command {
            read: notema_encryption::KeyCommand::Shell(args.read.clone().unwrap_or_default()),
            write: args.write.clone().map(notema_encryption::KeyCommand::Shell),
        },
    };

    // Only to open what is stored now — moving it never changes the format, so
    // a passphrase-protected key stays that way wherever it lands.
    let passphrase = info
        .passphrase_protected
        .then(prompts::prompt_unlock_passphrase)
        .transpose()?;
    let leaving = info.store;
    store.set_key_store(&target, passphrase.as_ref())?;

    let now = this_device_or_bail(&store)?;
    println!("This device's key is now {}.", now.store.whereabouts());
    if now.store == notema_encryption::KeyStore::Command && args.write.is_none() {
        println!(
            "Without `--write` this is read-only: `notema encryption key rotate` and `notema encryption key passphrase` will have nowhere to write the new key."
        );
    }
    // A keychain item is ours and already gone; a secret manager is the user's
    // to clear, so the copy left there has to be named.
    if leaving == notema_encryption::KeyStore::Command {
        println!(
            "The old copy is still wherever the previous fetch command read it from; remove it there if you no longer want it."
        );
    }
    Ok(())
}

fn key_export_command(cli: &Cli, args: &ExportKeyArgs) -> AppResult<()> {
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    let info = this_device_or_bail(&store)?;

    // Unprotected key material is about to be written somewhere the user chose,
    // so say plainly what is going in the file before it goes there.
    let warning = if info.passphrase_protected {
        "Write a copy of this device's key, still protected by its passphrase?"
    } else {
        "Write a copy of this device's key? It is unprotected — anyone with the file can read this journal."
    };
    if !prompts::confirm(warning, args.confirm.yes)? {
        println!("Aborted.");
        return Ok(());
    }

    store.export_identity(&args.path)?;
    println!("Wrote this device's key to {}.", args.path.display());
    println!(
        "To restore it, copy the file back into a config directory as `identity.toml`. Keep it somewhere safe."
    );
    Ok(())
}

/// Bail unless this device has a key that can unlock this journal.
fn require_unlock_available(store: &JournalStore) -> AppResult<()> {
    if !store.unlock_available() {
        bail!(
            "this journal is encrypted but this device has no key at {}; run `{}` first",
            store.identity_path().display(),
            crate::ENROLL_CMD
        );
    }
    Ok(())
}

/// Unlock this device's identity, prompting for a passphrase only when the key
/// is passphrase-protected. Returns the passphrase too (for rotation, which
/// re-wraps the new key with it). Bails when this device has no key.
fn unlock_identity(store: &mut JournalStore) -> AppResult<Option<SecretString>> {
    require_unlock_available(store)?;
    let passphrase = if store.identity_needs_passphrase()? {
        Some(prompts::prompt_unlock_passphrase()?)
    } else {
        None
    };
    store.unlock(passphrase.as_ref())?;
    Ok(passphrase)
}

/// Open the store and unlock this device's identity. Used by the device
/// operations that must decrypt to re-encrypt.
fn open_unlocked_store_with_passphrase(
    cli: &Cli,
) -> AppResult<(JournalStore, Option<SecretString>)> {
    let startup::Startup { mut store, .. } = startup::load_existing(cli.config.as_deref())?;
    let passphrase = unlock_identity(&mut store)?;
    Ok((store, passphrase))
}

fn open_unlocked_store(cli: &Cli) -> AppResult<JournalStore> {
    Ok(open_unlocked_store_with_passphrase(cli)?.0)
}

/// Unlock this device's identity when the store is encrypted, prompting only
/// for a passphrase-protected key. A no-op for plaintext stores.
pub(super) fn unlock_if_encrypted(store: &mut JournalStore) -> AppResult<()> {
    if !store.encryption_enabled() {
        return Ok(());
    }
    unlock_identity(store)?;
    Ok(())
}

/// Mount the whole journal store as a decrypted filesystem. Journals appear as
/// top-level folders; entries and their assets are decrypted on read and
/// re-encrypted on write. Only encrypted journals can be mounted — for a
/// plaintext journal a mount would add nothing over the files already on disk.
/// The identity is unlocked first, prompting only when the key is passphrase-
/// protected. Blocks until unmounted.
///
/// With no `mountpoint` a temporary directory is created and used (on macOS the
/// journal still shows up as a drive in Finder); an explicit path is created if
/// it doesn't exist. Either way, a directory we created is removed after unmount.
#[cfg(feature = "fuse")]
fn mount_command(cli: &Cli, mountpoint: Option<&Path>) -> AppResult<()> {
    let startup::Startup { mut store, .. } = startup::load_existing(cli.config.as_deref())?;

    if !store.encryption_enabled() {
        bail!(
            "`notema mount` is only for encrypted journals; this journal is not encrypted. \
             Enable encryption with `notema encryption enable`, or open the files directly."
        );
    }
    // Unlock before creating the mount point, so a wrong passphrase leaves no
    // stray directory behind.
    unlock_identity(&mut store)?;

    // Resolve the mount point. An explicit path is created if missing; with none,
    // fall back to a fresh temp directory. `created` tracks whether we made the
    // directory so we can remove it again on unmount and leave nothing behind.
    let (mount_path, created): (PathBuf, bool) = match mountpoint {
        Some(path) if path.exists() => {
            if !path.is_dir() {
                bail!("mount point {} is not a directory", path.display());
            }
            (path.to_path_buf(), false)
        }
        Some(path) => {
            std::fs::create_dir_all(path)
                .with_context(|| format!("creating mount point {}", path.display()))?;
            (path.to_path_buf(), true)
        }
        None => {
            let path = std::env::temp_dir().join(format!("notema-mount-{}", std::process::id()));
            std::fs::create_dir_all(&path)
                .with_context(|| format!("creating mount point {}", path.display()))?;
            (path, true)
        }
    };

    println!(
        "Mounting journal at {}. Unmount with `umount {}` (macOS: `diskutil unmount`) or Ctrl-C.",
        mount_path.display(),
        mount_path.display()
    );
    notema_fuse::mount(store, &mount_path)?;
    println!("Unmounted {}.", mount_path.display());

    // Best-effort cleanup after unmount, only for a directory we created (and
    // only while empty — the mount left it as we found it). Ctrl-C kills the
    // process before this runs; the empty directory is harmless if it lingers.
    if created {
        let _ = std::fs::remove_dir(&mount_path);
    }
    Ok(())
}

fn device_passphrase_command(cli: &Cli, args: &PassphraseArgs) -> AppResult<()> {
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    let info = this_device_or_bail(&store)?;
    // Before the confirmation and the passphrase prompt: if the re-wrapped key
    // has nowhere to go, asking first would be asking for something we were
    // never going to use.
    store.check_key_is_writable()?;

    if args.remove
        && !prompts::confirm(
            "Remove the passphrase, storing this device's key unprotected?",
            args.confirm.yes,
        )?
    {
        println!("Aborted.");
        return Ok(());
    }

    let current = if info.passphrase_protected {
        Some(prompts::prompt_unlock_passphrase()?)
    } else {
        None
    };
    let new = if args.remove {
        None
    } else {
        Some(prompts::prompt_new_passphrase()?)
    };
    store.set_passphrase(current.as_ref(), new.as_ref())?;

    if new.is_some() {
        println!("Updated this device's key passphrase.");
    } else {
        println!(
            "Removed the passphrase; the key now opens automatically. Keep this device secure."
        );
    }
    Ok(())
}

fn device_rotate_command(cli: &Cli) -> AppResult<()> {
    {
        // Ahead of the unlock prompt, and well ahead of the roster op and
        // re-encryption pass that a rotation would otherwise have to roll back.
        let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
        store.check_key_is_writable()?;
    }
    let (mut store, passphrase) = open_unlocked_store_with_passphrase(cli)?;
    let summary = store.rotate_identity(passphrase.as_ref(), encryption::cli_progress("files"))?;
    println!(
        "Rotated this device's key and re-encrypted {} file(s).",
        summary.migrated_files
    );
    println!("The previous key can no longer read this journal.");
    print_warnings(&summary.warnings);
    Ok(())
}

fn device_enroll_command(cli: &Cli, args: &NewIdentityArgs) -> AppResult<()> {
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    if !store.encryption_enabled() {
        bail!(
            "this journal is not encrypted yet; run `notema encryption enable` to turn it on for this device"
        );
    }
    if store.unlock_available() {
        let mut store = store;
        let name = store
            .this_device()?
            .map(|device| device.name)
            .unwrap_or_default();
        // Unlock to tell the states apart: already a recipient, request still
        // queued, or request denied/lost — that last one re-requests with the
        // existing key rather than minting a new identity.
        unlock_identity(&mut store)?;
        if store.is_current_recipient()? {
            bail!("this device can already read this journal as '{name}'");
        }
        if store.self_request_pending()? {
            bail!(
                "this device is already waiting for approval as '{name}'.\n\
                 Run `notema encryption device list` to see the request, or approve it \
                 from a device that can already read this journal."
            );
        }
        let recipient = store.renew_access_request()?;
        println!("Requested access again as '{name}' with this device's existing key.");
        println!("  {}", recipient.encryption_key);
        println!(
            "Fingerprint (read this out to confirm it on the approving device):\n  {}",
            recipient.fingerprint()
        );
        println!(
            "On a device that can already read this journal, approve it — this request\nappears in `notema encryption device list` and a modal at launch — then run there:"
        );
        println!("  {} {name}", crate::APPROVE_CMD);
        return Ok(());
    }

    let (name, passphrase) =
        prompts::resolve_new_identity_options(args.name.as_deref(), args.no_passphrase)?;
    // Asked before the key exists, answered after it does — enroll mints an
    // identity just as `enable` does, so it offers the same choice.
    let use_keyring =
        encryption::resolve_key_source(args.key_store.map(|store| store == NewKeyStore::Keyring))?;

    // Joining a store that already exists (its recipients synced here): drop a
    // request for a device that can decrypt to approve.
    let recipient = store.request_access(&name, passphrase.as_ref())?;
    if use_keyring {
        encryption::move_key_to_keyring(&store, passphrase.as_ref());
    }
    println!("Requested access as '{name}'. Your public recipient (safe to share):");
    println!("  {}", recipient.encryption_key);
    println!(
        "Fingerprint (read this out to confirm it on the approving device):\n  {}",
        recipient.fingerprint()
    );
    println!(
        "On a device that can already read this journal, approve it — this request\nappears in `notema encryption device list` and a modal at launch — then run there:"
    );
    println!("  {} {name}", crate::APPROVE_CMD);
    encryption::print_backup_advice(&store)?;
    if passphrase.is_none() {
        println!("This key has no passphrase — keep this device and its backups secure.");
    }
    Ok(())
}

fn device_list_command(cli: &Cli) -> AppResult<()> {
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    print_device_roster(&store)
}

/// The roster and any pending requests. Shared by `device list` and the roster
/// half of `encryption status`.
///
/// Every fallible read runs before the first line prints, so a caller that
/// reports the error instead of propagating it cannot contradict itself.
fn print_device_roster(store: &JournalStore) -> AppResult<()> {
    let recipients = store.recipients()?;
    if recipients.is_empty() {
        println!("This journal is not encrypted.");
        return Ok(());
    }

    let this_device = store.this_device()?;
    let pending = store.pending_requests()?;
    println!("Recipients:");
    for recipient in &recipients {
        let marker = if this_device
            .as_ref()
            .is_some_and(|device| device.name == recipient.name)
        {
            "  (this device)"
        } else {
            ""
        };
        println!("  {}  {}{marker}", recipient.name, recipient.encryption_key);
        println!("      fingerprint: {}", recipient.fingerprint());
    }

    if !pending.is_empty() {
        println!("\nPending approval (run `{}`):", crate::APPROVE_CMD);
        println!("Confirm each fingerprint out-of-band before approving.");
        for request in &pending {
            println!(
                "  {}  {}  [{}]",
                request.recipient.name, request.recipient.encryption_key, request.id
            );
            println!("      fingerprint: {}", request.recipient.fingerprint());
        }
    }
    Ok(())
}

fn device_revoke_command(cli: &Cli, name: &str, skip_confirm: bool) -> AppResult<()> {
    let startup::Startup { mut store, .. } = startup::load_existing(cli.config.as_deref())?;
    // Everything knowable without secrets is validated before the scary
    // confirmation and the passphrase prompt.
    if !store.encryption_enabled() {
        bail!("this journal is not encrypted; there are no devices to revoke");
    }
    if !store
        .recipients()?
        .iter()
        .any(|recipient| recipient.name == name)
    {
        bail!("no device named '{name}'; run `notema encryption device list` to see the roster");
    }
    require_unlock_available(&store)?;
    if !prompts::confirm(
        &format!("Revoke '{name}' and re-encrypt all entries to exclude it?"),
        skip_confirm,
    )? {
        println!("Aborted.");
        return Ok(());
    }
    unlock_identity(&mut store)?;
    let summary = store.revoke_recipient(name, encryption::cli_progress("files"))?;
    println!(
        "Revoked '{name}' and re-encrypted {} file(s).",
        summary.migrated_files
    );
    println!("Revocation is forward-only: entries that device already synced stay readable to it.");
    print_warnings(&summary.warnings);
    Ok(())
}

fn device_rename_command(cli: &Cli, old: &str, new: &str) -> AppResult<()> {
    let store = open_unlocked_store(cli)?;
    let warnings = store.rename_recipient(old, new)?;
    println!("Renamed '{old}' to '{new}'.");
    print_warnings(&warnings);
    Ok(())
}

/// The pending requests an `approve`/`reject` invocation targets: `--all` picks
/// every queued request, otherwise `which` matches a request by id or device
/// name. `action` names the operation in the "how to select" error. Errors if
/// nothing was selected or matched; the empty-queue case is handled by callers.
fn select_requests(
    pending: Vec<PendingRequest>,
    args: &RequestSelectionArgs,
    action: &str,
) -> AppResult<Vec<PendingRequest>> {
    let selected: Vec<_> = if args.all {
        pending
    } else if let Some(which) = &args.which {
        pending
            .into_iter()
            .filter(|request| &request.id == which || &request.recipient.name == which)
            .collect()
    } else {
        bail!("specify a device name or id to {action}, or pass --all");
    };
    if selected.is_empty() {
        bail!("no pending request matched");
    }
    Ok(selected)
}

fn device_approve_command(cli: &Cli, args: &RequestSelectionArgs) -> AppResult<()> {
    let store = open_unlocked_store(cli)?;
    let pending = store.pending_requests()?;
    if pending.is_empty() {
        println!("No pending requests.");
        return Ok(());
    }

    for request in select_requests(pending, args, "approve")? {
        let summary = store.approve_pending(&request, encryption::cli_progress("files"))?;
        println!(
            "Approved '{}' and re-encrypted {} file(s).",
            request.recipient.name, summary.migrated_files
        );
        print_warnings(&summary.warnings);
    }
    Ok(())
}

fn device_reject_command(cli: &Cli, args: &RequestSelectionArgs) -> AppResult<()> {
    // Rejecting only deletes the request file, so no unlock/re-encryption needed.
    let startup::Startup { store, .. } = startup::load_existing(cli.config.as_deref())?;
    let pending = store.pending_requests()?;
    if pending.is_empty() {
        println!("No pending requests.");
        return Ok(());
    }

    for request in select_requests(pending, args, "reject")? {
        store.deny_pending(&request)?;
        println!("Rejected '{}'.", request.recipient.name);
    }
    Ok(())
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// Print post-success warnings from a storage operation (leftover backup,
/// un-advanced trust pins). The operation itself succeeded.
pub(crate) fn print_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("Warning: {warning}");
    }
}

fn set_default_journal(cli: &Cli, journal: &str) -> AppResult<()> {
    let startup::Startup {
        config_path: path,
        mut config,
        store,
        ..
    } = startup::load_existing(cli.config.as_deref())?;
    log::validate_existing_journal(store.root(), journal)?;
    config.journal.default = Some(journal.to_string());
    config::save_config(&path, &config)?;
    println!("Default journal set to {journal}");
    Ok(())
}

#[cfg(unix)]
fn stdin_has_command_input() -> bool {
    std::fs::metadata("/dev/stdin")
        .map(|metadata| {
            let file_type = metadata.file_type();
            file_type.is_fifo() || file_type.is_socket() || file_type.is_file()
        })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn stdin_has_command_input() -> bool {
    use std::io::IsTerminal;
    !std::io::stdin().is_terminal()
}
