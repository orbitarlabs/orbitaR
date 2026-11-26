use crate::{Env, Findings, bench};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const DOCKERFILE: &str = "container/Dockerfile";
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn stamp(epoch: i64) -> (String, String) {
    let (days, secs) = (epoch.div_euclid(86_400), epoch.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let name = MONTHS[usize::try_from(month - 1).unwrap_or(0)];
    let time = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    (format!("{name} {day:>2} {year}"), time)
}

fn stamps(bytes: &[u8]) -> Vec<String> {
    let printable = |b: u8| b.is_ascii_graphic() || b == b' ';
    let mut found = Vec::new();
    for run in bytes.split(|b| !printable(*b)) {
        let Ok(text) = std::str::from_utf8(run) else {
            continue;
        };
        let date = text.len() == 11
            && MONTHS.contains(&&text[..3])
            && text.as_bytes()[3] == b' '
            && text[4..6].trim_start().chars().all(|c| c.is_ascii_digit())
            && text.as_bytes()[6] == b' '
            && text[7..].chars().all(|c| c.is_ascii_digit());
        let time = text.len() == 8
            && text.char_indices().all(|(i, c)| {
                if i == 2 || i == 5 {
                    c == ':'
                } else {
                    c.is_ascii_digit()
                }
            });
        if date || time {
            found.push(text.to_owned());
        }
    }
    found
}

pub(crate) fn stamped(what: &str, bytes: &[u8]) -> Findings {
    let (date, time) = stamp(bench::SOURCE_DATE_EPOCH.parse().unwrap_or_default());
    let found = stamps(bytes);
    let mut findings: Findings = (found.iter())
        .filter(|s| **s != date && **s != time)
        .map(|s| format!("reproduce: {what} embeds `{s}`, not the fixed `{date}` `{time}`"))
        .collect();
    if !found.contains(&date) {
        findings.push(format!(
            "reproduce: {what} holds no `{date}`; was it built without SOURCE_DATE_EPOCH, by a plain cargo build?"
        ));
    }
    findings
}

fn dockerfile(root: &Path) -> Findings {
    let want = format!("ENV SOURCE_DATE_EPOCH={}", bench::SOURCE_DATE_EPOCH);
    let text = fs::read_to_string(root.join(DOCKERFILE)).unwrap_or_default();
    let lines: Vec<&str> = (text.lines().map(str::trim))
        .filter(|l| l.starts_with("ENV") && l.contains("SOURCE_DATE_EPOCH"))
        .collect();
    let mut findings: Findings = (lines.iter().filter(|l| **l != want))
        .map(|l| {
            format!(
                "reproduce: {DOCKERFILE} sets `{l}`, not `{want}`, the date that xtask builds with"
            )
        })
        .collect();
    if lines.is_empty() {
        findings.push(format!(
            "reproduce: {DOCKERFILE} lacks `{want}`, the date that xtask builds with"
        ));
    }
    findings
}

pub(crate) fn check(root: &Path, file: Option<&Path>, env: &Env) -> Findings {
    let mut findings = dockerfile(root);
    if let Some(file) = file {
        match fs::read(root.join(file)) {
            Ok(bytes) => findings.extend(stamped(&file.display().to_string(), &bytes)),
            Err(e) => findings.push(format!("{}: {e}", file.display())),
        }
        return findings;
    }
    findings.extend(twice(root, env));
    findings
}

fn twice(root: &Path, env: &Env) -> Findings {
    let mut findings = Vec::new();
    let mut digests = Vec::new();
    for side in ["a", "b"] {
        let target = root.join("target/reproduce").join(side);
        let _ = fs::remove_dir_all(&target);
        let bin = match bench::engine(env, root, &target) {
            Ok(bin) => bin,
            Err(failed) => return failed,
        };
        match fs::read(&bin) {
            Ok(bytes) => {
                findings.extend(stamped(&format!("build {side}"), &bytes));
                digests.push(crate::lock::hex(&Sha256::digest(&bytes)));
            }
            Err(e) => return vec![format!("{}: {e}", bin.display())],
        }
    }
    println!("reproduce: engine sha256 {}", digests.join(" "));
    if digests.first() != digests.last() {
        findings.push(format!(
            "reproduce: two clean builds differ: {}; the engine build is not reproducible",
            digests.join(" ≠ ")
        ));
    }
    findings
}
