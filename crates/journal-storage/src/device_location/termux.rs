//! Android/Termux provider: shell out to `termux-location` (from the Termux:API
//! add-on) and parse the JSON it prints.

use super::{DeviceFix, DeviceLocationSource, parse_fix_json};
use crate::AppResult;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

/// How long to wait for a fix before giving up. A cold GPS lock can take a while,
/// so this is generous; the dialog shows "Resolving…" meanwhile.
const TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn locate() -> AppResult<DeviceFix> {
    // `-r once` takes a single reading instead of streaming updates, which is
    // what we want and easier on the battery. The default GPS provider is the
    // most accurate; the command falls back internally when GPS is unavailable.
    let mut child = match Command::new("termux-location")
        .args(["-p", "gps", "-r", "once"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "termux-location not found — install the Termux:API app and the `termux-api` package"
        ),
        Err(error) => return Err(error.into()),
    };

    // Read stdout on a helper thread so a hung command can't block us forever.
    let mut stdout = child.stdout.take().expect("stdout piped above");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = String::new();
        let read = stdout.read_to_string(&mut buffer);
        let _ = tx.send(read.map(|_| buffer));
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(output)) => {
            let _ = child.wait();
            parse_fix_json(&output, DeviceLocationSource::Termux)
        }
        Ok(Err(error)) => {
            let _ = child.kill();
            Err(error.into())
        }
        Err(_) => {
            let _ = child.kill();
            anyhow::bail!("timed out waiting for a location fix from termux-location")
        }
    }
}
