use crate::AppResult;
use anyhow::{Context, bail};
use std::{fs, io::Write, path::Path, process::Command};

pub fn open_editor(editor: &str, path: &Path) -> AppResult<()> {
    let mut parts = shell_words::split(editor)?;
    if parts.is_empty() {
        bail!("editor command is empty");
    }

    let program = parts.remove(0);
    let status = Command::new(&program)
        .args(parts)
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch editor '{program}'"))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }
    Ok(())
}

/// Write `body` to a temporary file, open the editor on it, and return the
/// edited text. Returns `None` if the editor exits with an error.
pub fn edit_body(editor: &str, body: &str) -> AppResult<Option<String>> {
    let temp = editor_buffer(body)?;

    let result = open_editor(editor, temp.path());
    let new_body = if result.is_ok() {
        Some(fs::read_to_string(temp.path()).context("reading edited buffer")?)
    } else {
        None
    };
    result?;
    Ok(new_body)
}

fn editor_buffer(body: &str) -> AppResult<tempfile::NamedTempFile> {
    let mut temp = tempfile::Builder::new()
        .prefix(".journal-")
        .suffix(".body.md")
        .tempfile()
        .context("creating editor buffer")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .context("setting editor buffer permissions")?;
    }
    temp.write_all(body.as_bytes())
        .context("writing editor buffer")?;
    temp.flush().context("flushing editor buffer")?;
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_buffer_is_removed_after_edit() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("path.txt");
        let command = format!(
            "sh -c 'printf %s \"$1\" > {}; printf edited > \"$1\"' sh",
            shell_words::quote(marker.to_string_lossy().as_ref())
        );

        let edited = edit_body(&command, "original").unwrap().unwrap();
        let temp_path = fs::read_to_string(marker).unwrap();

        assert_eq!(edited, "edited");
        assert!(!Path::new(&temp_path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn editor_buffer_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = editor_buffer("secret").unwrap();
        let mode = temp.path().metadata().unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
    }
}
