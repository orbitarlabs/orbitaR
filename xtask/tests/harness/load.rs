use super::speed::{ONE_SI, claim_tree, evaluated, fake, passed, procfs};
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

fn readings<'a>(record: &'a Value, key: &str) -> Vec<Vec<&'a str>> {
    (record["speed"].as_array().into_iter().flatten())
        .map(|b| {
            (b[key].as_array().into_iter().flatten())
                .filter_map(|r| {
                    r.as_str().or_else(|| {
                        r.as_object()
                            .and_then(|o| o.keys().next())
                            .map(String::as_str)
                    })
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
    assert_eq!(
        readings(&record, "load"),
        [vec!["quiet"; 1 + runs]],
        "{record}"
    );
    assert_eq!(
        readings(&record, "during"),
        [vec!["quiet"; 1 + runs]],
        "{record}"
    );
    let through = &record["speed"][0]["during"][0]["quiet"];
    assert_eq!(through["busy_cpu_s"], json!(0.0), "{through}");
    assert_eq!(through["others_cpu_s"], json!(0.0), "{through}");
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
        let why = format!("{held} of the node's 8 CPUs, not under 0.05");
        let refused = format!("the node is not quiet for a timed run: {why}");
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(&refused) && f.contains("harness")),
            "{outcome:?}"
        );
        assert!(
            record["speed"].as_array().is_none_or(Vec::is_empty),
            "{record}"
        );
        let kept = record["load"].as_array().unwrap();
        assert_eq!(kept.len(), 3, "{record}");
        assert!(
            (kept.iter()).all(|r| r["at"] == "input at 8 threads"
                && r["before"]["loaded"]["why"]
                    .as_str()
                    .is_some_and(|w| w.contains(&why))),
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
        assert_eq!(readings(&record, "load"), [vec![kind; 2]], "{record}");
        let named = "input at 8 threads: of its 2 runs, the warm-up first, 2 began on a node that was not quiet";
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
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet for a timed run: ") && f.contains("no-proc/loadavg")),
        "{outcome:?}"
    );
    let root = one_si_tree("load-no-node");
    let proc = procfs(&root, 1);
    std::fs::remove_file(root.join("benchmarks/NODE.toml")).unwrap();
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet for a timed run: claim: benchmarks/NODE.toml is missing")),
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

fn mkfifo(path: &std::path::Path) {
    let made = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo {}", path.display());
}

fn served<F: Fn(usize) -> u32 + Send + 'static>(
    fifo: &std::path::Path,
    reads: F,
) -> impl FnOnce() + use<F> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let done = Arc::new(AtomicBool::new(false));
    let rotate = |fifo: &std::path::Path, k: usize| {
        let fresh = fifo.with_extension(format!("{k}"));
        let _ = std::fs::remove_file(&fresh);
        mkfifo(&fresh);
        std::fs::rename(&fresh, fifo).unwrap();
    };
    rotate(fifo, 0);
    let server = {
        let (done, fifo) = (done.clone(), fifo.to_owned());
        std::thread::spawn(move || {
            for k in 0.. {
                let line = format!("8.00 8.00 8.00 {}/300 4242\n", reads(k));
                let opened = std::fs::OpenOptions::new().write(true).open(&fifo);
                if done.load(Ordering::Relaxed) {
                    break;
                }
                rotate(&fifo, k + 1);
                let _ = std::io::Write::write_all(&mut opened.unwrap(), line.as_bytes());
            }
        })
    };
    let fifo = fifo.to_owned();
    move || {
        done.store(true, Ordering::Relaxed);
        let _ = std::fs::read_to_string(&fifo);
        server.join().unwrap();
    }
}

