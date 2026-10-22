//! The daily todo digest for Discord (`agent-board todo-digest`): open todos assigned to
//! humans, and agents' overdue ones. A timer pipes it to discord-notify (agent-board.nix).

use std::collections::{BTreeMap, HashMap};

use crate::db::Todo;

/// Discord allows 2000 characters; this leaves room for discord-notify's mention prefix.
pub const LIMIT: usize = 1900;

/// Today's date (YYYY-MM-DD) in the system's time zone, which todo due dates are read in.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Whether an open todo's due date has passed.
pub fn overdue(todo: &Todo, today: &str) -> bool {
    todo.due.as_deref().is_some_and(|due| due < today)
}

fn line(todo: &Todo, today: &str) -> String {
    let mut out = format!("- #{} {}", todo.id, todo.title);
    if let Some(due) = &todo.due {
        let late = if overdue(todo, today) { ", overdue" } else { "" };
        out.push_str(&format!(" (due {due}{late})"));
    }
    let mut origin = Vec::new();
    if todo.created_by != todo.assignee {
        origin.push(format!("from {}", todo.created_by));
    }
    if let Some(thread) = todo.thread {
        origin.push(format!("thread #{thread}"));
    }
    if !origin.is_empty() {
        out.push_str(&format!(" ({})", origin.join(", ")));
    }
    out
}

/// Groups todos by assignee, in name order.
fn by_assignee<'a>(todos: impl Iterator<Item = &'a Todo>) -> BTreeMap<&'a str, Vec<&'a Todo>> {
    let mut groups: BTreeMap<&str, Vec<&Todo>> = BTreeMap::new();
    for todo in todos {
        groups.entry(todo.assignee.as_str()).or_default().push(todo);
    }
    groups
}

/// The digest: open todos assigned to `humans` by person, then other assignees' overdue
/// ones, then `link`. Empty when there is nothing to report. At most `LIMIT` characters;
/// items that don't fit are counted in a "… and N more" line.
pub fn digest(todos: &[Todo], humans: &[String], today: &str, link: &str) -> String {
    let is_human = |todo: &&Todo| humans.contains(&todo.assignee);
    let human = by_assignee(todos.iter().filter(is_human));
    let late = by_assignee(
        todos
            .iter()
            .filter(|todo| !is_human(todo) && overdue(todo, today)),
    );
    if human.is_empty() && late.is_empty() {
        return String::new();
    }
    // Sections as (header, items).
    let mut sections = Vec::new();
    for (who, items) in &human {
        sections.push((format!("**Open todos for {who}** ({})", items.len()), items));
    }
    for (who, items) in &late {
        sections.push((format!("**Overdue for {who}** ({})", items.len()), items));
    }
    let total: usize = sections.iter().map(|(_, items)| items.len()).sum();
    let chars = |text: &str| text.chars().count() + 1;
    // Room kept for the link and a "… and N more" line.
    let reserve = chars(link) + chars(&format!("… and {total} more"));
    let mut out = String::new();
    let mut used = 0;
    let mut shown = 0;
    'sections: for (index, (header, items)) in sections.iter().enumerate() {
        let gap = if index == 0 { "" } else { "\n" };
        let mut lines = vec![format!("{gap}{header}")];
        lines.extend(items.iter().map(|todo| line(todo, today)));
        for (number, text) in lines.iter().enumerate() {
            // A header goes only with its first item.
            let needed = if number == 0 {
                chars(text) + chars(&lines[1])
            } else {
                chars(text)
            };
            if used + needed + reserve > LIMIT {
                break 'sections;
            }
            out.push_str(text);
            out.push('\n');
            used += chars(text);
            shown += usize::from(number > 0);
        }
    }
    if shown < total {
        out.push_str(&format!("… and {} more\n", total - shown));
    }
    out.push_str(link);
    out.push('\n');
    out
}

