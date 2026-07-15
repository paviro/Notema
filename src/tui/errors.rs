/// The cause of a file-load error, for a toast: drops the outermost context —
/// which for these loads is a "reading/in theme <path>" wrapper whose full path
/// overflows the card and whose prose just repeats the toast title — keeps the
/// rest of the chain, and collapses it to a single capped line. Callers that
/// know the file name prepend it themselves.
pub(crate) fn concise_error(error: &anyhow::Error) -> String {
    let tail: Vec<String> = error
        .chain()
        .skip(1)
        .map(|cause| cause.to_string())
        .collect();
    let detail = if tail.is_empty() {
        error.to_string()
    } else {
        tail.join(": ")
    };
    let first_line = detail.lines().next().unwrap_or("unknown error");
    let mut concise: String = first_line.chars().take(120).collect();
    if first_line.chars().count() > 120 {
        concise.push('…');
    }
    concise
}

#[cfg(test)]
mod tests {
    use super::concise_error;

    #[test]
    fn concise_error_drops_the_outer_path_prose_and_keeps_the_cause() {
        // A macOS theme path carries a space ("Application Support") — the whole
        // wrapper, path and all, must go, leaving just the cause chain.
        let err = anyhow::anyhow!("unrecognized color 'blorp'")
            .context("in `mood`")
            .context("in theme /Users/x/Library/Application Support/notema/themes/fjord.toml");
        let concise = concise_error(&err);
        assert!(!concise.contains('/'), "drops the path: {concise}");
        assert!(
            !concise.contains("Application"),
            "drops the path prose: {concise}"
        );
        assert!(
            concise.contains("in `mood`"),
            "keeps the token context: {concise}"
        );
        assert!(
            concise.contains("unrecognized color 'blorp'"),
            "keeps the cause: {concise}"
        );
    }

    #[test]
    fn concise_error_falls_back_to_a_lone_error() {
        let err = anyhow::anyhow!("No such file or directory (os error 2)");
        assert_eq!(
            concise_error(&err),
            "No such file or directory (os error 2)"
        );
    }

    #[test]
    fn concise_error_is_single_line_and_capped() {
        let err = anyhow::anyhow!("{}\nsecond line", "x".repeat(400));
        let concise = concise_error(&err);
        assert!(!concise.contains('\n'), "single line");
        assert!(
            concise.chars().count() <= 121,
            "capped: {}",
            concise.chars().count()
        );
        assert!(concise.ends_with('…'), "marks truncation");
    }
}
