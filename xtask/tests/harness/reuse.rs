use super::{
    rival::{ABINIT, INPUT, PW, TUNED, rivals_tree, script},
    speed::{both, fake, passed},
};
use crate::{Tree, git, has, write};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use xtask::Env;
use xtask::harness::{Outcome, Session, Tier};

const QUIET: &str = r#"{"quiet": {"load_avg": [8.0, 8.0, 8.0], "others_runnable": 0, "psi_some_avg10": 20.0, "psi_some_share": 0.0}}"#;
const ALONE: &str = r#"{"quiet": {"wall_s": 0.2, "busy_cpu_s": 1.6, "child_cpu_s": 1.5, "harness_cpu_s": 0.05, "others_cpu_s": 0.05, "share": 0.03125}}"#;

fn pace(root: &Tree, code: &str, seconds: f64) {
    write(root, &format!("pace/{code}"), &seconds.to_string());
}

fn reusable(name: &str) -> (Tree, PathBuf) {
    let root = rivals_tree(name, INPUT, TUNED, PW, ABINIT);
    for (code, bin) in [
        ("qe", "rivals/qe/bin/pw.x"),
        ("abinit", "rivals/abinit/bin/abinit"),
    ] {
        let body = std::fs::read_to_string(root.join(bin)).unwrap();
        let body = body.trim_start_matches("#!/bin/sh\n");
        let paced = root.join(format!("pace/{code}"));
        script(
            &root.join(bin),
            &format!("t=$(cat '{}')\n{body}", paced.display()),
        );
        pace(&root, code, 0.5);
    }
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    assert!(xtask::run(&["node"], &root, &env).is_empty());
    write(
        &root,
        ".gitignore",
        "records/\nearlier/\npace/\nfake-engine\n",
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "tree"]);
    let earlier = root.join("earlier");
    std::fs::create_dir_all(&earlier).unwrap();
    (root, earlier)
}

fn claim(root: &Tree, earlier: Option<&Path>) -> (Outcome, Value) {
    let bin = fake(root, "step -2");
    let mut session = Session::new(root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    if let Some(dir) = earlier {
        session = session.with_rival_records(dir);
    }
    let outcome = session.evaluate("1_a", both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

fn quiet(mut record: Value) -> Value {
    let (quiet, alone): (Value, Value) = (
        serde_json::from_str(QUIET).unwrap(),
        serde_json::from_str(ALONE).unwrap(),
    );
    for b in record["speed"].as_array_mut().unwrap() {
        let runs =
            1 + b["t_delta_s"].as_array().unwrap().len() + b["losses"].as_array().unwrap().len();
        b["load"] = json!(vec![quiet.clone(); runs]);
        b["during"] = json!(vec![alone.clone(); runs]);
    }
    record
}

fn kept(root: &Tree, earlier: &Path, file: &str) -> (Value, String) {
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    let record = quiet(serde_json::from_str(&text).unwrap());
    let text = record.to_string();
    std::fs::write(earlier.join(file), &text).unwrap();
    (record, text)
}

fn rivals(record: &Value) -> Vec<&Value> {
    (record["speed"].as_array().unwrap().iter())
        .filter(|b| b["code"] != "orbitar")
        .collect()
}

fn of<'a>(record: &'a Value, code: &str) -> &'a Value {
    rivals(record)
        .into_iter()
        .find(|b| b["code"] == code)
        .unwrap()
}

fn sha256(text: &str) -> String {
    super::hex(&<sha2::Sha256 as sha2::Digest>::digest(text))
}

fn samples(block: &Value) -> Value {
    let mut block = block.clone();
    let fields = block.as_object_mut().unwrap();
    for key in ["accuracy", "reused"] {
        fields.remove(key);
    }
    block
}

fn why_of<'a>(record: &'a Value, code: &str) -> Vec<&'a str> {
    (record["rival_reuse"]["rerun"].as_array().unwrap().iter())
        .filter(|r| r["code"] == code)
        .flat_map(|r| r["why"].as_array().unwrap().iter())
        .map(|w| w.as_str().unwrap())
        .collect()
}

