# Security

## Reporting a vulnerability

Please report vulnerabilities privately via
[GitHub security advisories](https://github.com/minikin/keeler/security/advisories/new)
— not in public issues. You'll get a response within a week.

## Supported versions

The latest release (and `main`) receive fixes. Installations pin what they
run via `KEELER_REF`; the version installed is recorded at the top of
`.claude/keeler.md`.

## Scope worth knowing

- `install.sh` is distributed over `curl | bash`. If that bothers you (it
  should, a little), you have two better options: clone and read it first —
  the installer behaves identically run from a checkout — or verify a
  pinned release before running it: download `install.sh` and
  `install.sh.sha256` from the release page, then `sha256sum -c
  install.sh.sha256` (`shasum -a 256 -c` on macOS).
- The installer writes only into the project you point it at and never
  overwrites your files — conflicts land alongside as `<name>.keeler`. The
  one exception is `.claude/keeler.md`, the rules file Keeler owns: an
  upgrade replaces it and keeps your previous copy as
  `.claude/keeler.md.bak`.
- Tools come from crates.io / GitHub releases via `cargo binstall --locked`,
  plus the `llvm-tools-preview` component via rustup.
