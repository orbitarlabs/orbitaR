use crate::{has, lever, tree};

const DOCKERFILE: &str = "FROM x AS toolchain\nENV PATH=/opt/bin\nENV SOURCE_DATE_EPOCH=1767225600\n# SOURCE_DATE_EPOCH fixes __DATE__\n";

fn checked(name: &str, strings: &[&str]) -> Vec<String> {
    let mut bytes = b"\x7fELF\0mimalloc: options\0".to_vec();
    for s in strings {
        bytes.extend_from_slice(s.as_bytes());
        bytes.push(0);
    }
    let root = tree(
        &format!("reproduce-{name}"),
        &[("container/Dockerfile", DOCKERFILE)],
    );
    std::fs::write(root.join("orbitar"), bytes).unwrap();
    lever(&["reproduce", "--check", "orbitar"], &root)
}

#[test]
fn a_binary_with_the_fixed_date_and_time_passes() {
    assert_eq!(
        checked("fixed", &["Jan  1 2026", "00:00:00"]),
        Vec::<String>::new()
    );
}

#[test]
fn a_binary_with_the_date_of_its_build_fails() {
    let found = checked("today", &["Nov 15 2026", "01:50:52"]);
    assert!(
        has(&found, "embeds `Nov 15 2026`, not the fixed `Jan  1 2026`"),
        "{found:?}"
    );
    assert!(has(&found, "embeds `01:50:52`"), "{found:?}");
    assert!(has(&found, "holds no `Jan  1 2026`"), "{found:?}");
}

#[test]
fn a_binary_with_no_date_fails() {
    let found = checked("none", &[]);
    assert!(has(&found, "holds no `Jan  1 2026`"), "{found:?}");
}

#[test]
fn a_dockerfile_with_another_build_date_fails() {
    let root = tree(
        "reproduce-dockerfile",
        &[(
            "container/Dockerfile",
            "ENV SOURCE_DATE_EPOCH=1767225600\nFROM y\nENV SOURCE_DATE_EPOCH=86400\n",
        )],
    );
    std::fs::write(root.join("orbitar"), b"Jan  1 2026\0").unwrap();
    let found = lever(&["reproduce", "--check", "orbitar"], &root);
    assert!(
        has(&found, "sets `ENV SOURCE_DATE_EPOCH=86400`, not"),
        "{found:?}"
    );
    std::fs::write(root.join("container/Dockerfile"), "FROM y\n").unwrap();
    let found = lever(&["reproduce", "--check", "orbitar"], &root);
    assert!(
        has(&found, "lacks `ENV SOURCE_DATE_EPOCH=1767225600`"),
        "{found:?}"
    );
}

#[test]
fn the_stamp_of_an_epoch_is_gccs_date_and_time_in_utc() {
    assert_eq!(
        xtask::stamp(1_767_225_600),
        ("Jan  1 2026".into(), "00:00:00".into())
    );
    assert_eq!(
        xtask::stamp(1_709_212_025),
        ("Feb 29 2024".into(), "13:07:05".into())
    );
    assert_eq!(xtask::stamp(0), ("Jan  1 1970".into(), "00:00:00".into()));
    assert_eq!(
        xtask::stamp(951_782_400),
        ("Feb 29 2000".into(), "00:00:00".into())
    );
    assert_eq!(
        xtask::stamp(4_107_542_400),
        ("Mar  1 2100".into(), "00:00:00".into())
    );
    assert_eq!(
        xtask::stamp(1_735_689_599),
        ("Dec 31 2024".into(), "23:59:59".into())
    );
}
