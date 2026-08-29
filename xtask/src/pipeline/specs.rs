//! The spec reader: ticked tasks from every spec, and the promise
//! `Status: Implemented` makes.
//!
//! The grammar is `scripts/keeler-graph.sh`'s, re-read here for the gate —
//! the two must agree, or a human and the tools would disagree about what
//! a spec says. Like that parser, this one refuses what it cannot read
//! instead of dropping it: a ticked task that vanishes in parsing is a
//! task the gate silently vouched for.

use std::collections::BTreeMap;
use std::path::Path;

use super::decision::TaskId;
use crate::Failure;

/// One checkbox line in a spec's Tasks section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub ticked: bool,
}

/// One spec file, as far as the gate cares: its slug, whether its status
/// says `Implemented`, and its tasks in the order the spec lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub slug: String,
    pub implemented: bool,
    pub tasks: Vec<Task>,
}

/// Reads one spec's status and tasks.
///
/// # Errors
///
/// Refuses, naming the line, whatever the grammar cannot read: a checkbox
/// item that does not open with `**Tn `, a checkbox shape it does not
/// know, a task defined twice, an unclosed code fence, or a spec with no
/// `## Tasks` section at all.
pub fn parse(slug: &str, content: &str) -> Result<Spec, String> {
    let mut spec = Spec {
        slug: slug.to_string(),
        implemented: false,
        tasks: Vec::new(),
    };
    let mut status_read = false;
    // Some(line) while inside a fence — the line is what an unclosed one
    // is named by.
    let mut fence_opened_at = None;
    let mut in_tasks = false;
    let mut section_found = false;
    let mut defined_at = BTreeMap::new();

    for (index, raw) in content.lines().enumerate() {
        let number = index + 1;
        // CRLF endings would hide every heading and every item.
        let line = raw.trim_end_matches('\r');

        // Fences are tracked over the whole file, before anything else
        // reads the line: a quoted `## Tasks` with an example item under
        // it would otherwise open the section there.
        if line.trim_start_matches([' ', '\t']).starts_with("```") {
            fence_opened_at = match fence_opened_at {
                Some(_) => None,
                None => Some(number),
            };
            continue;
        }
        if fence_opened_at.is_some() {
            continue;
        }

        if !status_read && let Some(value) = line.strip_prefix("**Status:**") {
            status_read = true;
            spec.implemented = value.trim() == "Implemented";
        } else if is_tasks_heading(line) {
            in_tasks = true;
            section_found = true;
        } else if in_tasks && (line.starts_with("# ") || line.starts_with("## ")) {
            // A heading at the section level or above ends it; a deeper
            // one is structure within it, and truncating there would drop
            // tasks in silence.
            in_tasks = false;
        } else if in_tasks {
            read_item(&mut spec, &mut defined_at, line, number)?;
        }
    }

    if let Some(opened) = fence_opened_at {
        return Err(format!(
            "line {opened}: a code fence opened here and was never closed — \
             every task after it would be dropped in silence"
        ));
    }
    if !section_found {
        return Err("no ## Tasks section found — nothing to read".to_string());
    }
    Ok(spec)
}

/// Whether this line opens the Tasks section: `## Tasks` and nothing else.
fn is_tasks_heading(line: &str) -> bool {
    line.strip_prefix("## Tasks")
        .is_some_and(|rest| rest.trim_matches([' ', '\t']).is_empty())
}

/// Reads one line inside the Tasks section: a task item, a shape that must
/// be refused, or prose to pass over.
fn read_item(
    spec: &mut Spec,
    defined_at: &mut BTreeMap<String, usize>,
    line: &str,
    number: usize,
) -> Result<(), String> {
    if let Some((ticked, body)) = checkbox(line) {
        let Some(id) = task_id(body) else {
            return Err(format!(
                "line {number}: an item that does not open with **Tn — : {line}"
            ));
        };
        if let Some(first) = defined_at.insert(id.to_string(), number) {
            return Err(format!(
                "line {number}: {id} is defined twice (first at line {first})"
            ));
        }
        spec.tasks.push(Task {
            id: TaskId::new(&spec.slug, id),
            ticked,
        });
    } else if unreadable_checkbox(line) {
        // A checkbox the grammar does not know is a task that would
        // vanish: never spawned, never counted, never asked for a review.
        return Err(format!(
            "line {number}: a checkbox line the grammar cannot read: {line}"
        ));
    }
    Ok(())
}