#[test]
fn a_task_on_the_last_read_of_a_window_loads_that_window_and_the_run_waits_for_a_quiet_one() {
    let root = one_si_tree("load-last-read");
    let proc = procfs(&root, 1);
    let stop = served(&proc.join("loadavg"), |k| if k == 4 { 2 } else { 1 });
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, record) = evaluated(&root, &session);
    stop();
    assert!(passed(&outcome), "{outcome:?}");
    let log = record["load"].as_array().unwrap();
    let first = &log[0]["before"]["loaded"]["sample"];
    assert_eq!(first["others_runnable"], json!(1), "{record}");
    assert!(log[1]["before"]["quiet"].is_object(), "{record}");
    assert_eq!(readings(&record, "load")[0][0], "quiet", "{record}");
}

#[test]
fn a_task_that_takes_cpus_after_the_child_starts_stops_a_claim_and_a_pilot_records_it() {
    for pilot in [false, true] {
        let root = one_si_tree(&format!("load-after-start-{pilot}"));
        let proc = procfs(&root, 1);
        let stat = proc.join("stat");
        let busy = format!(
            "n=$(awk '/^cpu /{{print $2}}' '{0}'); echo \"cpu  $((n + 10000)) 0 1000 100000 0 0 0 0 0 0\" > '{0}.next'; mv '{0}.next' '{0}'\nstep -2",
            stat.display()
        );
        let bin = fake(&root, &busy);
        let mut session = Session::new(&root, &bin, Tier::Claim, &root.join("records"));
        if pilot {
            use std::ffi::OsString;
            let bin: OsString = bin.into();
            let records: OsString = root.join("records").into();
            let vars = move |key: &str| match key {
                "ORBITAR_BIN" => Some(bin.clone()),
                "ORBITAR_TIER" => Some("claim".into()),
                "ORBITAR_PILOT" => Some("1".into()),
                "ORBITAR_RECORD_DIR" => Some(records.clone()),
                _ => None,
            };
            session = Session::from_vars(&root, vars).unwrap().unwrap();
        }
        let (outcome, record) = evaluated(&root, &session.with_load(&proc));
        let why = " CPU s over the ";
        match pilot {
            false => {
                assert!(
                    matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet for a timed run: ") && f.contains(why)),
                    "{outcome:?}"
                );
                let kept = record["load"].as_array().unwrap();
                assert!(
                    kept.last().unwrap()["during"]["loaded"]["why"]
                        .as_str()
                        .is_some_and(|w| w.contains(why)),
                    "{record}"
                );
            }
            true => {
                assert_eq!(readings(&record, "during"), [vec!["loaded"; 2]], "{record}");
                let named = "input at 8 threads: of its 2 runs, the warm-up first, 0 began on a node that was not quiet and 2 shared it with other tasks";
                assert!(
                    matches!(&outcome, Outcome::Fail(f) if f.contains(named)),
                    "{outcome:?}"
                );
            }
        }
    }
}

fn rival_lost_on_its_warm_up(name: &str, busy: bool) -> (Outcome, Value) {
    use super::rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let count = std::env::temp_dir().join(format!("xtask-{name}-calls-{}", std::process::id()));
    let _ = std::fs::remove_file(&count);
    let root = rivals_tree(name, INPUT, TUNED, "exit 7", ABINIT);
    let proc = procfs(&root, 1);
    let stat = proc.join("stat");
    let bump = match busy {
        true => format!(
            "n=$(awk '/^cpu /{{print $2}}' '{0}'); echo \"cpu  $((n + 10000)) 0 1000 100000 0 0 0 0 0 0\" > '{0}.next'; mv '{0}.next' '{0}'",
            stat.display()
        ),
        false => ":".to_owned(),
    };
    let pw = format!(
        "k=$(cat '{0}' 2>/dev/null || echo 0); k=$((k + 1)); echo $k > '{0}'\n[ $k = 3 ] && {{ {bump}; exit 1; }}\n{PW}",
        count.display()
    );
    super::rival::script(&root.join("rivals/qe/bin/pw.x"), &pw);
    let bin = fake(&root, "step -2");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"))
        .with_load(&proc);
    let out = evaluated(&root, &session);
    let _ = std::fs::remove_file(&count);
    out
}

