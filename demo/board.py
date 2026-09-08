#!/usr/bin/env python3
"""Build a throwaway repository whose board shows every state at once.

Repository machinery: never installed into a project, never shipped as
part of the workflow, and reached by hand rather than by a recipe.

Nothing here mocks `keeler-top`. The board is given a real git
repository, real branches and worktrees, real `.keeler/runs/<slug>/`
files in Claude's stream-json and real tmux sessions; only the work is
invented. So the board can be filmed, or looked at after a change to
the frame, without spending an agent on a wave.

    python3 demo/board.py                 # build it, print the command
    python3 demo/board.py --clean         # take it all away again

One at a time: the tmux session a running task needs is named for the
spec's slug and nothing else, so a second copy of this fixture would
answer for the first, and cleaning either one kills both their
sessions.

Python because the fixture is mostly JSON, and this file is the one
place in the repository that is neither the deliverable nor gated.
"""

import json
import os
import shutil
import subprocess
import sys
import time
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

PLUGIN = Path(__file__).resolve().parent.parent
ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
ROOT = Path(ARGS[0] if ARGS else "/tmp/keeler-demo").resolve()
SLUG = "01-the-espresso-machine"
SPEC = f"specs/{SLUG}.md"
NOW = datetime.now(timezone.utc)


def run(*args, cwd=None, check=True):
    return subprocess.run(args, cwd=cwd or ROOT, check=check,
                          capture_output=True, text=True)


def git(*args, cwd=None, check=True):
    return run("git", *args, cwd=cwd, check=check)


TASKS = [
    # id, title, needs, state
    ("T1", "The grinder learns a dose", [], "done"),
    ("T2", "The boiler holds a temperature", ["T1"], "done"),
    ("T3", "The pump follows a pressure curve", ["T1"], "running"),
    ("T4", "The scale stops the shot", ["T1"], "failed"),
    ("T5", "The steam wand reads the jug", ["T2"], "running"),
    ("T6", "The portafilter knows what it holds", ["T2"], "passed"),
    ("T7", "The machine remembers a recipe", ["T3", "T5"], "blocked"),
    ("T8", "The water tank refuses a dry shot", ["T2"], "paused"),
    ("T9", "The drip tray counts what it caught", ["T4"], "blocked"),
    ("T10", "The morning routine, end to end", ["T6", "T8"], "ready"),
]

SPEC_TEXT = f"""# Spec 01 — The espresso machine

**Status:** Approved
**Effort:** Large
**Module:** `src/`

## Context

A demonstration spec: ten tasks with a real dependency graph, so the
board has something to show. No code behind it — the work is invented,
the board is not.

---

## Acceptance Tests

### Scenario: a shot stops at the weight it was asked for

```
Given a dose of 18 g and a target of 36 g
When  the shot runs
Then  the pump stops within 0.3 g of the target
```

---

## Tasks

{{tasks}}

---

## Implementation Notes

Invented. See the Context.

### Non-goals

- Actually making coffee.
"""


def task_line(tid, title, needs, ticked):
    box = "x" if ticked else " "
    needs_s = f" Needs: {', '.join(needs)}." if needs else ""
    return (f"- [{box}] **{tid} — {title}.**{needs_s} "
            f"Scenarios: _a shot stops at the weight it was asked for_. "
            f"Tests: unit + acceptance.")


def spec_text(done_ids):
    lines = [task_line(t, title, needs, t in done_ids)
             for t, title, needs, _ in TASKS]
    return SPEC_TEXT.format(tasks="\n".join(lines))


