use super::speed::{ONE_SI, claim_tree, evaluated, passed, procfs};
use super::{CASE, INPUT};
use crate::Tree;
use serde_json::{Value, json};
use xtask::harness::{Outcome, Session, Tier};

fn one_si_tree(name: &str) -> Tree {
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let case = CASE.replace("\"push\"", "\"claim\"");
    claim_tree(name, &case, &[("input.toml", &input)], ONE_SI, "step -2")
}

fn claim_at_load(name: &str, runnable: Option<u32>, pilot: bool) -> (Outcome, Value) {
    use std::ffi::OsString;
    let root = one_si_tree(name);
    let proc = match runnable {
        Some(n) => procfs(&root, n),
        None => {
            procfs(&root, 1);
            root.join("no-proc")
        }
    };
    let bin: OsString = root.join("fake-engine").into();
    let pairs: Vec<(&str, OsString)> = vec![
        ("ORBITAR_BIN", bin),
        ("ORBITAR_TIER", "claim".into()),
        ("ORBITAR_RECORD_DIR", root.join("records").into()),
    ]
    .into_iter()
    .chain(pilot.then(|| ("ORBITAR_PILOT", "1".into())))
    .collect();
    let vars = move |key: &str| {
        (pairs.iter())
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    };
    let session = Session::from_vars(&root, vars)
        .unwrap()
        .unwrap()
        .with_load(&proc);
    evaluated(&root, &session)
}

fn readings(record: &Value) -> Vec<Vec<&str>> {
    (record["speed"].as_array().into_iter().flatten())
        .map(|b| {
            (b["load"].as_array().into_iter().flatten())
                .filter_map(|r| {
                    r.as_object()
                        .and_then(|o| o.keys().next())
                        .map(String::as_str)
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_node_whose_only_runnable_task_is_the_reader_is_quiet_whatever_load_averages_the_last_run_left()
{
    let (outcome, record) = claim_at_load("load-quiet", Some(1), false);
    assert!(passed(&outcome), "{outcome:?}");
    let runs = record["speed"][0]["wall_s"].as_array().unwrap().len();
    assert_eq!(readings(&record), [vec!["quiet"; 1 + runs]], "{record}");
    let sample = &record["speed"][0]["load"][0]["quiet"];
    assert_eq!(sample["load_avg"], json!([8.0, 8.0, 8.0]), "{sample}");
    assert_eq!(sample["psi_some_avg10"], json!(20.0), "{sample}");
    assert_eq!(sample["psi_some_share"], json!(0.0), "{sample}");
}

#[test]
fn one_other_runnable_task_holds_an_eighth_of_the_nodes_8_cpus_past_the_5_percent_cv_and_stops_a_claim()
 {
    for (runnable, held) in [
        (2, "1 other runnable tasks hold 0.125"),
        (9, "8 other runnable tasks hold 1"),
    ] {
        let (outcome, record) =
            claim_at_load(&format!("load-busy-{runnable}"), Some(runnable), false);
        let why = format!(
            "the node is not quiet before a timed run: {held} of the node's 8 CPUs, not under 0.05"
        );
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(&why) && f.contains("harness")),
            "{outcome:?}"
        );
        assert!(
            record["speed"].as_array().is_none_or(Vec::is_empty),
            "{record}"
        );
    }
}

#[test]
fn a_pilot_records_a_loaded_or_unread_node_before_each_run_and_runs_on_so_its_readout_shows_it() {
    for (n, (runnable, kind)) in [(Some(2), "loaded"), (None, "unread")]
        .into_iter()
        .enumerate()
    {
        let (outcome, record) = claim_at_load(&format!("load-pilot-{n}"), runnable, true);
        assert_eq!(readings(&record), [vec![kind; 2]], "{record}");
        let named = "input at 8 threads: 2 of its 2 runs, the warm-up first, began on a node that was not quiet";
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(named)),
            "{outcome:?}"
        );
    }
}

#[test]
fn a_claim_stops_on_a_node_that_shows_no_load_or_declares_no_cpus() {
    let (outcome, _) = claim_at_load("load-unread", None, false);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet before a timed run: ") && f.contains("no-proc/loadavg")),
        "{outcome:?}"
    );
    let root = one_si_tree("load-no-node");
    let proc = procfs(&root, 1);
    std::fs::remove_file(root.join("benchmarks/NODE.toml")).unwrap();
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet before a timed run: claim: benchmarks/NODE.toml is missing")),
        "{outcome:?}"
    );
}

fn claim_with_stalls(name: &str, tick_us: u64) -> Outcome {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = one_si_tree(name);
    let proc = procfs(&root, 1);
    let done = Arc::new(AtomicBool::new(false));
    let stalls = {
        let (done, proc) = (done.clone(), proc.clone());
        std::thread::spawn(move || {
            let mut total: u64 = 1000;
            while !done.load(Ordering::Relaxed) {
                total += tick_us;
                let next = proc.join("pressure/cpu.next");
                let line = format!("some avg10=0.00 avg60=0.00 avg300=0.00 total={total}\n");
                std::fs::write(&next, line).unwrap();
                std::fs::rename(&next, proc.join("pressure/cpu")).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })
    };
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    done.store(true, Ordering::Relaxed);
    stalls.join().unwrap();
    outcome
}

#[test]
fn a_task_that_waits_for_a_cpu_through_the_window_stops_a_claim_and_one_that_waits_a_hundredth_of_it_does_not()
 {
    let waited =
        |o: &Outcome| matches!(o, Outcome::Fail(f) if f.contains("a task waited for a CPU "));
    let outcome = claim_with_stalls("load-stall", 10_000);
    assert!(
        waited(&outcome)
            && matches!(&outcome, Outcome::Fail(f) if f.contains(" of the window, not under 0.05")),
        "{outcome:?}"
    );
    let outcome = claim_with_stalls("load-stall-brief", 100);
    assert!(!waited(&outcome) && passed(&outcome), "{outcome:?}");
}
