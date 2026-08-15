//! SHA-256 checksums in the format `sha256sum -c` reads.
//!
//! One pure-Rust digest replaces the old shell dance of picking between
//! `sha256sum` and `shasum -a 256` depending on the platform.

use sha2::{Digest, Sha256};

/// The checksum line for `bytes`, named by the basename of `path`.
///
/// Exactly `<64 lowercase hex>  <basename>` — two spaces, and no directory
/// component, because verification runs wherever the adopter downloaded the
/// release assets, not where they were built.
#[must_use]
pub fn checksum_line(bytes: &[u8], path: &str) -> String {
    let digest = Sha256::digest(bytes);
    let name = std::path::Path::new(path)
        .file_name()
        .map_or(path, |name| name.to_str().unwrap_or(path));
    format!("{digest:x}  {name}")
}

#[cfg(test)]
mod tests {
    use super::checksum_line;

    /// The canonical empty-input digest, from FIPS 180-4.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn a_known_vector_hashes_correctly() {
        assert_eq!(
            checksum_line(b"", "empty"),
            format!("{EMPTY_SHA256}  empty")
        );
        assert_eq!(
            checksum_line(b"abc", "abc.txt"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  abc.txt",
        );
    }

    #[test]
    fn the_name_is_the_bare_basename() {
        // Verification runs wherever the assets were downloaded, so the
        // line must not carry the directory the file happened to live in.
        let line = checksum_line(b"", "release/v1/install.sh");
        assert!(
            line.ends_with("  install.sh"),
            "the line carries a directory: {line}",
        );
    }

    #[test]
    fn two_spaces_separate_digest_and_name() {
        // Not cosmetic: `sha256sum -c` splits on exactly this.
        let line = checksum_line(b"content", "file");
        let (digest, rest) = line.split_at(64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
        assert_eq!(rest, "  file");
    }

    use proptest::prelude::Strategy as _;

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 256,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
            )),
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// Format stability: whatever the bytes and whatever the path, the
        /// line is `<64 lowercase hex><two spaces><basename>`. This is the
        /// shape `sha256sum -c` parses; drift here breaks verification for
        /// everyone who followed the documented steps.
        #[test]
        fn the_line_is_always_a_digest_two_spaces_and_a_basename(
            bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..64),
            dirs in proptest::collection::vec("[a-z]{1,6}", 0..3),
            // A name of nothing but dots is not a file name — `.` and `..`
            // are directory references, and `fs::read` would refuse them
            // long before a checksum was ever asked for. Stating the
            // precondition beats loosening the assertion to accept them.
            name in "[a-zA-Z0-9._-]{1,12}"
                .prop_filter("only-dots is not a file name", |n: &String| {
                    n.chars().any(|c| c != '.')
                }),
        ) {
            let mut path = dirs.join("/");
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(&name);

            let line = checksum_line(&bytes, &path);
            let (digest, rest) = line.split_at(64);
            proptest::prop_assert!(
                digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                "digest is not 64 lowercase hex characters: {}", line,
            );
            proptest::prop_assert_eq!(rest, format!("  {}", name));
        }
    }
}
