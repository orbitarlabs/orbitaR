use super::u32_in;
use crate::{Findings, lock::hex};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, ops::Range, path::Path, process::Command};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Local,
    Data,
    Central,
    End,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Self::Local => "local header",
            Self::Data => "stored data",
            Self::Central => "central header",
            Self::End => "end record",
        }
    }
}

type Fields = Vec<(&'static str, Range<usize>)>;

struct Region {
    part: Part,
    member: String,
    at: Range<usize>,
    fields: Fields,
}

struct Header {
    part: Part,
    signature: [u8; 4],
    fixed: &'static [(&'static str, usize)],
    tails: &'static [(&'static str, usize)],
}

const LOCAL: Header = Header {
    part: Part::Local,
    signature: *b"PK\x03\x04",
    fixed: &[
        ("signature", 4),
        ("version needed", 2),
        ("flags", 2),
        ("method", 2),
        ("time", 2),
        ("date", 2),
        ("crc-32", 4),
        ("compressed size", 4),
        ("size", 4),
        ("name length", 2),
        ("extra length", 2),
    ],
    tails: &[("name", 26), ("extra", 28)],
};

const CENTRAL: Header = Header {
    part: Part::Central,
    signature: *b"PK\x01\x02",
    fixed: &[
        ("signature", 4),
        ("version made by", 2),
        ("version needed", 2),
        ("flags", 2),
        ("method", 2),
        ("time", 2),
        ("date", 2),
        ("crc-32", 4),
        ("compressed size", 4),
        ("size", 4),
        ("name length", 2),
        ("extra length", 2),
        ("comment length", 2),
        ("disk", 2),
        ("internal attributes", 2),
        ("external attributes", 4),
        ("local header offset", 4),
    ],
    tails: &[("name", 28), ("extra", 30), ("comment", 32)],
};

const END: Header = Header {
    part: Part::End,
    signature: *b"PK\x05\x06",
    fixed: &[
        ("signature", 4),
        ("disk", 2),
        ("central directory disk", 2),
        ("entries on this disk", 2),
        ("entries", 2),
        ("central directory size", 4),
        ("central directory offset", 4),
        ("comment length", 2),
    ],
    tails: &[("comment", 20)],
};

const END_SIZE: usize = 22;
const HAS_DESCRIPTOR: u16 = 8;
const ZIP64: u16 = 1;

fn zip64(extra: &[u8]) -> bool {
    let mut rest = extra;
    while let [a, b, c, d, tail @ ..] = rest {
        if u16::from_le_bytes([*a, *b]) == ZIP64 {
            return true;
        }
        rest = tail
            .get(usize::from(u16::from_le_bytes([*c, *d]))..)
            .unwrap_or_default();
    }
    false
}

fn u16_in(bytes: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(i..i.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn number(bytes: &[u8], i: usize) -> Result<usize, String> {
    u32_in(bytes, i)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("byte {i} is past the end"))
}

fn read(header: &Header, bytes: &[u8], at: usize, member: &str) -> Result<Region, String> {
    let short = || format!("the {} at byte {at} runs past the end", header.part.name());
    let mut fields = Vec::new();
    let mut end = at;
    for &(name, width) in header.fixed {
        fields.push((name, end..end + width));
        end += width;
    }
    for &(name, length_at) in header.tails {
        let width = usize::from(u16_in(bytes, at + length_at).ok_or_else(short)?);
        fields.push((name, end..end + width));
        end += width;
    }
    let held = bytes.get(at..end).ok_or_else(short)?;
    if !held.starts_with(&header.signature) {
        return Err(format!("byte {at} holds no {}", header.part.name()));
    }
    Ok(Region {
        part: header.part,
        member: member.to_owned(),
        at: at..end,
        fields,
    })
}

fn field<'a>(bytes: &'a [u8], region: &Region, name: &str) -> &'a [u8] {
    (region.fields.iter())
        .find(|(n, _)| *n == name)
        .and_then(|(_, range)| bytes.get(range.clone()))
        .unwrap_or_default()
}

