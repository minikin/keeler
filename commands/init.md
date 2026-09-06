---
description: Prepare this project for Keeler — tools, Cargo.toml, .gitignore and the CI workflow
argument-hint: "[--no-tools] [--no-ci]"
---

Read `${CLAUDE_PLUGIN_ROOT}/keeler.md` before anything else — the workflow rules this command prepares a project for.

Options: $ARGUMENTS

You are in the **init stage**, run once per project. Keeler's own files — the commands, the skills, the rules, the recipes and the graph parser — stay in the plugin; what this stage lands is only what something other than the plugin reads from the repository: GitHub Actions reads the workflow from the repository and nowhere else, and clippy and rustfmt read their configs from the directory they run in.

1. **Check where you are.** The installer needs a Rust project: a `Cargo.toml` at the root of the working directory. If there is none, stop and say so — there is nothing here to configure.
2. **Run the installer from the plugin:**

   ```
   bash "${CLAUDE_PLUGIN_ROOT}/install.sh" .
   ```

   Never fetch it over the network: the copy in the plugin is the one whose version pins the CI workflow, and a downloaded one would install a different Keeler than the one you are running.

   Forward the flags the user asked for, in this order after the `.`:

   - `--no-tools` when they do not want the CLI tools (nextest, llvm-cov, mutants, crap) installed — for an offline machine, or one where the tools are managed elsewhere,
   - `--no-ci` when they do not want the GitHub Actions workflow.

3. **Report what it did, from its own output** — it prints what it created, what it modified and what it skipped. It is safe to re-run: existing files are never overwritten, and a second run reports nothing added.
4. **Relay the stale report verbatim if there is one.** A project that adopted Keeler through the installer before the plugin existed still holds the files it copied in. The installer names each one and gives a single `git rm -r --` line that removes them, and it names the rules import to delete from `CLAUDE.md` by hand. **It touches none of them** — one of those files may have been edited on purpose, and that is the user's call, not yours. Do not run the removal yourself unless the user asks.
5. **Say how `keeler` is reached.** Your Bash tool already has it: the plugin's `bin/` is on the PATH of every session where the plugin is enabled. A human in a terminal does not — tell them to add `${CLAUDE_PLUGIN_ROOT}/bin` to their PATH (the literal path is what `/plugin` shows for the installed plugin), after which `keeler --list` shows every recipe.

Then hand back: the next step is `/keeler:spec` for the first feature. Nothing is created under `specs/` here, so the rules reach a session automatically only once that first spec exists — until then, each command reads them itself.