#[test]
fn a_rival_block_whose_key_matches_an_earlier_record_is_reused_with_its_source_and_a_drift_check() {
    let (root, earlier) = reusable("reuse-match");
    let (outcome, first) = claim(&root, None);
    assert!(passed(&outcome), "{outcome:?}: {first}");
    assert!(first["rival_reuse"].is_null(), "{first}");
    for b in rivals(&first) {
        for field in [
            "decks",
            "runtime",
            "protocol",
            "node",
            "declared_host",
            "identity",
        ] {
            assert!(!b["reuse_key"][field].is_null(), "{field}: {b}");
        }
        assert!(b["reused"].is_null(), "{b}");
    }
    let (source, text) = kept(&root, &earlier, "1_a.json");
    let (outcome, second) = claim(&root, Some(&earlier));
    assert!(passed(&outcome), "{outcome:?}: {second}");
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 2, "{reuse}");
    assert_eq!(reuse["mode"], "claim", "{reuse}");
    assert!(reuse.get("records").is_none(), "{reuse}");
    let drift = reuse["drift"].as_array().unwrap();
    let codes: Vec<&Value> = drift.iter().map(|d| &d["code"]).collect();
    assert_eq!(codes, [&json!("qe"), &json!("abinit")], "{reuse}");
    for d in drift {
        assert_eq!(d["held"], true, "{d}");
        assert_eq!(d["runs"].as_array().unwrap().len(), 3, "{d}");
    }
    for b in rivals(&second) {
        assert_eq!(
            b["reused"],
            json!({
                "commit": first["commit"],
                "record_sha256": sha256(&text),
                "started_unix_ms": first["started_unix_ms"],
            }),
            "{b}"
        );
        assert_eq!(
            samples(b),
            samples(of(&source, b["code"].as_str().unwrap()))
        );
    }
    let ours = |r: &Value| r["speed"].as_array().unwrap()[2]["t_delta_s"].clone();
    assert_ne!(ours(&first), ours(&second));
}

fn altered(value: &Value) -> Value {
    match value {
        Value::Number(n) if n.is_u64() => json!(n.as_u64().unwrap() + 1),
        Value::Number(n) => json!(n.as_f64().unwrap() * 2.0 + 1.0),
        Value::String(_) => json!("other"),
        Value::Bool(b) => json!(!b),
        Value::Array(items) => {
            let mut items = items.clone();
            items.push(json!("other"));
            Value::Array(items)
        }
        Value::Object(map) => {
            let mut map = map.clone();
            match map.iter().next().map(|(k, v)| (k.clone(), altered(v))) {
                Some((k, v)) => map.insert(k, v),
                None => map.insert("other".into(), json!("other")),
            };
            Value::Object(map)
        }
        Value::Null => json!(1e-9),
    }
}

#[test]
fn a_difference_in_any_one_key_field_runs_the_rival_again() {
    let (root, earlier) = reusable("reuse-key-field");
    claim(&root, None);
    let (first, _) = kept(&root, &earlier, "1_a.json");
    std::fs::remove_file(earlier.join("1_a.json")).unwrap();
    let fields: Vec<String> = (rivals(&first)[0]["reuse_key"].as_object().unwrap().keys())
        .cloned()
        .collect();
    assert_eq!(fields.len(), 23, "{fields:?}");
    for field in &fields {
        let mut copy = first.clone();
        for b in copy["speed"].as_array_mut().unwrap() {
            if b["code"] != "orbitar" {
                b["reuse_key"][field] = altered(&b["reuse_key"][field]);
            }
        }
        std::fs::write(earlier.join(format!("{field}.json")), copy.to_string()).unwrap();
    }
    let (outcome, second) = claim(&root, Some(&earlier));
    assert!(passed(&outcome), "{outcome:?}: {second}");
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 0, "{reuse}");
    assert_eq!(reuse["drift"], json!([]), "{reuse}");
    assert!(reuse["unfit"].as_array().unwrap().is_empty(), "{reuse}");
    for code in ["qe", "abinit"] {
        let why = why_of(&second, code);
        for field in &fields {
            assert!(
                why.iter()
                    .any(|w| w.ends_with(&format!("its key differs in {field}"))),
                "{code} {field}: {why:?}"
            );
        }
    }
}

