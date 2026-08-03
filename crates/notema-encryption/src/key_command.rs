//! Fetching this device's stored key from an external command.
//!
//! The command's stdout is the key material, so nothing here may let it reach an
//! unzeroized buffer: `Command::output()` is unusable because it grows a `Vec`
//! by realloc (leaving copies in freed heap) and hands it back inside a struct
//! that derives `Debug` and prints stdout.

use crate::{CommandStderr, EncryptionError, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use zeroize::Zeroizing;

/// Upper bound on the command's stdout. The secret bundle is a few hundred bytes;
/// the cap lets the destination buffer be allocated once at full size, which is
/// what makes it zeroizable — a `Vec` that grew by realloc cannot reach the
/// copies its earlier allocations left behind.
const MAX_STDOUT: usize = 64 * 1024;

/// How much of a failing command's stderr to quote back.
const MAX_STDERR: usize = 4 * 1024;

/// How a key command is spelled in `identity.toml`: a single line run through the
/// platform shell, or an explicit argv vector run with no shell at all.
///
/// The string form is the ergonomic one and what the documented recipes use, so
/// pipes work. The vector form is the escape hatch for anyone who would rather
/// not involve a shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyCommand {
    Shell(String),
    Argv(Vec<String>),
}

impl KeyCommand {
    /// How to name this command in an error. For the shell form that's the line
    /// the user wrote, not `sh`, which would tell them nothing.
    ///
    /// A command has no business embedding the key it is meant to fetch, but
    /// nothing stops one from doing so, and this string ends up in error
    /// messages — so redact anything that reads like key material.
    pub fn label(&self) -> String {
        let full = match self {
            Self::Shell(line) => line.trim().to_string(),
            Self::Argv(argv) => argv.first().cloned().unwrap_or_default(),
        };
        let cleaned = full
            .split_whitespace()
            .map(|word| {
                if crate::error::looks_secret(word) {
                    "<redacted>"
                } else {
                    word
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        match cleaned.char_indices().nth(60) {
            Some((cut, _)) => format!("{}…", &cleaned[..cut]),
            None => cleaned,
        }
    }

    fn build(&self) -> Result<Command> {
        match self {
            Self::Shell(line) if line.trim().is_empty() => Err(EncryptionError::EmptyKeyCommand),
            Self::Shell(line) => {
                let (shell, flag) = if cfg!(windows) {
                    ("cmd", "/C")
                } else {
                    ("sh", "-c")
                };
                let mut command = Command::new(shell);
                command.arg(flag).arg(line);
                Ok(command)
            }
            Self::Argv(argv) => {
                let (program, args) = argv.split_first().ok_or(EncryptionError::EmptyKeyCommand)?;
                if program.trim().is_empty() {
                    return Err(EncryptionError::EmptyKeyCommand);
                }
                let mut command = Command::new(program);
                command.args(args);
                Ok(command)
            }
        }
    }
}

/// Run `command` and return its stdout as the stored key material.
///
/// Environment and working directory are inherited: `op`, `pass` and `vault` all
/// need `PATH`, `HOME`, `GPG_TTY` and friends to find their agents. There is no
/// timeout — a Touch ID or pinentry prompt is legitimately unbounded, and the
/// caller guarantees this never runs while the TUI owns the terminal.
pub(crate) fn run(command: &KeyCommand) -> Result<Zeroizing<Vec<u8>>> {
    let mut child = command
        .build()?
        // Nothing may read the terminal out from under a caller that is about to
        // prompt for a passphrase.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| spawn_error(command, &error))?;

    let mut stdout = child.stdout.take().expect("stdout piped above");
    let mut stderr = child.stderr.take().expect("stderr piped above");

    // Allocated once at full size and never grown, so `Zeroizing` wipes every
    // byte the secret could have touched — it zeroes spare capacity too. One
    // byte over the cap so a too-long output is detectable rather than silently
    // truncated.
    let mut buffer = Zeroizing::new(vec![0u8; MAX_STDOUT + 1]);

    // Drain stderr concurrently: a command that chatters past the pipe buffer
    // while we are blocked on stdout would deadlock.
    let (filled, stderr_tail) = std::thread::scope(|scope| {
        let drain = scope.spawn(move || drain_tail(&mut stderr, MAX_STDERR));
        let filled = fill(&mut stdout, buffer.as_mut_slice());
        // `fill` stops at the cap, so a command still producing output would
        // block on a full pipe — and never close stderr, leaving the drain
        // above unable to finish. Kill it: its output is already more than we
        // will accept. Closing our end first gives a well-behaved command the
        // chance to exit on EPIPE instead.
        drop(stdout);
        if matches!(filled, Ok(read) if read > MAX_STDOUT) {
            let _ = child.kill();
        }
        (filled, drain.join().unwrap_or_default())
    });

    let status = child.wait()?;
    let filled = filled?;

    if filled > MAX_STDOUT {
        return Err(EncryptionError::KeyCommandOutputTooLarge { limit: MAX_STDOUT });
    }
    if !status.success() {
        // Whatever partial secret reached the buffer dies with it.
        return Err(EncryptionError::KeyCommandFailed {
            program: command.label(),
            status: describe(&status),
            stderr: CommandStderr::new(&stderr_tail, MAX_STDERR),
        });
    }
    // `truncate` never reallocates, so the dropped tail stays inside the same
    // allocation and is wiped along with it.
    buffer.truncate(filled);
    Ok(buffer)
}

/// Run `command` with `material` on its stdin, for seeding a secret manager.
///
/// Piping rather than passing the secret as an argument is the point: argv is
/// visible to every process on the machine.
pub(crate) fn store(command: &KeyCommand, material: &[u8]) -> Result<()> {
    let mut child = command
        .build()?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| spawn_error(command, &error))?;

    let mut stdin = child.stdin.take().expect("stdin piped above");
    let mut stdout = child.stdout.take().expect("stdout piped above");
    let mut stderr = child.stderr.take().expect("stderr piped above");

    // Write and drain at once: a command that prints more than a pipe buffer
    // before reading all of its input would otherwise deadlock against us.
    let (written, stderr_tail) = std::thread::scope(|scope| {
        let drain = scope.spawn(move || {
            let _ = drain_tail(&mut stdout, 0);
            drain_tail(&mut stderr, MAX_STDERR)
        });
        let written = stdin.write_all(material).and_then(|()| stdin.flush());
        drop(stdin);
        (written, drain.join().unwrap_or_default())
    });

    let status = child.wait()?;

    // A command that exits early makes our write fail with a broken pipe, so the
    // exit status is the honest diagnosis and goes first.
    if !status.success() {
        return Err(EncryptionError::KeyCommandFailed {
            program: command.label(),
            status: describe(&status),
            stderr: CommandStderr::new(&stderr_tail, MAX_STDERR),
        });
    }
    match written {
        // A command that succeeded without reading all of its input still got
        // what it wanted; only a real write failure is worth reporting.
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            Err(EncryptionError::KeyCommandSpawn {
                program: command.label(),
                detail: format!("could not write the key to its stdin: {error}"),
            })
        }
        _ => Ok(()),
    }
}

fn spawn_error(command: &KeyCommand, error: &io::Error) -> EncryptionError {
    let detail = if error.kind() == io::ErrorKind::NotFound {
        "not found — check the program name and that it is on PATH".to_string()
    } else {
        error.to_string()
    };
    EncryptionError::KeyCommandSpawn {
        program: command.label(),
        detail,
    }
}

/// Read into `dest` until it is full or the reader ends, with no intermediate
/// buffer the secret could be copied through.
fn fill(reader: &mut impl Read, dest: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < dest.len() {
        match reader.read(&mut dest[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Read a stream to its end, keeping only the last `limit` bytes. Draining fully
/// is what keeps the child from blocking on a full pipe; the rolling window is
/// what keeps a runaway command from filling memory.
fn drain_tail(reader: &mut impl Read, limit: usize) -> String {
    let mut kept: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if limit > 0 {
                    kept.extend_from_slice(&chunk[..read]);
                    if kept.len() > limit {
                        kept.drain(..kept.len() - limit);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

fn describe(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => "killed by a signal".to_string(),
    }
}
