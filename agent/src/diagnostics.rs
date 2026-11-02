//! Preserve operation/path context and concrete causes at user-facing error boundaries.
//! Callers must attach metadata, not credentials, command arguments or source contents.
use std::io::ErrorKind;

pub fn format_error(error: &anyhow::Error) -> String {
    let mut chain = error.chain();
    let mut text = readable(&chain.next().map(ToString::to_string).unwrap_or_default());
    for (index, cause) in chain.enumerate() {
        if index == 0 {
            text.push_str("\n\nCaused by:");
        }
        text.push_str(&format!(
            "\n{}. {}",
            index + 1,
            readable(&cause.to_string())
        ));
    }
    let kind = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind)
    });
    let advice = match kind {
        Some(ErrorKind::NotFound) => Some(
            "Check the exact path or executable shown above. For a missing project file, confirm --vault and compare the required file with the matching framework version; preserve project customizations and approved records. For an MCP executable, check its configured command, PATH and script interpreter. Use pm-agent --vault <VAULT> doctor for offline prerequisite checks.",
        ),
        Some(ErrorKind::PermissionDenied) => Some(
            "Check read/write or execute permission on the reported path and its parent directories. Verify the selected Vault and the account running pm-agent; do not discard project data to bypass this error.",
        ),
        Some(ErrorKind::InvalidData) => Some(
            "Check the reported file's format and encoding. Project instructions must be readable UTF-8 text; use the source extraction/review workflow for binary materials.",
        ),
        _ => None,
    };
    if let Some(advice) = advice {
        text.push_str("\n\nNext steps:\n");
        text.push_str(advice);
    }
    text
}

fn readable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_context_path_root_cause_and_evidence_based_next_steps() {
        let error = anyhow::Error::from(std::io::Error::from_raw_os_error(2))
            .context("Cannot inspect file metadata\nPath: /tmp/older project/Policy.md")
            .context("Cannot prepare Chat instructions\nStage: workflow");
        let diagnostic = format_error(&error);
        assert!(diagnostic.starts_with("Cannot prepare Chat instructions\nStage: workflow"));
        assert!(diagnostic.contains("/tmp/older project/Policy.md"));
        assert!(diagnostic.contains("os error 2"));
        assert!(diagnostic.contains("Next steps:"));
        assert!(diagnostic.contains("--vault"));
        assert!(diagnostic.contains("script interpreter"));
        assert!(diagnostic.contains("Caused by:"));
    }

    #[test]
    fn does_not_guess_a_missing_file_from_an_unrelated_error_or_emit_controls() {
        let diagnostic = format_error(&anyhow::anyhow!("Rejected\nDetail: bad \u{1b}[2J value"));
        assert!(diagnostic.starts_with("Rejected\nDetail:"));
        assert!(!diagnostic.contains('\u{1b}'));
        assert!(!diagnostic.contains("Next steps:"));
        let diagnostic = format_error(&anyhow::Error::from(std::io::Error::from(
            ErrorKind::PermissionDenied,
        )));
        assert!(diagnostic.contains("permission"));
        assert!(!diagnostic.contains("matching framework"));
    }
}
