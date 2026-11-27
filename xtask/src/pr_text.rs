use crate::{Findings, commits};
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

pub(crate) fn check(root: &Path, body: Option<&Path>, base: &str, head: &str) -> Findings {
    let mut findings = Vec::new();
    if let Some(file) = body {
        match std::fs::read_to_string(root.join(file)) {
            Ok(text) => findings.extend(lines("PR body", &text)),
            Err(e) => findings.push(format!("pr-text: {}: {e}", file.display())),
        }
    }
    let range = format!("{base}..{head}");
    match commits(root, &range) {
        Ok(commits) => {
            for commit in commits {
                let (_, message) = commit.headers_and_message();
                findings.extend(lines(&format!("commit {}", commit.short), message));
            }
        }
        Err(e) => findings.push(format!("pr-text: git log {range}: {e}")),
    }
    findings
}