fn layout(bytes: &[u8]) -> Result<Vec<Region>, String> {
    let last = (bytes.len().checked_sub(END_SIZE)).ok_or("no end record closes the file")?;
    let end = (last.saturating_sub(usize::from(u16::MAX))..=last)
        .rev()
        .find(|&i| {
            bytes[i..].starts_with(&END.signature)
                && u16_in(bytes, i + 20).map(usize::from) == Some(bytes.len() - i - END_SIZE)
        })
        .ok_or("no end record closes the file")?;
    let mut regions = vec![read(&END, bytes, end, "")?];
    let entries = u16_in(bytes, end + 10).unwrap_or_default();
    let directory = number(bytes, end + 16)?;
    if directory.checked_add(number(bytes, end + 12)?) != Some(end) {
        return Err("the central directory does not end at the end record".into());
    }
    let mut at = directory;
    for _ in 0..entries {
        let central = read(&CENTRAL, bytes, at, "")?;
        let name = String::from_utf8_lossy(field(bytes, &central, "name")).into_owned();
        let size = number(bytes, at + 20)?;
        let local = read(&LOCAL, bytes, number(bytes, at + 42)?, &name)?;
        let flags = [at + 8, local.at.start + 6].map(|i| u16_in(bytes, i).unwrap_or_default());
        if flags.iter().any(|f| f & HAS_DESCRIPTOR != 0) {
            return Err(format!(
                "member {name} uses a data descriptor; the release wheels never do"
            ));
        }
        if [&central, &local]
            .iter()
            .any(|r| zip64(field(bytes, r, "extra")))
        {
            return Err(format!(
                "member {name} uses a ZIP64 extra field; the release wheels never do"
            ));
        }
        let data = local.at.end..local.at.end + size;
        if bytes.get(data.clone()).is_none() {
            return Err(format!("the stored data of {name} runs past the end"));
        }
        at = central.at.end;
        regions.push(Region {
            part: Part::Data,
            member: name.clone(),
            at: data,
            fields: Vec::new(),
        });
        regions.extend([
            local,
            Region {
                member: name,
                ..central
            },
        ]);
    }
    if at != end {
        return Err(format!(
            "the central directory of {entries} entries ends at byte {at}, not {end}"
        ));
    }
    regions.sort_by_key(|r| (r.at.start, r.at.end));
    let mut cursor = 0;
    for region in &regions {
        if region.at.start != cursor {
            return Err(format!(
                "{} starts at byte {}, but the bytes before it end at {cursor}",
                named(region),
                region.at.start
            ));
        }
        cursor = region.at.end;
    }
    Ok(regions)
}

fn named(region: &Region) -> String {
    match region.member.as_str() {
        "" => format!("the {}", region.part.name()),
        member => format!("the {} of {member}", region.part.name()),
    }
}

fn place(regions: &[Region], at: usize) -> Option<String> {
    let region = regions.iter().find(|r| r.at.contains(&at))?;
    let field = (region.fields.iter())
        .find(|(_, range)| range.contains(&at))
        .map(|(name, _)| format!(", field {name}"));
    Some(named(region) + &field.unwrap_or_default())
}

type Laid = Result<Vec<Region>, String>;

fn first_difference(a: &[u8], b: &[u8], laid: [&Laid; 2]) -> String {
    let at = (a.iter().zip(b))
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let place = (laid.iter())
        .find_map(|l| l.as_ref().ok().and_then(|r| place(r, at)))
        .unwrap_or_else(|| "no region that a layout reads".into());
    if at == a.len() {
        format!(
            "wheel --same: the second wheel holds {} bytes past the end of the first at byte {at}",
            b.len() - at
        )
    } else if at == b.len() {
        format!(
            "wheel --same: the second wheel ends at byte {at}, {} bytes short, in {place}",
            a.len() - at
        )
    } else {
        format!("wheel --same: the first difference is at byte {at}, in {place}")
    }
}

fn differing(region: &Region, bytes: &[u8], other: &Region, theirs: &[u8]) -> Option<String> {
    let fields: Vec<&str> = (region.fields.iter())
        .map(|(name, _)| *name)
        .filter(|name| field(bytes, region, name) != field(theirs, other, name))
        .collect();
    let whole = bytes.get(region.at.clone()) != theirs.get(other.at.clone());
    match (whole, fields.is_empty()) {
        (false, _) => None,
        (true, true) => Some(format!("the {}", region.part.name())),
        (true, false) => Some(format!(
            "the {} ({})",
            region.part.name(),
            fields.join(", ")
        )),
    }
}

