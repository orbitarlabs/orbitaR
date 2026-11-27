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
    streamed: bool,
}

const PLAIN: Spec = Spec {
    members: &SAME,
    date: 2026,
    comment: "",
    streamed: false,
};

fn zipped(root: &Path, name: &str, spec: &Spec) -> PathBuf {
    let path = root.join(name);
    let status = Command::new("python3")
        .args([
            "-I",
            "-c",
            "import io, json, sys, zipfile\n\
             class Pipe(io.RawIOBase):\n    \
             def __init__(self, f): self.f = f\n    \
             def writable(self): return True\n    \
             def write(self, b): return self.f.write(b)\n\
             with open(sys.argv[1], 'wb') as f:\n    \
             out = Pipe(f) if sys.argv[5] == '1' else f\n    \
             with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:\n        \
             z.comment = sys.argv[4].encode()\n        \
             for p, t in json.loads(sys.argv[2]):\n            \
             z.writestr(zipfile.ZipInfo(p, (int(sys.argv[3]), 1, 1, 0, 0, 0)), t)",
        ])
        .arg(&path)
        .arg(serde_json::to_string(spec.members).unwrap())
        .arg(spec.date.to_string())
        .arg(spec.comment)
        .arg(if spec.streamed { "1" } else { "0" })
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
    let home = root.display().to_string();
    assert!(!found.iter().any(|f| f.contains(&home)), "{found:?}");
    found
}

#[test]
fn two_wheels_of_one_byte_stream_are_the_same() {
    let root = tree("same-equal", &[]);
    for (name, streamed) in [("plain", false), ("streamed", true)] {
        let spec = Spec { streamed, ..PLAIN };
        let a = zipped(&root, &format!("{name}-a.whl"), &spec);
        let b = zipped(&root, &format!("{name}-b.whl"), &spec);
        assert_eq!(compared(&root, &a, &b), Vec::<String>::new(), "{name}");
    }
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
    let mut descriptor_flag = full.clone();
    descriptor_flag[6] |= 8;
    let central = end - 2 * (46 + 12);
    descriptor_flag[central + 8] |= 8;
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
        ("bad-data", bad_data, "python3 zipfile"),
        (
            "far-directory",
            far_directory,
            "the central directory does not end at the end record",
        ),
        ("more-entries", more_entries, "byte "),
        (
            "descriptor-flag",
            descriptor_flag,
            "holds no data descriptor",
        ),
        ("prefixed", prefixed, "the end record"),
    ] {
        let a = written(&root, &format!("{name}-a.whl"), &bytes);
        let b = written(&root, &format!("{name}-b.whl"), &bytes);
        let found = compared(&root, &a, &b);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(
            has(&found, "are one byte stream, but no wheel: "),
            "{name}: {found:?}"
        );
        assert!(has(&found, want), "{name}: {found:?}");
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
        has(&found, "wheel --same: the second wheel is no wheel: "),
        "{found:?}"
    );
    let found = compared(&root, &b, &a);
    let want = format!(
        "wheel --same: the second wheel ends at byte {}, 5 bytes short, in no region that a layout reads",
        full.len()
    );
    assert!(has(&found, &want), "{found:?}");
    assert!(
        has(&found, "wheel --same: the first wheel is no wheel: "),
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
fn a_data_descriptor_is_compared_as_its_own_part() {
    let root = tree("same-descriptor", &[]);
    let streamed = Spec {
        streamed: true,
        ..PLAIN
    };
    let a = zipped(&root, "a.whl", &streamed);
    let b = zipped(
        &root,
        "b.whl",
        &Spec {
            members: &OTHER_B,
            ..streamed
        },
    );
    let found = compared(&root, &a, &b);
    assert!(
        has(&found, "the stored data, the data descriptor (crc-32"),
        "{found:?}"
    );
    let plain = zipped(&root, "plain.whl", &PLAIN);
    let found = compared(&root, &plain, &a);
    assert!(
        has(&found, "the data descriptor in the second wheel alone"),
        "{found:?}"
    );
    let found = compared(&root, &a, &plain);
    assert!(
        has(&found, "the data descriptor in the first wheel alone"),
        "{found:?}"
    );
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
        gone.starts_with("gone.whl: ") && !gone.contains(&home),
        "{gone}"
    );
    let text = written(&root, "text.whl", b"not a wheel");
    let text = xtask::hashes(&text).unwrap_err();
    assert_eq!(text, "text.whl: no end record closes the file");
}
