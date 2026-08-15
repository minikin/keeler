# The Rust installer — what was built, and why it was parked

This branch holds spec 05: the Keeler installer rewritten from
`install.sh` into a Rust crate. It is **not merged**, and it was parked
deliberately rather than abandoned by accident. This file is what a
future reader needs to decide whether to resume it, and what to avoid if
they do.

## The judgement that parked it

`install.sh` is 248 lines of shell that works. It has been installing
Keeler into three pinned real-world repositories on every CI run for
days without a failure.

The replacement is 2,373 lines of Rust across the crate and its tests,
four tasks of eight, and it is not shippable. Along the way it
introduced three classes of defect the shell version never had:

- it overwrote a project's own file when that file existed but could not
  be read — reproduced, content destroyed, and the run reported success;
- it produced `Cargo.toml` files that cargo refuses to parse, in the
  ordinary case of a project that already has `[dev-dependencies]`;
- it declared itself publishable while being impossible to package.

Every one of those was caught by the review stage, and none by the 165
tests, 111 killed mutants or the CRAP gate that were green at the time.
That is worth saying plainly: **the test suite was measuring the code we
wrote, not the promises the code makes.**

The honest arithmetic is that the migration spent a day producing
something worse than what it replaced. Shell's weaknesses are real —
dialect fragility bit this project once, and 467 lines sit outside the
mutation gate — but they had not cost anything close to what replacing
them cost.

## What is on this branch

| Task | State | What it delivers |
| --- | --- | --- |
| T1 | done, reviewed | the `keeler` crate; the shipped tree embedded with `include_dir!`; gates in both directions on the carried set |
| T2 | done, reviewed | conflicts kept as `<name>.keeler`; the rules file replaced with its `.bak`; own-content preservation and conflict totality as properties |
| T3 | done, reviewed | `Cargo.toml` and `.gitignore` edited structurally via `toml_edit`; idempotence and entry-merging as properties |
| T4 | done, review outstanding | tool installation, the refusal of a non-Rust directory, and the command line; `cargo install --path keeler` produces a working binary |
| T5 | not started | the switch: `tests/installer.rs` onto the library, `install.sh` deleted |
| T6 | not started | the contract checker into xtask; `scripts/` removed |
| T7 | not started | the release installs from its own tag |
| T8 | not started | the upgrade recipe and the documentation |

`cargo install --path keeler && keeler init <project>` works today: it
lays down 18 files, edits the manifest so cargo still reads it, and adds
three `.gitignore` entries.

## What it cost, in defects

Each review found something no other gate could. If this is resumed,
these are the shapes to test for first:

1. **Absent is not unreadable.** `fs::read` fails for more than
   `NotFound`. `install.sh` used `[ -e ]`, an existence test, and was
   right to.
2. **Editing TOML as text is a bug generator.** A second
   `[dev-dependencies]` header, or a `[lints.clippy]` beside inherited
   lints, makes the manifest unparseable. Use a parser; assert with
   `cargo metadata`, which catches semantic rules a TOML parser cannot.
3. **`include_dir!` sweeps hidden directories** and registers no rebuild
   dependency on stable, so the binary can carry stale content.
4. **A crate whose assets live outside its package root cannot be
   packaged.** Symlinks under the crate do work — cargo follows them —
   if crates.io is ever wanted again.
5. **Tests that pass for the wrong reason.** Two here did: one asserted a
   run failed and named a flag, which also holds when the flag is read as
   a path; another built the exact manifest that breaks cargo and passed
   because nothing parsed the result.

## What to keep even if this is never merged

- **`toml_edit` over text append** — if the shell installer ever needs to
  touch a manifest more than it does now, this is the lesson.
- **The review stage is not optional.** It found HIGH-severity defects in
  three of three tasks it ran on. It was skipped for the twenty tasks
  before that, and nothing noticed.
- **Spec 06** exists because of that, and is the next thing to do.

## What was already merged and is not affected

Specs 01–04 are on `main` and stay there. v0.2.0 was cut by the xtask
release machinery and verified end to end. The release guard, the
contract checker and the real-world CI job are all live.

One correction to the record: the four `--workspace` defects in the
shipped gates were found because the repository gained a second crate,
not because shell became Rust. Any second crate would have surfaced them.
The xtask migration was credited with more than it earned.

## Resuming

```
git checkout feat/installer-in-rust
just dev            # 165 tests, all green
cargo mutants --package keeler   # 111 caught, 2 unviable
```

Start with `/keeler:review` on T4, which never had one.