#[test]
fn a_replaced_rival_executable_runs_that_rival_again_and_reuses_the_other() {
    let (root, earlier) = reusable("reuse-executable");
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    let bin = root.join("rivals/abinit/bin/abinit");
    let body = std::fs::read_to_string(&bin).unwrap();
    script(
        &bin,
        &format!(
            "{}\n: another build",
            body.trim_start_matches("#!/bin/sh\n")
        ),
    );
    let (_, second) = claim(&root, Some(&earlier));
    assert!(of(&second, "abinit")["reused"].is_null(), "{second}");
    assert!(!of(&second, "qe")["reused"].is_null(), "{second}");
    let why = why_of(&second, "abinit");
    assert!(
        why.iter()
            .any(|w| w.ends_with("its key differs in runtime")),
        "{why:?}"
    );
}

#[test]
fn a_changed_rival_setting_in_the_protocol_files_runs_every_rival_again() {
    let (root, earlier) = reusable("reuse-protocol");
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    let settings = std::fs::read_to_string(root.join("benchmarks/RIVALS.toml")).unwrap();
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &settings.replace("budget_s = 1000.0", "budget_s = 999.0"),
    );
    let (_, second) = claim(&root, Some(&earlier));
    assert_eq!(second["rival_reuse"]["reused"], 0, "{second}");
    for code in ["qe", "abinit"] {
        let why = why_of(&second, code);
        assert!(
            why.iter()
                .any(|w| w.ends_with("its key differs in protocol")),
            "{why:?}"
        );
    }
}

#[test]
fn a_rival_cutoff_changed_since_the_earlier_record_runs_the_rival_again() {
    let (root, earlier) = reusable("reuse-cutoff");
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    let tuned = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    write(
        &root,
        "benchmarks/TUNED.toml",
        &tuned.replace("Si = 10.0", "Si = 12.0"),
    );
    let (_, second) = claim(&root, Some(&earlier));
    assert_eq!(second["rival_reuse"]["reused"], 0, "{second}");
    for code in ["qe", "abinit"] {
        let why = why_of(&second, code);
        let differs = why.iter().find(|w| w.contains("its key differs in"));
        assert!(
            differs.is_some_and(|w| w.contains("cutoff_ha") && w.contains("decks")),
            "{why:?}"
        );
    }
}