/// The item opening `- [ ] ` / `- [x] ` / `- [X] `: whether the box is
/// ticked, and the text after it.
fn checkbox(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix("- [")?;
    let mark = rest.chars().next()?;
    let body = rest.get(1..)?.strip_prefix("] ")?;
    match mark {
        'x' | 'X' => Some((true, body)),
        ' ' => Some((false, body)),
        _ => None,
    }
}

/// The id in an item body opening `**Tn ` — `Tn` itself, or nothing if the
/// body opens some other way.
fn task_id(body: &str) -> Option<&str> {
    let digits = body.strip_prefix("**T")?;
    let count = digits.bytes().take_while(u8::is_ascii_digit).count();
    (count > 0 && digits[count..].starts_with(' ')).then(|| &body[2..3 + count])
}

/// A bullet that reaches for a checkbox but is not one the grammar knows —
/// except a markdown link, `- [Keeler](…)`, which is prose.
fn unreadable_checkbox(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(['-', '*', '+']) else {
        return false;
    };
    let Some(bracketed) = rest.strip_prefix(" [") else {
        return false;
    };
    !bracketed
        .split_once(']')
        .is_some_and(|(_, after)| after.starts_with('('))
}

/// Every ticked task, in the order the specs list them — the gate's first
/// input.
#[must_use]
pub fn ticked(specs: &[Spec]) -> Vec<TaskId> {
    specs
        .iter()
        .flat_map(|spec| &spec.tasks)
        .filter(|task| task.ticked)
        .map(|task| task.id.clone())
        .collect()
}

/// The promise `Status: Implemented` makes: every task ticked. The tasks
/// that break it — unticked in a spec that claims to be finished — in the
/// order the specs list them.
#[must_use]
pub fn unkept_promises(specs: &[Spec]) -> Vec<TaskId> {
    specs
        .iter()
        .filter(|spec| spec.implemented)
        .flat_map(|spec| &spec.tasks)
        .filter(|task| !task.ticked)
        .map(|task| task.id.clone())
        .collect()
}

