use super::{
    rival::{ABINIT, INPUT, PW, TUNED, rivals_tree, script},
    speed::{both, fake, passed},
};
use crate::{Tree, has, write};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use xtask::Env;
use xtask::harness::{Outcome, Session, Tier};

const SLOW_PW: &str = "t=0.5\n";

fn declared(root: &Tree) -> PathBuf {
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    assert!(xtask::run(&["node"], root, &env).is_empty());
    let earlier = root.join("earlier");
    std::fs::create_dir_all(&earlier).unwrap();
    earlier
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

fn kept(root: &Tree, earlier: &Path, file: &str) -> String {
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    std::fs::write(earlier.join(file), &text).unwrap();
    text
}

fn rivals(record: &Value) -> Vec<&Value> {
    (record["speed"].as_array().unwrap().iter())
        .filter(|b| b["code"] != "orbitar")
        .collect()
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

#[test]
fn a_rival_block_whose_key_matches_an_earlier_record_is_reused_with_its_source_and_a_drift_check() {
    let root = rivals_tree(
        "reuse-match",
        INPUT,
        TUNED,
        &format!("{SLOW_PW}{PW}"),
        ABINIT,
    );
    let earlier = declared(&root);
    let (outcome, first) = claim(&root, None);
    assert!(passed(&outcome), "{outcome:?}: {first}");
    assert!(first["rival_reuse"].is_null(), "{first}");
    for b in rivals(&first) {
        assert!(b["reuse_key"]["input_sha256"].is_string(), "{b}");
        assert!(b["reused"].is_null(), "{b}");
    }
    let source = kept(&root, &earlier, "1_a.json");
    let (outcome, second) = claim(&root, Some(&earlier));
    assert!(passed(&outcome), "{outcome:?}: {second}");
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 2, "{reuse}");
    assert_eq!(reuse["drift"]["held"], true, "{reuse}");
    assert_eq!(reuse["drift"]["code"], "qe", "{reuse}");
    assert!(reuse["drift"]["t_delta_s"].is_f64(), "{reuse}");
    for b in rivals(&second) {
        assert_eq!(
            b["reused"],
            json!({
                "commit": first["commit"],
                "record_sha256": sha256(&source),
                "started_unix_ms": first["started_unix_ms"],
            }),
            "{b}"
        );
        let same = (rivals(&first).into_iter())
            .find(|f| f["code"] == b["code"])
            .unwrap();
        assert_eq!(samples(b), samples(same));
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
    let root = rivals_tree("reuse-key-field", INPUT, TUNED, PW, ABINIT);
    let earlier = declared(&root);
    let (_, first) = claim(&root, None);
    let fields: Vec<String> = (rivals(&first)[0]["reuse_key"].as_object().unwrap().keys())
        .cloned()
        .collect();
    assert_eq!(fields.len(), 17, "{fields:?}");
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
    assert!(reuse["drift"].is_null(), "{reuse}");
    let rerun = reuse["rerun"].as_array().unwrap();
    assert_eq!(rerun.len(), 2, "{reuse}");
    for entry in rerun {
        let why: Vec<&str> = (entry["why"].as_array().unwrap().iter())
            .map(|w| w.as_str().unwrap())
            .collect();
        for field in &fields {
            assert!(
                why.iter()
                    .any(|w| w.ends_with(&format!("its key differs in {field}"))),
                "{field}: {why:?}"
            );
        }
    }
    for b in rivals(&second) {
        assert!(b["reused"].is_null(), "{b}");
    }
}

#[test]
fn a_rival_cutoff_changed_since_the_earlier_record_runs_the_rival_again() {
    let root = rivals_tree("reuse-cutoff", INPUT, TUNED, PW, ABINIT);
    let earlier = declared(&root);
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    let tuned = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    let tuned = tuned.replace("Si = 10.0", "Si = 12.0");
    write(&root, "benchmarks/TUNED.toml", &tuned);
    let (_, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 0, "{reuse}");
    for entry in reuse["rerun"].as_array().unwrap() {
        assert!(
            entry["why"][0]
                .as_str()
                .unwrap()
                .ends_with("its key differs in cutoff_ha, input_sha256"),
            "{entry}"
        );
    }
}

#[test]
fn a_rival_slower_than_its_stored_spread_refuses_every_reuse_and_runs_each_rival_again() {
    let root = rivals_tree("reuse-drift", INPUT, TUNED, PW, ABINIT);
    let earlier = declared(&root);
    claim(&root, None);
    kept(&root, &earlier, "1_a.json");
    script(&root.join("rivals/qe/bin/pw.x"), &format!("t=0.6\n{PW}"));
    let (outcome, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["drift"]["held"], false, "{reuse}");
    assert_eq!(reuse["drift"]["code"], "qe", "{reuse}");
    assert!(
        reuse["drift"]["t_delta_s"].as_f64().unwrap() > 0.5,
        "{reuse}"
    );
    assert!(
        reuse["refused"]
            .as_str()
            .unwrap()
            .ends_with("every rival runs again"),
        "{reuse}"
    );
    assert_eq!(reuse["reused"], 0, "{reuse}");
    for b in rivals(&second) {
        assert!(b["reused"].is_null(), "{b}");
    }
    let qe = rivals(&second)[0];
    assert!(
        qe["median_t_delta_s"].as_f64().unwrap() > 0.5,
        "{outcome:?}: {qe}"
    );
}

#[test]
fn a_record_from_another_node_or_schema_is_no_source() {
    let root = rivals_tree("reuse-unfit", INPUT, TUNED, PW, ABINIT);
    let earlier = declared(&root);
    let (_, first) = claim(&root, None);
    let mut schema = first.clone();
    schema["schema"] = json!(3);
    std::fs::write(earlier.join("schema.json"), schema.to_string()).unwrap();
    let mut node = first.clone();
    node["node"]["host"]["cpu"] = json!("another CPU");
    std::fs::write(earlier.join("node.json"), node.to_string()).unwrap();
    let (_, second) = claim(&root, Some(&earlier));
    let reuse = &second["rival_reuse"];
    assert_eq!(reuse["reused"], 0, "{reuse}");
    let unfit: Vec<String> = (reuse["unfit"].as_array().unwrap().iter())
        .map(|u| u.as_str().unwrap().to_owned())
        .collect();
    assert!(has(&unfit, "schema 3, not 4"), "{unfit:?}");
    assert!(
        has(&unfit, "its node is not NODE.toml's: host.cpu"),
        "{unfit:?}"
    );
}

#[test]
fn a_host_that_is_not_the_declared_node_reuses_nothing() {
    let root = rivals_tree("reuse-host", INPUT, TUNED, PW, ABINIT);
    let earlier = declared(&root);
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
fn the_verifier_refuses_a_reused_block_whose_source_record_is_missing() {
    let root = rivals_tree(
        "reuse-missing",
        INPUT,
        TUNED,
        &format!("{SLOW_PW}{PW}"),
        ABINIT,
    );
    let earlier = declared(&root);
    claim(&root, None);
    let source = kept(&root, &earlier, "1_a.json");
    claim(&root, Some(&earlier));
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
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
    let missing = format!("its source record {} is not available", sha256(&source));
    let found = verify(&empty);
    assert!(has(&found, &missing), "{found:?}");
    let found = verify(&earlier);
    assert!(!has(&found, "its reused block"), "{found:?}");
    assert!(!has(&found, "drift check"), "{found:?}");
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