#[test]
fn a_rival_run_that_fails_while_another_task_takes_cpus_is_refused_and_never_scored_as_a_loss() {
    let (outcome, record) = rival_lost_on_its_warm_up("load-lost-busy", true);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("qe: input at 8 threads: the node is not quiet for a timed run: other tasks took")),
        "{outcome:?}"
    );
    assert!(
        record["speed"].as_array().is_none_or(Vec::is_empty),
        "{record}"
    );
    let last = record["load"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["at"], "qe: input at 8 threads", "{record}");
    assert!(last["during"]["loaded"].is_object(), "{record}");
    let (_, record) = rival_lost_on_its_warm_up("load-lost-quiet", false);
    let qe: Vec<&Value> = (record["speed"].as_array().unwrap().iter())
        .filter(|b| b["code"] == "qe")
        .collect();
    assert_eq!(qe.len(), 1, "{record}");
    assert!(qe[0]["warmup"]["loss"].is_string(), "{record}");
    let during = qe[0]["during"].as_array().unwrap();
    assert!(
        !during.is_empty() && during.iter().all(|d| d["quiet"].is_object()),
        "{record}"
    );
}

fn pilot(root: &Tree, bin: &std::path::Path) -> Session {
    use std::ffi::OsString;
    let (bin, records): (OsString, OsString) = (bin.into(), root.join("records").into());
    let vars = move |key: &str| match key {
        "ORBITAR_BIN" => Some(bin.clone()),
        "ORBITAR_TIER" => Some("claim".into()),
        "ORBITAR_PILOT" => Some("1".into()),
        "ORBITAR_RECORD_DIR" => Some(records.clone()),
        _ => None,
    };
    Session::from_vars(root, vars).unwrap().unwrap()
}

#[test]
fn a_timed_runs_other_cpu_time_is_the_nodes_busy_time_less_its_own_children_and_the_harness_over_its_wall_time()
 {
    let root = one_si_tree("load-contention");
    let proc = procfs(&root, 1);
    let (stat, own) = (proc.join("stat"), proc.join("self/stat"));
    let bump = format!(
        "awk '/^cpu /{{printf \"cpu  %d %d %d %d %d %d %d %d 0 0\\n\", $2 + 700, $3 + 11, $4 + 200, $5 + 5000, $6 + 3, $7 + 13, $8 + 17, $9 + 19}}' '{0}' > '{0}.next'; mv '{0}.next' '{0}'\n\
         awk '{{$15 += 23; $16 += 29; $17 += 310; $18 += 130; print}}' '{1}' > '{1}.next'; mv '{1}.next' '{1}'\nstep -2",
        stat.display(),
        own.display()
    );
    let bin = fake(&root, &bump);
    let (_, record) = evaluated(&root, &pilot(&root, &bin).with_load(&proc));
    let log = record["load"].as_array().unwrap();
    let count = |key: &str| log.iter().filter(|r| r.get(key).is_some()).count();
    assert!(count("during") >= 2, "{record}");
    assert_eq!(count("before"), count("during"), "{record}");
    let close = |a: &Value, b: f64| (a.as_f64().unwrap() - b).abs() < 1e-9;
    for r in log.iter().filter_map(|r| r.get("during")) {
        let c = &r["loaded"]["contention"];
        assert!(close(&c["busy_cpu_s"], 9.6), "{c}");
        assert!(close(&c["harness_cpu_s"], 0.52), "{c}");
        assert!(close(&c["child_cpu_s"], 4.4), "{c}");
        assert!(close(&c["others_cpu_s"], 4.68), "{c}");
        let wall = c["wall_s"].as_f64().unwrap();
        assert!(close(&c["share"], 4.68 / (wall * 8.0)), "{c}");
    }
}

#[test]
fn an_unreadable_load_file_stops_a_claim_with_its_path_and_its_text() {
    let root = one_si_tree("load-garbled");
    let proc = procfs(&root, 1);
    crate::write(&root, "proc/loadavg", "garbled\n");
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    let named = format!("{}: cannot read `garbled`", proc.join("loadavg").display());
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains(&named)),
        "{outcome:?}"
    );
}
