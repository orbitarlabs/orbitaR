use crate::{lever, tree};

fn model() -> String {
    let info = std::fs::read_to_string("/proc/cpuinfo").unwrap();
    let line = info.lines().find(|l| l.starts_with("model name")).unwrap();
    line.split_once(':').unwrap().1.trim().to_owned()
}

#[test]
fn node_writes_this_host_and_the_engine_build_and_keeps_the_other_tables() {
    let root = tree(
        "node-write",
        &[
            ("Cargo.toml", "[profile.release]\nlto = \"fat\"\n"),
            (
                "benchmarks/NODE.toml",
                "[host]\ncpu = \"old\"\n\n[rivals.qe]\nversion = \"7.4\"\n",
            ),
        ],
    );
    assert!(lever(&["node"], &root).is_empty());
    let text = std::fs::read_to_string(root.join("benchmarks/NODE.toml")).unwrap();
    let node: toml::Table = text.parse().unwrap();
    assert!(!text.contains("\n\n\n"), "{text}");
    let host = &node["host"];
    assert_eq!(host["cpu"].as_str(), Some(model().as_str()), "{text}");
    let logical = std::thread::available_parallelism().unwrap().get() as i64;
    assert_eq!(host["logical_cpus"].as_integer(), Some(logical), "{text}");
    let lscpu = std::process::Command::new("lscpu")
        .arg("-p=core,socket")
        .output()
        .unwrap();
    let pairs: std::collections::BTreeSet<&str> = std::str::from_utf8(&lscpu.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();
    assert_eq!(
        host["cores"].as_integer(),
        Some(pairs.len() as i64),
        "{text}"
    );
    let cores = host["core_cpus"].as_array().unwrap();
    assert_eq!(
        Some(cores.len() as i64),
        host["cores"].as_integer(),
        "{text}"
    );
    let info = std::fs::read_to_string("/proc/meminfo").unwrap();
    let total: i64 = (info.lines().find_map(|l| l.strip_prefix("MemTotal:")))
        .and_then(|v| v.trim().strip_suffix("kB")?.trim().parse().ok())
        .unwrap();
    let memory = host["memory_bytes"].as_integer().unwrap();
    assert!(64 << 20 < memory && memory <= total * 1024, "{text}");
    for key in ["cpus_allowed", "smt", "kernel", "microcode", "governor"] {
        assert!(host[key].is_str(), "{key}: {text}");
    }
    assert_eq!(
        node["orbitar"]["profile_release"]["lto"].as_str(),
        Some("fat")
    );
    assert!(
        node["orbitar"]["build"]
            .as_str()
            .unwrap()
            .starts_with("cargo build --release")
    );
    assert_eq!(
        node["rivals"]["qe"]["version"].as_str(),
        Some("7.4"),
        "{text}"
    );
}

#[test]
fn an_unreadable_node_file_is_a_finding_and_stays_as_it_was() {
    let root = tree("node-bad", &[("benchmarks/NODE.toml", "[host\n")]);
    let f = lever(&["node"], &root);
    assert!(
        f.iter().any(|f| f.starts_with("benchmarks/NODE.toml: ")),
        "{f:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("benchmarks/NODE.toml")).unwrap(),
        "[host\n"
    );
}

#[test]
fn a_node_path_that_cannot_be_read_is_a_finding_not_a_new_file() {
    let root = tree("node-dir", &[("benchmarks/NODE.toml/x", "")]);
    let f = lever(&["node"], &root);
    assert!(
        f.iter().any(|f| f.starts_with("benchmarks/NODE.toml: ")),
        "{f:?}"
    );
}

#[test]
fn a_node_file_that_is_not_text_is_a_finding_and_stays_as_it_was() {
    let root = tree("node-bytes", &[]);
    std::fs::create_dir_all(root.join("benchmarks")).unwrap();
    std::fs::write(root.join("benchmarks/NODE.toml"), b"cpu = \"\xff\"\n").unwrap();
    let f = lever(&["node"], &root);
    assert!(
        f.iter().any(|f| f.starts_with("benchmarks/NODE.toml: ")),
        "{f:?}"
    );
    assert_eq!(
        std::fs::read(root.join("benchmarks/NODE.toml")).unwrap(),
        b"cpu = \"\xff\"\n"
    );
}

#[test]
fn node_writes_each_rival_install_from_its_conda_meta() {
    let record = |name: &str, version: &str, build: &str| {
        format!(
            "{{\"name\": \"{name}\", \"version\": \"{version}\", \"build\": \"{build}\", \"sha256\": \"{name}-sha\"}}"
        )
    };
    let qe = record("qe", "7.5", "h19104ac_2");
    let blas = record("libblas", "3.9.0", "37_h4a7cf45_openblas");
    let other = record("zlib", "1.3.1", "hb9d3cd8_2");
    let abinit = record("abinit", "10.0.3", "h2b8cb88_2");
    let root = tree(
        "node-rivals",
        &[
            ("Cargo.toml", "[profile.release]\nlto = \"fat\"\n"),
            ("benchmarks/NODE.toml", ""),
            ("rivals/qe/conda-meta/qe-7.5-h19104ac_2.json", &qe),
            ("rivals/qe/conda-meta/libblas.json", &blas),
            ("rivals/qe/conda-meta/zlib.json", &other),
            ("rivals/abinit/conda-meta/abinit.json", &abinit),
        ],
    );
    let env = xtask::Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    assert!(xtask::run(&["node"], &root, &env).is_empty());
    let node: toml::Table = std::fs::read_to_string(root.join("benchmarks/NODE.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let qe = node["qe"].as_table().unwrap();
    assert_eq!(qe["qe"].as_str(), Some("7.5 h19104ac_2 sha256:qe-sha"));
    assert_eq!(
        qe["libblas"].as_str(),
        Some("3.9.0 37_h4a7cf45_openblas sha256:libblas-sha")
    );
    assert!(!qe.contains_key("zlib"), "{qe:?}");
    assert_eq!(
        node["abinit"]["abinit"].as_str(),
        Some("10.0.3 h2b8cb88_2 sha256:abinit-sha")
    );
}

#[test]
fn a_rival_row_that_node_toml_pins_must_be_installed_too() {
    let root = tree(
        "node-unpinned",
        &[
            ("Cargo.toml", "[profile.release]\nlto = \"fat\"\n"),
            ("benchmarks/NODE.toml", ""),
            ("rivals/.keep", ""),
        ],
    );
    let env = xtask::Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    assert!(xtask::run(&["node"], &root, &env).is_empty());
    let text = std::fs::read_to_string(root.join("benchmarks/NODE.toml")).unwrap();
    let text = text.replace("[qe]\n", "[qe]\nqe = \"7.5 h19104ac_2 sha256:x\"\n");
    std::fs::write(root.join("benchmarks/NODE.toml"), text).unwrap();
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-qm", "x"]);
    let f = xtask::run(&["bench", "claim"], &root, &env);
    assert!(
        crate::has(
            &f,
            "qe.qe is nothing here, \"7.5 h19104ac_2 sha256:x\" in NODE.toml"
        ),
        "{f:?}"
    );
}

fn claim_with_memory_shifted(name: &str, shift: i64, keys: &[&str]) -> (Vec<String>, i64) {
    let root = tree(
        name,
        &[
            ("Cargo.toml", "[profile.release]\nlto = \"fat\"\n"),
            ("benchmarks/NODE.toml", ""),
        ],
    );
    assert!(lever(&["node"], &root).is_empty());
    let path = root.join("benchmarks/NODE.toml");
    let mut node: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let host = node["host"].as_table_mut().unwrap();
    let memory = host["memory_bytes"].as_integer().unwrap();
    host.insert("memory_bytes".into(), (memory + shift).into());
    for key in keys {
        host.insert((*key).into(), "other".into());
    }
    std::fs::write(&path, toml::to_string(&node).unwrap()).unwrap();
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-qm", "x"]);
    (lever(&["bench", "claim"], &root), memory)
}

fn not_the_node(f: &[String]) -> bool {
    crate::has(f, "this host is not the declared node")
}

#[test]
fn a_claim_accepts_memory_that_moved_by_a_page_since_the_node_was_declared() {
    let f = claim_with_memory_shifted("node-page", 16_762_847_232 - 16_762_851_328, &[]).0;
    assert!(!not_the_node(&f), "{f:?}");
}

#[test]
fn a_claim_accepts_memory_up_to_64_mib_apart_and_refuses_one_byte_more() {
    let f = claim_with_memory_shifted("node-edge", 64 << 20, &[]).0;
    assert!(!not_the_node(&f), "{f:?}");
    let f = claim_with_memory_shifted("node-past", -((64 << 20) + 1), &[]).0;
    assert!(
        crate::has(&f, "(67108865 B apart, more than 67108864 B)"),
        "{f:?}"
    );
}

#[test]
fn a_claim_refuses_memory_a_gib_apart_and_names_the_key_both_values_and_the_gap() {
    let (f, host) = claim_with_memory_shifted("node-gib", -(1 << 30), &[]);
    assert!(
        crate::has(
            &f,
            &format!(
                "host.memory_bytes is {host} here, {} in NODE.toml (1073741824 B apart, more than 67108864 B)",
                host - (1 << 30)
            )
        ),
        "{f:?}"
    );
}

#[test]
fn a_claim_still_refuses_any_other_host_key_that_differs() {
    let f = claim_with_memory_shifted("node-cpu", 4096, &["cpu"]).0;
    assert!(crate::has(&f, "\"other\" in NODE.toml"), "{f:?}");
    assert!(!crate::has(&f, "memory_bytes"), "{f:?}");
}