fn members(a: (&[Region], &[u8]), b: (&[Region], &[u8]), contents: [&Contents; 2]) -> Findings {
    let mut findings = Vec::new();
    let order = |regions: &[Region]| -> Vec<String> {
        (regions.iter())
            .filter(|r| r.part == Part::Central)
            .map(|r| r.member.clone())
            .collect()
    };
    let (order_a, order_b) = (order(a.0), order(b.0));
    for (name, held, lacks) in (order_a.iter().map(|n| (n, "first", &order_b)))
        .chain(order_b.iter().map(|n| (n, "second", &order_a)))
    {
        if !lacks.contains(name) {
            findings.push(format!("wheel --same: {name} is in the {held} wheel alone"));
        }
    }
    if let Some((x, y)) = (order_a.iter().zip(&order_b)).find(|(x, y)| x != y) {
        findings.push(format!(
            "wheel --same: the member orders part at {x} and {y}"
        ));
    }
    let find = |regions: &'_ [Region], part: Part, member: &str| -> Option<usize> {
        (regions.iter()).position(|r| r.part == part && r.member == member)
    };
    for name in order_a.iter().filter(|n| order_b.contains(n)) {
        let parts: Vec<String> = [Part::Local, Part::Data, Part::Central]
            .into_iter()
            .filter_map(|part| {
                let (i, j) = (find(a.0, part, name)?, find(b.0, part, name)?);
                differing(&a.0[i], a.1, &b.0[j], b.1)
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        let content = match contents.map(|c| c.as_ref().ok().and_then(|c| c.get(name))) {
            [Some(x), Some(y)] if x == y => "; its content agrees",
            [Some(_), Some(_)] => "; its content differs",
            _ => "",
        };
        findings.push(format!(
            "wheel --same: {name} differs in {}{content}",
            parts.join(", ")
        ));
    }
    let ends = [a, b].map(|(regions, _)| regions.iter().find(|r| r.part == Part::End));
    if let [Some(x), Some(y)] = ends
        && let Some(differ) = differing(x, a.1, y, b.1)
    {
        findings.push(format!("wheel --same: the wheels differ in {differ}"));
    }
    findings
}

type Contents = Result<BTreeMap<String, String>, String>;

fn contents(wheel: &Path) -> Result<Vec<(String, String)>, String> {
    let script = "import hashlib, json, os, sys, zipfile\n\
        try:\n    \
        z = zipfile.ZipFile(sys.argv[1])\n    \
        print(json.dumps([[i.filename, hashlib.sha256(z.read(i)).hexdigest()] for i in z.infolist()]))\n\
        except Exception as e:\n    \
        sys.exit(f'{type(e).__name__}: {e}'.replace(sys.argv[1], os.path.basename(sys.argv[1])))";
    let out = Command::new("python3")
        .args(["-I", "-c", script])
        .arg(wheel)
        .output()
        .map_err(|e| format!("python3 zipfile: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let line = (said.lines().rev())
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no message");
        return Err(format!(
            "python3 zipfile cannot read it: {}",
            line.replace(&wheel.display().to_string(), &label(wheel))
        ));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("python3 zipfile printed no member list: {e}"))
}

fn label(wheel: &Path) -> String {
    wheel
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn shown(root: &Path, wheel: &Path) -> String {
    (wheel.strip_prefix(root).ok())
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| label(wheel), |p| p.display().to_string())
}

pub fn hashes(wheel: &Path) -> Result<Vec<String>, String> {
    let named = |e: String| format!("wheel {}: {e}", label(wheel));
    let bytes = fs::read(wheel).map_err(|e| named(e.to_string()))?;
    layout(&bytes).map_err(named)?;
    let head = format!(
        "wheel: {} ({} bytes, sha256 {})",
        label(wheel),
        bytes.len(),
        hex(&Sha256::digest(&bytes))
    );
    let members = contents(wheel).map_err(named)?.into_iter();
    Ok(std::iter::once(head)
        .chain(members.map(|(name, sha)| format!("  {sha}  {name}")))
        .collect())
}

pub(super) fn listed(wheel: &Path) -> Findings {
    match hashes(wheel) {
        Ok(lines) => {
            println!("{}", lines.join("\n"));
            Vec::new()
        }
        Err(e) => vec![e],
    }
}

pub fn same(root: &Path, first: &Path, second: &Path) -> Findings {
    let (first, second) = (root.join(first), root.join(second));
    let names = [shown(root, &first), shown(root, &second)];
    let read = |nth: &str, wheel: &Path, name: &str| {
        fs::read(wheel).map_err(|e| format!("wheel --same: the {nth} wheel {name}: {e}"))
    };
    let (a, b) = match (
        read("first", &first, &names[0]),
        read("second", &second, &names[1]),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return vec![e],
    };
    let sha = |bytes: &[u8]| hex(&Sha256::digest(bytes));
    let [x, y] = &names;
    if a == b {
        return match layout(&a).and_then(|_| contents(&first)) {
            Ok(_) => {
                println!(
                    "wheel --same: {x} and {y} are one byte stream: {} bytes, sha256 {}",
                    a.len(),
                    sha(&a)
                );
                Vec::new()
            }
            Err(e) => vec![
                format!("wheel {x}: {e}"),
                format!("wheel --same: {x} and {y} are one byte stream, but no release wheel"),
            ],
        };
    }
    let laid = [layout(&a), layout(&b)];
    let mut findings = vec![
        format!(
            "wheel --same: {x} ({} bytes, sha256 {}) and {y} ({} bytes, sha256 {}) differ",
            a.len(),
            sha(&a),
            b.len(),
            sha(&b)
        ),
        first_difference(&a, &b, [&laid[0], &laid[1]]),
    ];
    for (name, l) in names.iter().zip(&laid) {
        if let Err(e) = l {
            findings.push(format!("wheel {name}: {e}"));
        }
    }
    if let [Ok(ra), Ok(rb)] = &laid {
        let read = |wheel: &Path| -> Contents { contents(wheel).map(|c| c.into_iter().collect()) };
        let held = [read(&first), read(&second)];
        findings.extend(members((ra, &a), (rb, &b), [&held[0], &held[1]]));
    }
    findings
}
