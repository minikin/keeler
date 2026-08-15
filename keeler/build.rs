//! Tells cargo when the embedded content has changed.
//!
//! `include_dir!` sweeps a directory at compile time, but its path-tracking
//! is behind a nightly feature this project does not enable, so cargo
//! records no dependency on what was swept. Without this, editing a command
//! or a skill and rebuilding produces a binary carrying the previous text —
//! "the binary is the source" quietly stops being true.
//!
//! The `include_bytes!` singles need no help: rustc emits dependency info
//! for those itself.

fn main() {
    for tree in ["../.claude/commands/keeler", "../.claude/skills"] {
        println!("cargo:rerun-if-changed={tree}");
    }
}