#[test]
fn a_rival_slower_than_its_drift_budget_refuses_its_own_reuse_and_keeps_the_others() {
    let (root, earlier) = reusable("reuse-drift");
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    pace(&root, "abinit", 0.8);
    let (_, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    let drift = reuse["drift"].as_array().unwrap();
    let abinit = drift.iter().find(|d| d["code"] == "abinit").unwrap();
    assert_eq!(abinit["held"], false, "{reuse}");
    assert!(
        abinit["why"].as_str().unwrap().contains("median T(δ)"),
        "{abinit}"
    );
    assert!(reuse["refused_codes"]["abinit"].is_string(), "{reuse}");
    let fresh = of(&second, "abinit");
    assert!(fresh["reused"].is_null(), "{second}");
    assert!(
        fresh["median_t_delta_s"].as_f64().unwrap() > 0.7,
        "{second}"
    );
    assert!(!of(&second, "qe")["reused"].is_null(), "{second}");
    assert_eq!(reuse["reused"], 1, "{reuse}");
}

#[test]
fn a_dirty_later_pilot_or_foreign_record_is_no_source() {
    let (root, earlier) = reusable("reuse-unfit");
    claim(&root, None);
    let (first, _) = kept(&root, &earlier, "1_a.json");
    std::fs::remove_file(earlier.join("1_a.json")).unwrap();
    let edits: [(&str, &str, Value); 6] = [
        ("schema", "its schema is 3", json!(3)),
        ("dirty", "its dirty is true", json!(true)),
        ("pilot", "its pilot is true", json!(true)),
        ("commit", "it names no full commit hash", json!("abc")),
        (
            "started_unix_ms",
            "not before the reusing run's",
            json!(u64::MAX / 2),
        ),
        ("node", "its node is not NODE.toml's: host.cpu", Value::Null),
    ];
    for (field, _, value) in &edits {
        let mut copy = first.clone();
        match *field {
            "node" => copy["node"]["host"]["cpu"] = json!("another CPU"),
            _ => copy[*field] = value.clone(),
        }
        std::fs::write(earlier.join(format!("{field}.json")), copy.to_string()).unwrap();
    }
    let mut stranger = first.clone();
    stranger["commit"] = json!("0".repeat(40));
    std::fs::write(earlier.join("stranger.json"), stranger.to_string()).unwrap();
    let (_, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 0, "{reuse}");
    let unfit: Vec<String> = (reuse["unfit"].as_array().unwrap().iter())
        .map(|u| u.as_str().unwrap().to_owned())
        .collect();
    for (field, why, _) in &edits {
        assert!(has(&unfit, why), "{field}: {unfit:?}");
    }
    assert!(
        has(&unfit, "is no ancestor of the reusing run's commit"),
        "{unfit:?}"
    );
}

#[test]
fn a_host_that_is_not_the_declared_node_reuses_nothing() {
    let (root, earlier) = reusable("reuse-host");
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    let text = std::fs::read_to_string(root.join("benchmarks/NODE.toml")).unwrap();
    let mut node: toml::Table = text.parse().unwrap();
    node["host"]["cpu"] = "elsewhere".into();
    write(
        &root,
        "benchmarks/NODE.toml",
        &toml::to_string(&node).unwrap(),
    );
    let (_, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 0, "{reuse}");
    assert!(
        reuse["refused"]
            .as_str()
            .unwrap()
            .starts_with("this host is not benchmarks/NODE.toml's node: host.cpu"),
        "{reuse}"
    );
}

#[test]
fn the_verifier_reads_sources_only_for_a_reused_block_and_names_a_missing_one() {
    let (root, earlier) = reusable("reuse-missing");
    claim(&root, None);
    let (_, text) = kept(&root, &earlier, "1_a.json");
    let verify = |dir: &Path| {
        xtask::harness::verify(
            &root,
            Tier::Claim,
            &root.join("records"),
            std::time::UNIX_EPOCH,
            Some("1_a"),
            Some(dir),
        )
    };
    let nowhere = root.join("nowhere");
    let fresh = verify(&nowhere);
    assert!(!has(&fresh, "ORBITAR_RIVAL_RECORDS"), "{fresh:?}");
    claim(&root, Some(&earlier));
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let missing = format!("its source record {} is not available", sha256(&text));
    for dir in [&empty, &nowhere] {
        let found = verify(dir);
        assert!(has(&found, &missing), "{found:?}");
    }
    assert!(!has(&verify(&earlier), "is not available"));
}

#[test]
fn rival_records_reuse_only_in_the_claim_tier() {
    let root = rivals_tree("reuse-tier", INPUT, TUNED, PW, ABINIT);
    let refused = Session::from_vars(&root, |key: &str| match key {
        "ORBITAR_BIN" => Some("/bin/true".into()),
        "ORBITAR_TIER" => Some("node".into()),
        "ORBITAR_RIVAL_RECORDS" => Some("/elsewhere".into()),
        _ => None,
    })
    .err()
    .unwrap();
    assert!(
        refused.starts_with("ORBITAR_RIVAL_RECORDS reuses rival blocks in the claim tier alone"),
        "{refused}"
    );
}

#[test]
fn the_measuring_protocol_names_only_files_of_this_checkout() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for file in xtask::harness::REUSE_PROTOCOL {
        assert!(root.join(file).is_file(), "{file}");
    }
}
