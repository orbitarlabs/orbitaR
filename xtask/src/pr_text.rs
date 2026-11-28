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
const MAX_PROSE_LINES: usize = 30;
const MAX_CORE_LINES: usize = 8;

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

fn without_comments(mut line: &str, in_comment: &mut bool) -> String {
    let mut visible = String::new();
    loop {
        let delimiter = if *in_comment { "-->" } else { "<!--" };
        let Some((before, after)) = line.split_once(delimiter) else {
            if !*in_comment {
                visible.push_str(line);
            }
            return visible;
        };
        if !*in_comment {
            visible.push_str(before);
        }
        *in_comment = !*in_comment;
        line = after;
    }
}

fn fence(raw: &str) -> Option<(char, usize, &str)> {
    let line = raw.trim_start_matches(' ');
    let marker = line.chars().next()?;
    if raw.len() - line.len() > 3 || !matches!(marker, '`' | '~') {
        return None;
    }
    let rest = line.trim_start_matches(marker);
    let width = line.len() - rest.len();
    (width >= 3).then_some((marker, width, rest.trim()))
}

struct Fence {
    marker: char,
    width: usize,
    core_lines: Option<usize>,
}

fn body_format(text: &str) -> Findings {
    let mut findings = Vec::new();
    let mut required = ["Proof:", "Bars moved:", "Review:"].map(|label| (label, false));
    let mut first = true;
    let mut in_comment = false;
    let mut after_core_label = false;
    let mut prose_lines = 0;
    let mut fenced: Option<Fence> = None;
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        if let Some(block) = &mut fenced {
            if let Some((marker, width, "")) = fence(raw)
                && marker == block.marker
                && width >= block.width
            {
                fenced = None;
            } else if let Some(count) = &mut block.core_lines {
                *count += 1;
                if *count == MAX_CORE_LINES + 1 {
                    findings.push(format!(
                        "pr-text: PR body:{number}: Core change: more than {MAX_CORE_LINES} lines in the diff block"
                    ));
                }
            }
            continue;
        }

        let visible = without_comments(raw, &mut in_comment);
        let line = visible.trim();
        if line.is_empty() {
            continue;
        }
        if first && !line.starts_with("Intent:") {
            findings.push(format!(
                "pr-text: PR body:{number}: first non-empty line must start with Intent:"
            ));
        }
        first = false;
        if let Some((marker, width, info)) = fence(&visible)
            && (marker != '`' || !info.contains('`'))
        {
            fenced = Some(Fence {
                marker,
                width,
                core_lines: (after_core_label && info.split_whitespace().next() == Some("diff"))
                    .then_some(0),
            });
            in_comment = false;
            after_core_label = false;
            continue;
        }
        after_core_label = line.starts_with("Core change:");
        for (label, found) in &mut required {
            *found |= line.starts_with(*label);
        }
        prose_lines += 1;
        if prose_lines == MAX_PROSE_LINES + 1 {
            findings.push(format!(
                "pr-text: PR body:{number}: more than {MAX_PROSE_LINES} non-empty lines outside fenced code blocks"
            ));
        }
    }
    if first {
        findings.push("pr-text: PR body: missing Intent: on the first non-empty line".into());
    }
    for (label, found) in required {
        if !found {
            findings.push(format!("pr-text: PR body: missing {label} line"));
        }
    }
    findings
}

pub(crate) fn check(root: &Path, body: Option<&Path>, base: &str, head: &str) -> Findings {
    let mut findings = Vec::new();
    if let Some(file) = body {
        match std::fs::read_to_string(root.join(file)) {
            Ok(text) => {
                findings.extend(lines("PR body", &text));
                findings.extend(body_format(&text));
            }
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
