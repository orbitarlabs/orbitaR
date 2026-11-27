use crate::{has, lever, tree};
use sha2::Digest;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const SAME: [(&str, &str); 2] = [("orbitar/a.py", "a = 1\n"), ("orbitar/b.py", "b = 2\n")];
const OTHER_B: [(&str, &str); 2] = [SAME[0], ("orbitar/b.py", "b = 3\n")];

struct Spec<'a> {
    members: &'a [(&'a str, &'a str)],
    date: u16,
    comment: &'a str,
    variant: &'a str,
}

const PLAIN: Spec = Spec {
    members: &SAME,
    date: 2026,
    comment: "",
    variant: "plain",
};

const WRITE: &str = "import io, json, struct, sys, zipfile
path, members, year, comment, variant = sys.argv[1:6]
class Pipe(io.RawIOBase):
    def __init__(self, f): self.f = f
    def writable(self): return True
    def write(self, b): return self.f.write(b)
with open(path, 'wb') as f:
    out = f if variant in ('plain', 'zip64') else Pipe(f)
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
        z.comment = comment.encode()
        for p, t in json.loads(members):
            info = zipfile.ZipInfo(p, (int(year), 1, 1, 0, 0, 0))
            with z.open(info, 'w', force_zip64=variant == 'zip64') as m:
                m.write(t.encode())
if variant == 'unsigned':
    data = open(path, 'rb').read()
    z = zipfile.ZipFile(path)
    out, moved = b'', {}
    for i in z.infolist():
        start = i.header_offset
        n, m = struct.unpack('<HH', data[start + 26:start + 30])
        end = start + 30 + n + m + i.compress_size
        assert data[end:end + 4] == b'PK\\x07\\x08'
        moved[start] = len(out)
        out += data[start:end] + data[end + 4:end + 16]
    directory = len(out)
    at = z.start_dir
    for i in z.infolist():
        n, m, k = struct.unpack('<HHH', data[at + 28:at + 34])
        entry = bytearray(data[at:at + 46 + n + m + k])
        entry[42:46] = struct.pack('<I', moved[i.header_offset])
        out += entry
        at += len(entry)
    end = bytearray(data[at:])
    end[12:20] = struct.pack('<II', len(out) - directory, directory)
    open(path, 'wb').write(out + end)
";

fn zipped(root: &Path, name: &str, spec: &Spec) -> PathBuf {
    let path = root.join(name);
    let status = Command::new("python3")
        .args(["-I", "-c", WRITE])
        .arg(&path)
        .arg(serde_json::to_string(spec.members).unwrap())
        .arg(spec.date.to_string())
        .arg(spec.comment)
        .arg(spec.variant)
        .status()
        .unwrap();
    assert!(status.success());
    path
}

