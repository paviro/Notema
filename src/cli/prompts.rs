//! Interactive terminal prompts shared by the CLI encryption commands and
//! first-run setup, kept together so their wording stays consistent.

use crate::AppResult;
use anyhow::bail;
use notema_encryption::{ExposeSecret, SecretString};
use rpassword::prompt_password;
use std::io::{self, IsTerminal, Write};

/// Ask the user to confirm a destructive encryption operation, returning `true`
/// to proceed. `skip` (from `--yes`) bypasses the prompt. Without a terminal to
/// answer on, it refuses rather than blocking, pointing at `--yes`.
pub(crate) fn confirm(prompt: &str, skip: bool) -> AppResult<bool> {
    if skip {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        bail!(
            "{prompt}\nrefusing to continue without a terminal to confirm; re-run with --yes to proceed"
        );
    }
    print!("{prompt} [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(is_yes(&input))
}

/// Interpret an interactive `[y/N]` answer, defaulting to no.
pub(crate) fn is_yes(input: &str) -> bool {
    matches!(input.trim(), "y" | "Y" | "yes" | "YES" | "Yes")
}

/// Interpret an interactive `[Y/n]` answer, defaulting to yes.
pub(crate) fn is_not_no(input: &str) -> bool {
    !matches!(input.trim(), "n" | "N" | "no" | "NO" | "No")
}

/// Pose a `[Y/n]` choice: a question, one line per option, then the prompt.
/// Defaults to yes, so anything but an explicit no is taken as one.
fn prompt_default_yes(
    stdout: &mut impl Write,
    question: &str,
    options: [&str; 2],
    prompt: &str,
) -> AppResult<bool> {
    writeln!(stdout, "{question}")?;
    for option in options {
        writeln!(stdout, "  {option}")?;
    }
    write!(stdout, "{prompt} [Y/n]: ")?;
    stdout.flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(is_not_no(&input))
}

/// Resolve the device name and optional passphrase for a *new* identity,
/// reusing the first-run prompts. `name` skips the name prompt; `no_passphrase`
/// stores the key unprotected, otherwise the passphrase is chosen interactively.
pub(crate) fn resolve_new_identity_options(
    name: Option<&str>,
    no_passphrase: bool,
) -> AppResult<(String, Option<SecretString>)> {
    let mut stdout = io::stdout();
    let device_name = match name {
        Some(name) => name.to_string(),
        None => prompt_device_name(&mut stdout)?,
    };
    let passphrase = if no_passphrase {
        None
    } else {
        prompt_passphrase_choice(&mut stdout)?
    };
    Ok((device_name, passphrase))
}

/// Prompt for this device's name (used to label its key), defaulting to the
/// hostname.
pub(crate) fn prompt_device_name(stdout: &mut impl Write) -> AppResult<String> {
    let default_name = crate::platform::device::default_device_name();
    write!(stdout, "Device name [{default_name}]: ")?;
    stdout.flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let name = input.trim();
    Ok(if name.is_empty() {
        default_name
    } else {
        name.to_string()
    })
}

/// Ask whether to protect the key with a passphrase, returning the passphrase to
/// use (`None` = store the key unprotected). Defaults to yes.
pub(crate) fn prompt_passphrase_choice(stdout: &mut impl Write) -> AppResult<Option<SecretString>> {
    let use_passphrase = prompt_default_yes(
        stdout,
        "Protect the key with a passphrase?",
        [
            "Yes — key is encrypted at rest; you enter the passphrase to unlock (best for laptops).",
            "No  — key opens automatically; relies on this device's own security (phones with full-disk encryption, etc.).",
        ],
        "Use a passphrase?",
    )?;
    use_passphrase.then(prompt_new_passphrase).transpose()
}

/// Prompt for a new passphrase twice, rejecting an empty entry or a mismatch.
pub(crate) fn prompt_new_passphrase() -> AppResult<SecretString> {
    let passphrase = SecretString::from(prompt_password("New journal encryption passphrase: ")?);
    if passphrase.expose_secret().is_empty() {
        bail!("encryption passphrase cannot be empty");
    }
    let confirm = SecretString::from(prompt_password("Confirm journal encryption passphrase: ")?);
    if passphrase.expose_secret() != confirm.expose_secret() {
        bail!("encryption passphrases did not match");
    }
    Ok(passphrase)
}

/// Prompt once for an existing passphrase to unlock this device's identity.
pub(crate) fn prompt_unlock_passphrase() -> AppResult<SecretString> {
    Ok(SecretString::from(prompt_password(
        "Journal encryption passphrase: ",
    )?))
}

/// Ask whether to keep this device's key in the OS keychain, defaulting to yes.
///
/// Answers no without asking when there is no keychain to reach, and when there
/// is no terminal to ask on: a keychain picked on a script's behalf may be
/// unreachable in the session that has to open the key, so `--key-store` is how
/// that is asked for deliberately.
pub(crate) fn prompt_keyring_choice(keyring_available: bool) -> AppResult<bool> {
    if !keyring_available || !io::stdin().is_terminal() {
        return Ok(false);
    }
    prompt_default_yes(
        &mut io::stdout(),
        "Where should this device's key be kept?",
        [
            "Keychain — held by the operating system, so no copy sits in a file backups can pick up.",
            "File     — inside identity.toml, readable only by you. Works everywhere, including over SSH.",
        ],
        "Use the keychain?",
    )
}