def stamp(delta_seconds):
    return (NOW - timedelta(seconds=delta_seconds)).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def stream(path, model, spawned_ago, records):
    """records: list of (kind, payload, seconds_ago)"""
    out = []
    out.append(json.dumps({
        "type": "system", "subtype": "init", "cwd": str(ROOT),
        "session_id": str(uuid.uuid4()), "model": model,
        "permissionMode": "acceptEdits", "uuid": str(uuid.uuid4()),
        "timestamp": stamp(spawned_ago),
    }))
    for kind, payload, ago, ctx, out_tokens in records:
        mid = "msg_" + uuid.uuid4().hex[:16]
        if kind == "text":
            content = [{"type": "text", "text": payload}]
        elif kind == "bash":
            content = [{"type": "tool_use", "id": payload["id"], "name": "Bash",
                        "input": {"command": payload["command"],
                                  "description": payload.get("description", "")}}]
        elif kind == "edit":
            content = [{"type": "tool_use", "id": payload["id"], "name": "Edit",
                        "input": {"file_path": payload["file_path"],
                                  "old_string": "a", "new_string": "b"}}]
        elif kind == "skill":
            content = [{"type": "tool_use", "id": payload["id"], "name": "Skill",
                        "input": {"skill": payload["skill"], "args": "high"}}]
        else:
            raise ValueError(kind)
        out.append(json.dumps({
            "type": "assistant",
            "message": {
                "model": model.split("[")[0], "id": mid, "type": "message",
                "role": "assistant", "content": content,
                "usage": {
                    "input_tokens": 2,
                    "cache_creation_input_tokens": 270,
                    "cache_read_input_tokens": ctx - 272,
                    "output_tokens": out_tokens,
                },
            },
            "parent_tool_use_id": None, "session_id": "demo",
            "uuid": str(uuid.uuid4()), "timestamp": stamp(ago),
        }))
        if kind != "text" and payload.get("returned"):
            out.append(json.dumps({
                "type": "user",
                "message": {"role": "user", "content": [
                    {"tool_use_id": payload["id"], "type": "tool_result",
                     "content": "ok"}]},
                "parent_tool_use_id": None, "session_id": "demo",
                "uuid": str(uuid.uuid4()), "timestamp": stamp(ago - 1),
            }))
    path.write_text("\n".join(out) + "\n")


def tid_of(t):
    return t.lower()