fn written(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn compared(root: &Path, a: &Path, b: &Path) -> Vec<String> {
    let found = xtask::same(root, a, b);
    let home = [root.display().to_string(), std::env::var("HOME").unwrap()];
    for f in &found {
        assert!(!home.iter().any(|h| f.contains(h.as_str())), "{found:?}");
        assert!(!f.contains('\n') && !f.contains("Traceback"), "{found:?}");
    }
    found
}

#[test]
fn two_wheels_of_one_byte_stream_are_the_same() {
    let root = tree("same-equal", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let b = zipped(&root, "b.whl", &PLAIN);
    assert_eq!(compared(&root, &a, &b), Vec::<String>::new());
}

#[test]
fn one_byte_stream_that_is_no_wheel_is_no_same_wheel() {
    let root = tree("same-no-wheel", &[]);
    let full = fs::read(zipped(&root, "full.whl", &PLAIN)).unwrap();
    let mut bad_data = full.clone();
    bad_data[42] ^= 1;
    let mut far_directory = full.clone();
    let end = full.len() - 22;
    far_directory[end + 16] ^= 1;
    let mut more_entries = full.clone();
    more_entries[end + 10] += 1;
    let prefixed = [b"junk!".as_slice(), &full].concat();
    for (name, bytes, want) in [
        (
            "text",
            b"not a wheel".to_vec(),
            "no end record closes the file",
        ),
        (
            "truncated",
            full[..full.len() - 10].to_vec(),
            "no end record closes the file",
        ),
        (
            "bad-data",
            bad_data,
            "python3 zipfile cannot read it: BadZipFile: Bad CRC-32 for file 'orbitar/a.py'",
        ),
        (
            "far-directory",
            far_directory,
            "the central directory does not end at the end record",
        ),
        ("more-entries", more_entries, "byte "),
        ("prefixed", prefixed, "the end record"),
    ] {
        let a = written(&root, &format!("{name}-a.whl"), &bytes);
        let b = written(&root, &format!("{name}-b.whl"), &bytes);
        let found = compared(&root, &a, &b);
        assert_eq!(found.len(), 2, "{name}: {found:?}");
        assert!(
            found[0].starts_with(&format!("wheel {name}-a.whl: ")) && found[0].contains(want),
            "{name}: {found:?}"
        );
        assert!(
            has(&found, "are one byte stream, but no release wheel"),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn a_wheel_and_its_superset_differ_past_the_end() {
    let root = tree("same-superset", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let full = fs::read(&a).unwrap();
    let b = written(&root, "b.whl", &[full.as_slice(), b"junk!"].concat());
    let found = compared(&root, &a, &b);
    let want = format!(
        "wheel --same: the second wheel holds 5 bytes past the end of the first at byte {}",
        full.len()
    );
    assert!(has(&found, &want), "{found:?}");
    assert!(
        has(&found, "wheel b.whl: no end record closes the file"),
        "{found:?}"
    );
    let found = compared(&root, &b, &a);
    let want = format!(
        "wheel --same: the second wheel ends at byte {}, 5 bytes short, in no region that a layout reads",
        full.len()
    );
    assert!(has(&found, &want), "{found:?}");
    assert!(
        has(&found, "wheel b.whl: no end record closes the file"),
        "{found:?}"
    );
}

#[test]
fn a_truncated_copy_names_the_field_where_it_ends() {
    let root = tree("same-truncated", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let full = fs::read(&a).unwrap();
    let b = written(&root, "b.whl", &full[..20]);
    let found = compared(&root, &a, &b);
    let want = format!(
        "wheel --same: the second wheel ends at byte 20, {} bytes short, in the local header of orbitar/a.py, field compressed size",
        full.len() - 20
    );
    assert!(has(&found, &want), "{found:?}");
}

#[test]
fn a_change_in_a_local_header_alone_is_named_by_its_field() {
    let root = tree("same-local", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let mut bytes = fs::read(&a).unwrap();
    bytes[10] ^= 1;
    let b = written(&root, "b.whl", &bytes);
    let found = compared(&root, &a, &b);
    assert_eq!(
        found[1..],
        [
            "wheel --same: the first difference is at byte 10, in the local header of orbitar/a.py, field time",
            "wheel --same: orbitar/a.py differs in the local header (time); its content agrees",
        ],
        "{found:?}"
    );
    assert!(
        has(&found[..1], "wheel --same: a.whl (") && has(&found[..1], ") and b.whl ("),
        "{found:?}"
    );
}

#[test]
fn a_change_in_the_stored_data_alone_is_named_outside_both_headers() {
    let root = tree("same-data", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let mut bytes = fs::read(&a).unwrap();
    bytes[42] ^= 1;
    let b = written(&root, "b.whl", &bytes);
    let found = compared(&root, &a, &b);
    assert_eq!(
        found[1..],
        [
            "wheel --same: the first difference is at byte 42, in the stored data of orbitar/a.py",
            "wheel --same: orbitar/a.py differs in the stored data",
        ],
        "{found:?}"
    );
}

#[test]
fn two_wheels_that_differ_name_each_differing_member_and_header() {
    let root = tree("same-differ", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    let more = [SAME[0], SAME[1], ("orbitar/c.py", "")];
    let turned = [SAME[1], SAME[0]];
    for (name, spec, wants) in [
        (
            "content",
            Spec {
                members: &OTHER_B,
                ..PLAIN
            },
            vec![
                "wheel --same: orbitar/b.py differs in the local header (crc-32",
                "the stored data, the central header (crc-32",
                "; its content differs",
            ],
        ),
        (
            "alone",
            Spec {
                members: &more,
                ..PLAIN
            },
            vec![
                "wheel --same: orbitar/c.py is in the second wheel alone",
                "wheel --same: the wheels differ in the end record (entries on this disk, entries, central directory size, central directory offset)",
            ],
        ),
        (
            "order",
            Spec {
                members: &turned,
                ..PLAIN
            },
            vec!["wheel --same: the member orders part at orbitar/a.py and orbitar/b.py"],
        ),
        (
            "date",
            Spec {
                date: 2025,
                ..PLAIN
            },
            vec![
                "wheel --same: orbitar/a.py differs in the local header (date), the central header (date); its content agrees",
            ],
        ),
        (
            "comment",
            Spec {
                comment: "SECRET",
                ..PLAIN
            },
            vec!["wheel --same: the wheels differ in the end record (comment length, comment)"],
        ),
    ] {
        let b = zipped(&root, &format!("{name}.whl"), &spec);
        let found = compared(&root, &a, &b);
        assert!(
            has(&found, "wheel --same: the first difference is at byte "),
            "{name}: {found:?}"
        );
        for want in wants {
            assert!(has(&found, want), "{name}: {found:?}");
        }
        assert!(
            !has(&found, "SECRET") && !has(&found, "534543524554"),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn a_wheel_whose_members_use_a_data_descriptor_or_zip64_is_refused_at_the_command() {
    let root = tree("same-descriptor", &[]);
    zipped(&root, "plain.whl", &PLAIN);
    let newline = [("orbitar/a\nb.py", "a = 1\n")];
    for (stem, variant, members, member, want) in [
        (
            "signed",
            "signed",
            &SAME[..],
            "orbitar/a.py",
            "a data descriptor",
        ),
        (
            "unsigned",
            "unsigned",
            &SAME[..],
            "orbitar/a.py",
            "a data descriptor",
        ),
        (
            "zip64",
            "zip64",
            &SAME[..],
            "orbitar/a.py",
            "a ZIP64 extra field",
        ),
        (
            "signed-newline",
            "signed",
            &newline[..],
            "orbitar/a\\nb.py",
            "a data descriptor",
        ),
        (
            "zip64-newline",
            "zip64",
            &newline[..],
            "orbitar/a\\nb.py",
            "a ZIP64 extra field",
        ),
    ] {
        let name = format!("{stem}.whl");
        let spec = Spec {
            members,
            variant,
            ..PLAIN
        };
        let wheel = zipped(&root, &name, &spec);
        let want =
            format!("wheel {name}: member {member} uses {want}; the release wheels never do");
        let found = lever(&["wheel", "--same", &name, &name], &root);
        assert_eq!(
            found,
            [
                want.clone(),
                format!(
                    "wheel --same: {name} and {name} are one byte stream, but no release wheel"
                ),
            ],
            "{name}"
        );
        let found = lever(&["wheel", "--same", "plain.whl", &name], &root);
        assert!(has(&found, &want), "{name}: {found:?}");
        assert!(!found.iter().any(|f| f.contains('\n')), "{name}: {found:?}");
        assert_eq!(xtask::hashes(&wheel), Err(want), "{name}");
    }
}

#[test]
fn a_wheel_that_cannot_be_read_is_no_same_wheel() {
    let root = tree("same-missing", &[]);
    let a = zipped(&root, "a.whl", &PLAIN);
    for (x, y, want) in [
        (
            a.clone(),
            root.join("gone.whl"),
            "wheel --same: the second wheel gone.whl: ",
        ),
        (
            root.join("gone.whl"),
            a.clone(),
            "wheel --same: the first wheel gone.whl: ",
        ),
    ] {
        let found = compared(&root, &x, &y);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(has(&found, want), "{found:?}");
    }
}

#[test]
fn wheel_same_compares_two_files_under_the_root() {
    let root = tree("same-lever", &[]);
    zipped(&root, "a.whl", &PLAIN);
    zipped(&root, "b.whl", &PLAIN);
    zipped(
        &root,
        "c.whl",
        &Spec {
            members: &OTHER_B,
            ..PLAIN
        },
    );
    assert_eq!(
        lever(&["wheel", "--same", "a.whl", "b.whl"], &root),
        Vec::<String>::new()
    );
    let found = lever(&["wheel", "--same", "a.whl", "c.whl"], &root);
    assert!(
        has(&found, "wheel --same: orbitar/b.py differs in "),
        "{found:?}"
    );
    assert!(has(&found, "wheel --same: a.whl ("), "{found:?}");
}

#[test]
fn a_listed_wheel_names_its_file_and_the_sha256_of_each_member() {
    let root = tree("listed", &[]);
    let wheel = zipped(&root, "a.whl", &PLAIN);
    let lines = xtask::hashes(&wheel).unwrap();
    let size = fs::metadata(&wheel).unwrap().len();
    assert!(
        lines[0].starts_with(&format!("wheel: a.whl ({size} bytes, sha256 ")),
        "{lines:?}"
    );
    let a: String = (sha2::Sha256::digest(b"a = 1\n").iter())
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(lines[1], format!("  {a}  orbitar/a.py"), "{lines:?}");
    assert_eq!(lines.len(), 3, "{lines:?}");
    let home = root.display().to_string();
    let gone = xtask::hashes(&root.join("gone.whl")).unwrap_err();
    assert!(
        gone.starts_with("wheel gone.whl: ") && !gone.contains(&home),
        "{gone}"
    );
    let text = written(&root, "text.whl", b"not a wheel");
    let text = xtask::hashes(&text).unwrap_err();
    assert_eq!(text, "wheel text.whl: no end record closes the file");
    let mut bytes = fs::read(&wheel).unwrap();
    bytes[42] ^= 1;
    let bad = written(&root, "bad.whl", &bytes);
    assert_eq!(
        xtask::hashes(&bad),
        Err("wheel bad.whl: python3 zipfile cannot read it: BadZipFile: Bad CRC-32 for file 'orbitar/a.py'".to_owned())
    );
}

#[test]
fn a_descriptor_bit_in_a_later_central_header_alone_is_refused() {
    let root = tree("same-central-bit", &[]);
    let mut bytes = fs::read(zipped(&root, "plain.whl", &PLAIN)).unwrap();
    let end = bytes.len() - 22;
    let directory = usize::try_from(u32::from_le_bytes(
        bytes[end + 16..end + 20].try_into().unwrap(),
    ))
    .unwrap();
    bytes[directory + 46 + SAME[0].0.len() + 8] |= 8;
    written(&root, "central.whl", &bytes);
    let found = lever(&["wheel", "--same", "central.whl", "central.whl"], &root);
    assert!(
        has(
            &found,
            "wheel central.whl: member orbitar/b.py uses a data descriptor; the release wheels never do"
        ),
        "{found:?}"
    );
}

#[test]
fn a_listed_wheel_that_is_refused_or_unreadable_is_a_finding() {
    let root = tree("listed-findings", &[]);
    let good = zipped(&root, "good.whl", &PLAIN);
    assert_eq!(xtask::listed(&good), Vec::<String>::new());
    let signed = zipped(
        &root,
        "signed.whl",
        &Spec {
            variant: "signed",
            ..PLAIN
        },
    );
    let text = written(&root, "text.whl", b"not a wheel");
    for (wheel, want) in [
        (
            signed,
            "wheel signed.whl: member orbitar/a.py uses a data descriptor",
        ),
        (text, "wheel text.whl: no end record closes the file"),
        (root.join("gone.whl"), "wheel gone.whl: "),
    ] {
        let found = xtask::listed(&wheel);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(has(&found, want), "{found:?}");
    }
}
