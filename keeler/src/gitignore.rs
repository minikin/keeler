//! Adding Keeler's build artifacts to a project's `.gitignore`.
//!
//! Append-only, and only what is missing. The file is the project's own, so
//! their entries and their formatting survive untouched.

/// What Keeler's gates leave behind and git should not carry.
///
/// `crap-baseline.json` is deliberately absent: it is the shared reference
/// the delta gate measures against, so it belongs in git.
const ENTRIES: [&str; 4] = ["/target", "lcov.info", "crap-report.json", "mutants.out*/"];

/// The pattern with the slashes that do not change what it covers stripped,
/// so equivalent spellings compare equal.
///
/// At the root of a `.gitignore`, `/target`, `target/` and `target` all
/// name the same directory. Treating them as different entries piles up
/// lines the project has to read forever.
fn core(pattern: &str) -> &str {
    pattern.trim_start_matches('/').trim_end_matches('/')
}

/// True when the file already covers this pattern in any equivalent form.
#[must_use]
pub fn already_covers(ignored: &str, pattern: &str) -> bool {
    let wanted = core(pattern);
    ignored.lines().any(|line| core(line.trim()) == wanted)
}

/// The file with whatever it lacks appended, and the entries that were.
///
/// A file whose last line has no newline gets one first: without it the
/// first appended entry glues itself onto their last line, breaking both
/// that line and every later run's already-present check.
#[must_use]
pub fn extended(ignored: &str) -> (String, Vec<String>) {
    let mut out = ignored.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let mut added = Vec::new();
    for entry in ENTRIES {
        if !already_covers(&out, entry) {
            out.push_str(entry);
            out.push('\n');
            added.push(entry.to_string());
        }
    }
    (out, added)
}

#[cfg(test)]
mod tests {
    use super::{already_covers, core, extended};

    #[test]
    fn equivalent_spellings_share_a_core() {
        assert_eq!(core("/target"), "target");
        assert_eq!(core("target/"), "target");
        assert_eq!(core("/target/"), "target");
        assert_eq!(core("target"), "target");
    }

    #[test]
    fn a_different_path_is_not_covered() {
        // The comparison is on the whole entry, not a prefix: ignoring
        // `targets/` says nothing about `target/`.
        assert!(!already_covers("targets/\n", "/target"));
        assert!(!already_covers("/target-old\n", "/target"));
        assert!(already_covers("target/\n", "/target"));
    }

    #[test]
    fn an_empty_file_gains_no_leading_blank_line() {
        let (out, added) = extended("");
        assert!(!out.starts_with('\n'), "{out:?}");
        assert_eq!(added.len(), 4);
    }
}