def main():
    if ROOT.exists():
        shutil.rmtree(ROOT)
    for tid in (tid_of(t) for t, *_ in TASKS):
        wt = ROOT.parent / f"{ROOT.name}-{SLUG}-{tid}"
        if wt.exists():
            shutil.rmtree(wt)
    ROOT.mkdir(parents=True)

    git("init", "-q", "-b", "main")
    git("config", "user.name", "demo")
    git("config", "user.email", "demo@example.com")
    (ROOT / "README.md").write_text("# The espresso machine\n")
    (ROOT / "specs").mkdir()
    (ROOT / "reviews" / SLUG).mkdir(parents=True)
    git("add", "-A")
    git("commit", "-qm", "chore: the beginning")

    # the feature branch carries the spec, ticks for the done tasks
    done_ids = {t for t, _, _, s in TASKS if s == "done"}
    git("checkout", "-qb", f"feat/{SLUG}")
    (ROOT / SPEC).write_text(spec_text(done_ids))
    for tid in (tid_of(t) for t, _, _, s in TASKS if s in ("done", "passed", "failed")):
        (ROOT / "reviews" / SLUG / f"{tid}.md").write_text(
            f"# Review — {tid}\n\n**Commit:** deadbee\n\n**Verdict:** pass\n")
    git("add", "-A")
    git("commit", "-qm", f"docs({SLUG}): the approved spec and the task graph")

    runs = ROOT / ".keeler" / "runs" / SLUG
    runs.mkdir(parents=True)
    (ROOT / ".gitignore").write_text(".keeler/\n")
    git("add", ".gitignore")
    git("commit", "-qm", "chore: ignore the run directory")
    base = git("rev-parse", "HEAD").stdout.strip()

    subjects = {
        "feat": "the shape of it",
        "test": "the red one",
        "fix": "what the review found",
        "docs": "the review record",
        "chore": "tick the box",
    }

    for tid_upper, title, _needs, state in TASKS:
        tid = tid_of(tid_upper)
        branch = f"keeler/{SLUG}/{tid}"
        if state in ("blocked", "ready"):
            continue
        git("checkout", "-q", "-b", branch, base)
        howmany = {"running": 3, "passed": 4, "failed": 4,
                   "paused": 2, "done": 5}[state]
        for kind in list(subjects)[:howmany]:
            f = ROOT / "src" / f"{tid}_{kind}.rs"
            f.parent.mkdir(exist_ok=True)
            f.write_text(f"// {kind} for {tid}\n")
            git("add", "-A")
            git("commit", "-qm", f"{kind}({SLUG}): {tid_upper} — {subjects[kind]}")
        git("checkout", "-q", f"feat/{SLUG}")

    # worktrees for the live ones, one of them dirty
    for tid_upper, _t, _n, state in TASKS:
        if state not in ("running", "paused", "failed", "passed"):
            continue
        tid = tid_of(tid_upper)
        wt = ROOT.parent / f"{ROOT.name}-{SLUG}-{tid}"
        git("worktree", "add", "-q", str(wt), f"keeler/{SLUG}/{tid}")
    dirty = ROOT.parent / f"{ROOT.name}-{SLUG}-t3"
    (dirty / "src" / "t3_feat.rs").write_text("// half a curve\n")
    (dirty / "src" / "scratch.rs").write_text("// not yet added\n")

    # the streams
    OPUS = "claude-opus-5[1m]"
    stream(runs / "t3.stream", OPUS, 41 * 60, [
        ("edit", {"id": "tu1", "file_path": f"{ROOT}/tests/pump.rs",
                  "returned": True}, 2100, 88_000, 900),
        ("text", "The red test is in. Now the curve itself.", 1800, 96_000, 400),
        ("bash", {"id": "tu2", "command": "cargo nextest run --workspace",
                  "returned": True}, 1500, 104_000, 220),
        ("skill", {"id": "tu3", "skill": "code-review", "returned": True},
         600, 380_000, 300),
        ("text", "Two findings, both mine. Fixing, tests first.", 400,
         402_000, 260),
        ("bash", {"id": "tu4", "command": "keeler dev 2>&1 | tail -35"},
         134, 412_000, 180),
    ])
    stream(runs / "t5.stream", OPUS, 63 * 60, [
        ("edit", {"id": "u1", "file_path": f"{ROOT}/tests/wand.rs",
                  "returned": True}, 3000, 120_000, 800),
        ("bash", {"id": "u2", "command": "just mutants-diff", "returned": True},
         1200, 640_000, 500),
        ("text", "Zero survivors. The gate now, end to end.", 700, 800_000, 200),
        ("bash", {"id": "u3", "command": "keeler keeler-branch"}, 472,
         842_000, 150),
    ])
    stream(runs / "t4.stream", OPUS, 96 * 60, [
        ("bash", {"id": "f1", "command": "keeler dev", "returned": True},
         3600, 300_000, 700),
        ("bash", {"id": "f2", "command": "keeler keeler-branch", "returned": True},
         900, 470_000, 200),
        ("text", "The gate is red: the scale test hangs on a stub.", 800,
         472_000, 180),
    ])
    stream(runs / "t6.stream", OPUS, 120 * 60, [
        ("bash", {"id": "p1", "command": "keeler keeler-branch", "returned": True},
         2400, 330_000, 400),
        ("text", "Green. Record written, box ticked.", 2300, 332_000, 120),
    ])
    stream(runs / "t8.stream", "claude-sonnet-5", 30 * 60, [
        ("edit", {"id": "s1", "file_path": f"{ROOT}/tests/tank.rs",
                  "returned": True}, 1500, 36_000, 400),
        ("bash", {"id": "s2", "command": "keeler dev", "returned": True}, 1000,
         44_000, 200),
    ])
    for tid in ("t1", "t2"):
        stream(runs / f"{tid}.stream", OPUS, 180 * 60, [
            ("bash", {"id": f"{tid}g", "command": "keeler keeler-branch",
                      "returned": True}, 9000, 210_000, 300),
        ])

    # verdicts and markers
    (runs / "t6.exit").write_text("0\n")
    (runs / "t4.exit").write_text("2\n")
    (runs / "t1.exit").write_text("0\n")
    (runs / "t2.exit").write_text("0\n")
    (runs / "t8.paused").write_text("")
    for tid in ("t1", "t2", "t3", "t4", "t5", "t6", "t8"):
        (runs / f"{tid}.log").write_text("(the log the runner kept)\n")
        (runs / f"{tid}.sh").write_text("#!/usr/bin/env bash\n# demo\n")

    # the two running tasks need a session to be running
    for tid in ("t3", "t5"):
        name = f"keeler-{SLUG}-{tid}"
        subprocess.run(["tmux", "kill-session", "-t", f"={name}"],
                       capture_output=True)
        subprocess.run(["tmux", "new-session", "-d", "-s", name, "sleep 7200"],
                       check=True)

    keeler = PLUGIN / "bin" / "keeler"
    print(f"the demo repository is {ROOT}")
    print()
    print("  # the board, in a 120x40 terminal:")
    print(f"  cd {ROOT} && {keeler} keeler-top {SPEC}")
    print()
    print("  # the same frame as text, for a script or a diff:")
    print(f"  cd {ROOT} && {keeler} keeler-top --once {SPEC}")
    print()
    print(f"  # afterwards:  python3 {Path(__file__).resolve()} --clean")


if __name__ == "__main__":
    if "--clean" in sys.argv:
        for tid_upper, *_ in TASKS:
            subprocess.run(["tmux", "kill-session", "-t",
                            f"=keeler-{SLUG}-{tid_of(tid_upper)}"],
                           capture_output=True)
            wt = ROOT.parent / f"{ROOT.name}-{SLUG}-{tid_of(tid_upper)}"
            shutil.rmtree(wt, ignore_errors=True)
        shutil.rmtree(ROOT, ignore_errors=True)
        print(f"cleaned {ROOT} — and any tmux session named for {SLUG}")
    else:
        main()