/// Reads every `*.md` in `dir` but `TEMPLATE.md`, in file-name order.
///
/// # Errors
///
/// Names the directory it cannot read, the file it cannot read, or the
/// file and line one of the specs was refused on.
pub fn read_from(dir: &Path) -> Result<Vec<Spec>, Failure> {
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .and_then(Iterator::collect)
        .map_err(|why| format!("cannot read {}: {why}", dir.display()))?;

    let mut found: Vec<(String, std::path::PathBuf)> = entries
        .iter()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let slug = name.strip_suffix(".md")?.to_string();
            (slug != "TEMPLATE").then(|| (slug, entry.path()))
        })
        .collect();
    // File-name order, whatever order the directory hands them back in —
    // the gate's output must not depend on the filesystem's mood.
    found.sort();

    let mut specs = Vec::new();
    for (slug, path) in found {
        let content = crate::read(&path.display().to_string())?;
        specs.push(parse(&slug, &content).map_err(|why| format!("{}: {why}", path.display()))?);
    }
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::{Spec, Task, TaskId, parse, ticked, unkept_promises};

    fn text(status: &str, tasks: &str) -> String {
        format!(
            "# Spec 09 — Demo\n\n**Status:** {status}\n\n## Context\n\nWords.\n\n## Tasks\n\n{tasks}\n"
        )
    }

    fn task(id: &str, ticked: bool) -> Task {
        Task {
            id: TaskId::new("09-demo", id),
            ticked,
        }
    }

    #[test]
    fn ticked_tasks_are_read_from_the_tasks_section() {
        // Given a Tasks section with ticked and unticked items — one of
        // them ticked the way GitHub renders too, with an uppercase X
        let spec = parse(
            "09-demo",
            &text(
                "Approved",
                "- [x] **T1 — First.**\n- [ ] **T2 — Second.**\n- [X] **T3 — Third.**",
            ),
        )
        .unwrap();

        // Then every item is a task, ids lowercased into the one address
        assert_eq!(
            spec,
            Spec {
                slug: "09-demo".into(),
                implemented: false,
                tasks: vec![task("t1", true), task("t2", false), task("t3", true)],
            },
        );
        // And only the ticked ones are the gate's input
        assert_eq!(
            ticked(std::slice::from_ref(&spec)),
            vec![TaskId::new("09-demo", "t1"), TaskId::new("09-demo", "t3")],
        );
    }

    #[test]
    fn a_checkbox_outside_the_tasks_section_is_prose() {
        // Given checkboxes before the Tasks section and after the heading
        // that ends it
        let spec = parse(
            "09-demo",
            "**Status:** Approved\n\n## Context\n\n- [x] **T9 — Not a task.**\n\n\
             ## Tasks\n\n- [x] **T1 — Real.**\n\n## Notes\n\n- [ ] **T2 — Not one either.**\n",
        )
        .unwrap();

        // Then only the section's own item was read
        assert_eq!(spec.tasks, vec![task("t1", true)]);
    }

    #[test]
    fn a_fenced_example_is_not_a_task() {
        // Given a fenced example item inside the Tasks section — the shape
        // spec 06's implementation notes quote
        let spec = parse(
            "09-demo",
            &text(
                "Approved",
                "- [x] **T1 — Real.**\n\n```\n- [ ] **T9 — An example item.**\n```\n\n- [x] **T2 — Also real.**",
            ),
        )
        .unwrap();

        // Then the example is prose and the items around it are tasks
        assert_eq!(spec.tasks, vec![task("t1", true), task("t2", true)]);
    }

    #[test]
    fn a_fenced_tasks_heading_does_not_open_the_section() {
        // Given a spec that quotes a `## Tasks` heading and an item in a
        // fence before the real section
        let spec = parse(
            "09-demo",
            "**Status:** Approved\n\n## Context\n\n```\n## Tasks\n\n- [x] **T9 — Quoted.**\n```\n\n\
             ## Tasks\n\n- [x] **T1 — Real.**\n",
        )
        .unwrap();

        // Then the quoted section was never opened
        assert_eq!(spec.tasks, vec![task("t1", true)]);
    }

    #[test]
    fn an_indented_fence_still_fences() {
        // Given a fence indented inside an item, as markdown allows
        let spec = parse(
            "09-demo",
            &text(
                "Approved",
                "- [x] **T1 — Real.**\n  ```\n- [ ] **T9 — Hidden.**\n  ```",
            ),
        )
        .unwrap();

        // Then what it holds is prose
        assert_eq!(spec.tasks, vec![task("t1", true)]);
    }

    #[test]
    fn an_item_that_does_not_open_with_tn_is_refused_naming_the_line() {
        // Given items that are checkboxes but not `**Tn ` openings
        for bad in [
            "- [x] no id at all",
            "- [x] **T — digitless.**",
            "- [x] **T1— unspaced.**",
            "- [x] *T1 — one star.**",
        ] {
            // When the spec is read
            let refusal = parse(
                "09-demo",
                &format!("**Status:** Approved\n\n## Tasks\n\n{bad}\n"),
            )
            .unwrap_err();

            // Then it is refused, naming the line
            assert!(
                refusal.contains("line 5") && refusal.contains("does not open with **Tn"),
                "`{bad}` was not refused by line: {refusal}",
            );
        }
    }

    #[test]
    fn a_checkbox_the_grammar_cannot_read_is_refused() {
        // Given checkbox shapes the grammar does not know — a task written
        // in one of these would vanish: never spawned, never counted
        for bad in [
            "* [y] a strange mark",
            "- [✓] a pretty tick",
            "+ [x no closing bracket",
        ] {
            let refusal = parse(
                "09-demo",
                &format!("**Status:** Approved\n\n## Tasks\n\n{bad}\n"),
            )
            .unwrap_err();
            assert!(
                refusal.contains("line 5") && refusal.contains("cannot read"),
                "`{bad}` was not refused by line: {refusal}",
            );
        }
    }

    #[test]
    fn a_link_bullet_is_prose_not_a_checkbox() {
        // Given a markdown link bullet in the section — brackets, but prose
        let spec = parse(
            "09-demo",
            &text(
                "Approved",
                "- [Keeler](https://example.com) explains why.\n- [x] **T1 — Real.**",
            ),
        )
        .unwrap();
        assert_eq!(spec.tasks, vec![task("t1", true)]);
    }

    #[test]
    fn a_task_defined_twice_is_refused_naming_both_lines() {
        // Given the same id on two items
        let refusal = parse(
            "09-demo",
            &text("Approved", "- [x] **T1 — Once.**\n- [ ] **T1 — Twice.**"),
        )
        .unwrap_err();

        // Then the refusal names the line and where the first one was
        assert!(
            refusal.contains("line 12") && refusal.contains("T1") && refusal.contains("line 11"),
            "the duplicate is not named by both lines: {refusal}",
        );
    }

    #[test]
    fn an_unclosed_fence_is_refused() {
        // Given a fence nobody closed — every task after it would be
        // dropped in silence, which reads as every one of them reviewed
        let refusal = parse(
            "09-demo",
            &text(
                "Approved",
                "- [x] **T1 — Real.**\n```\n- [x] **T2 — Swallowed.**",
            ),
        )
        .unwrap_err();
        assert!(
            refusal.contains("line 12") && refusal.contains("never closed"),
            "the open fence is not named: {refusal}",
        );
    }

    #[test]
    fn a_spec_without_a_tasks_section_is_refused() {
        // Given a spec whose Tasks heading is missing — or typo'd, which
        // would hide every tick below it
        let refusal = parse(
            "09-demo",
            "**Status:** Approved\n\n## Task\n\n- [x] **T1 — Lost.**\n",
        )
        .unwrap_err();
        assert!(
            refusal.contains("no ## Tasks section"),
            "the missing section is not named: {refusal}",
        );
    }

    #[test]
    fn a_deeper_heading_is_structure_within_the_section() {
        // Given a `### Phase` heading between items, and a `# Appendix`
        // after them
        let spec = parse(
            "09-demo",
            &text(
                "Approved",
                "- [x] **T1 — A.**\n\n### Phase 2\n\n- [x] **T2 — B.**\n\n# Appendix\n\n- [x] **T3 — C.**",
            ),
        )
        .unwrap();

        // Then the deeper heading does not end the section; the top-level
        // one does
        assert_eq!(spec.tasks, vec![task("t1", true), task("t2", true)]);
    }

    #[test]
    fn an_implemented_status_is_the_promise() {
        let spec = parse("09-demo", &text("Implemented", "- [x] **T1 — Done.**")).unwrap();
        assert!(spec.implemented);
    }

    #[test]
    fn any_other_status_promises_nothing() {
        // Given the statuses a spec can carry without claiming to be
        // finished — including the template's menu line and none at all
        for status in ["Draft", "Approved", "Draft | Approved | Implemented"] {
            let spec = parse("09-demo", &text(status, "- [ ] **T1 — Open.**")).unwrap();
            assert!(!spec.implemented, "`{status}` read as Implemented");
        }
        let unstated = parse("09-demo", "## Tasks\n\n- [ ] **T1 — Open.**\n").unwrap();
        assert!(!unstated.implemented);
    }

    #[test]
    fn the_first_status_line_wins_and_a_fenced_one_is_prose() {
        // Given a fenced Status line before the real one, and a stray one
        // after it
        let spec = parse(
            "09-demo",
            "```\n**Status:** Implemented\n```\n\n**Status:** Draft\n\n**Status:** Implemented\n\n\
             ## Tasks\n\n- [ ] **T1 — Open.**\n",
        )
        .unwrap();

        // Then the spec's status is the first one said in prose
        assert!(!spec.implemented);
    }

    #[test]
    fn crlf_endings_do_not_hide_the_grammar() {
        // Given a spec written with CRLF line endings
        let spec = parse(
            "09-demo",
            "**Status:** Implemented\r\n\r\n## Tasks\r\n\r\n- [x] **T1 — Done.**\r\n",
        )
        .unwrap();
        assert!(spec.implemented);
        assert_eq!(spec.tasks, vec![task("t1", true)]);
    }

    #[test]
    fn an_implemented_spec_vouches_for_every_task() {
        // Given a spec marked Implemented with a task unticked
        let spec = parse(
            "06-demo",
            &format!(
                "**Status:** Implemented\n\n## Tasks\n\n{}",
                "- [x] **T1 — Done.**\n- [ ] **T2 — Not done.**\n"
            ),
        )
        .unwrap();

        // When the promise is checked
        let broken = unkept_promises(&[spec]);

        // Then the spec and the task are named
        assert_eq!(broken, vec![TaskId::new("06-demo", "t2")]);
        assert_eq!(broken[0].to_string(), "06-demo/t2");
    }

    #[test]
    fn a_spec_not_marked_implemented_promises_nothing() {
        let spec = parse(
            "09-demo",
            &text("Approved", "- [x] **T1 — Done.**\n- [ ] **T2 — Open.**"),
        )
        .unwrap();
        assert_eq!(unkept_promises(&[spec]), vec![]);
    }

    #[test]
    fn an_implemented_spec_with_every_box_ticked_keeps_its_promise() {
        let spec = parse(
            "09-demo",
            &text(
                "Implemented",
                "- [x] **T1 — Done.**\n- [x] **T2 — Done too.**",
            ),
        )
        .unwrap();
        assert_eq!(unkept_promises(&[spec]), vec![]);
    }
}
