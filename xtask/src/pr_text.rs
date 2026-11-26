use crate::{Findings, git};
use std::path::Path;

const MARKS: [&str; 5] = [
    "🤖",
    "claude.com/claude-code",
    "Claude Code",
    "chatgpt.com/codex",
    "openai.com/codex",
];
const TOOLS: [&str; 6] = ["Claude", "Codex", "ChatGPT", "OpenAI", "GPT", "AI"];
const GENERATED: [&str; 2] = ["Generated ", "generated "];
const CO_AUTHOR: &str = "co-authored-by:";

fn generated(rest: &str) -> bool {
    let Some(rest) = GENERATED.iter().find_map(|g| rest.strip_prefix(g)) else {
        return false;
    };
    let Some(rest) = ["with ", "by "].iter().find_map(|w| rest.strip_prefix(w)) else {
        return false;
    };
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    TOOLS.iter().any(|tool| rest.starts_with(tool))
}

fn co_author(rest: &str) -> bool {
    (rest.get(..CO_AUTHOR.len())).is_some_and(|w| w.eq_ignore_ascii_case(CO_AUTHOR))
}

fn generated_by(line: &str) -> Option<&'static str> {
    if let Some(mark) = MARKS.iter().find(|m| line.contains(*m)) {
        return Some(mark);
    }
    (line.char_indices().map(|(i, _)| &line[i..])).find_map(|rest| {
        if generated(rest) {
            Some("Generated with|by <tool>")
        } else if co_author(rest) {
            Some("a co-author trailer")
        } else {
            None
        }
    })
}

fn lines(what: &str, text: &str) -> Findings {
    (text.lines().enumerate())
        .filter_map(|(k, line)| {
            generated_by(line).map(|form| {
                format!(
                    "pr-text: {what}:{}: `{}` holds a generated-by line ({form}); drop it",
                    k + 1,
                    line.trim()
                )
            })
        })
        .collect()
}

fn messages(root: &Path, range: &str) -> Result<Vec<(String, String)>, String> {
    let ids = git(root, &["log", "--format=%H %h", range])?;
    (ids.lines())
        .map(|line| {
            let (id, short) =
                (line.split_once(' ')).ok_or_else(|| format!("git log printed `{line}`"))?;
            let message = git(root, &["show", "-s", "--format=%B", id])?;
            Ok((short.to_owned(), message))
        })
        .collect()
}

pub(crate) fn check(root: &Path, body: Option<&Path>, base: &str, head: &str) -> Findings {
    let mut findings = Vec::new();
    if let Some(file) = body {
        match std::fs::read_to_string(root.join(file)) {
            Ok(text) => findings.extend(lines("PR body", &text)),
            Err(e) => findings.push(format!("pr-text: {}: {e}", file.display())),
        }
    }
    let range = format!("{base}..{head}");
    match messages(root, &range) {
        Ok(commits) => {
            for (short, message) in commits {
                findings.extend(lines(&format!("commit {short}"), &message));
            }
        }
        Err(e) => findings.push(format!("pr-text: git log {range}: {e}")),
    }
    findings
}