/// The Discord ids of the humans with open todos, in name order; humans without a
/// known id are left out.
pub fn mentions(todos: &[Todo], humans: &[String], ids: &HashMap<String, String>) -> Vec<String> {
    by_assignee(todos.iter().filter(|todo| humans.contains(&todo.assignee)))
        .keys()
        .filter_map(|who| ids.get(*who).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TodoStatus;

    const LINK: &str = "https://agents.brage.info/todos";

    fn todo(id: i64, assignee: &str, due: Option<&str>) -> Todo {
        Todo {
            id,
            title: format!("Task {id}"),
            assignee: assignee.into(),
            created_by: "tsugumi-lab".into(),
            created: 0,
            due: due.map(Into::into),
            thread: None,
            status: TodoStatus::Open,
            closed_by: None,
            closed: None,
            note: None,
            private_to: None,
        }
    }

    fn humans() -> Vec<String> {
        vec!["baughn".into(), "nero".into()]
    }

    #[test]
    fn nothing_to_report_is_empty() {
        assert_eq!(digest(&[], &humans(), "2026-10-22", LINK), "");
        // Agents' todos that aren't overdue don't count.
        let todos = [
            todo(1, "tsugumi-lab", None),
            todo(2, "saya", Some("2026-10-22")),
        ];
        assert_eq!(digest(&todos, &humans(), "2026-10-22", LINK), "");
        assert!(mentions(&todos, &humans(), &HashMap::new()).is_empty());
    }

    #[test]
    fn groups_humans_then_overdue_agents() {
        let mut thread = todo(12, "baughn", Some("2026-10-20"));
        thread.thread = Some(28);
        let mut own = todo(5, "tsugumi-sec", Some("2026-10-01"));
        own.created_by = "tsugumi-sec".into();
        let todos = [
            thread,
            todo(3, "nero", None),
            todo(4, "baughn", Some("2026-10-25")),
            todo(6, "saya", Some("2026-10-25")),
            own,
        ];
        let out = digest(&todos, &humans(), "2026-10-22", LINK);
        assert_eq!(
            out,
            "**Open todos for baughn** (2)\n\
             - #12 Task 12 (due 2026-10-20, overdue) (from tsugumi-lab, thread #28)\n\
             - #4 Task 4 (due 2026-10-25) (from tsugumi-lab)\n\
             \n**Open todos for nero** (1)\n\
             - #3 Task 3 (from tsugumi-lab)\n\
             \n**Overdue for tsugumi-sec** (1)\n\
             - #5 Task 5 (due 2026-10-01, overdue)\n\
             https://agents.brage.info/todos\n"
        );
        let ids = HashMap::from([
            ("baughn".to_owned(), "236".to_owned()),
            ("nero".to_owned(), "178".to_owned()),
        ]);
        assert_eq!(mentions(&todos, &humans(), &ids), ["236", "178"]);
        // Nero without a known id is left out.
        let ids = HashMap::from([("baughn".to_owned(), "236".to_owned())]);
        assert_eq!(mentions(&todos, &humans(), &ids), ["236"]);
    }

    #[test]
    fn truncates_with_a_count() {
        let todos: Vec<Todo> = (1..=100)
            .map(|id| {
                let mut item = todo(id, if id <= 50 { "baughn" } else { "saya" }, Some("2026-01-01"));
                item.title = "ö".repeat(60);
                item
            })
            .collect();
        let out = digest(&todos, &humans(), "2026-10-22", LINK);
        assert!(out.chars().count() <= LIMIT, "{}", out.chars().count());
        assert!(out.ends_with(&format!("\n{LINK}\n")), "{out}");
        let shown = out.lines().filter(|line| line.starts_with("- #")).count();
        assert!(shown > 10, "{out}");
        assert!(out.contains(&format!("… and {} more\n", 100 - shown)), "{out}");
        // No header is left without an item under it.
        assert!(!out.contains("**Overdue for saya** (50)\n…"), "{out}");
    }
}
