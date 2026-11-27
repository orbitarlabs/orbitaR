use super::{CASE, GOOD, INPUT, engine, honest, run_case};
use crate::tree;
use xtask::harness::{Evidence, Metrics, Outcome, Session, Tier};

#[test]
fn repeats_must_print_the_same_bytes() {
    let case = CASE.replace("analytic\"\n", "analytic\"\nrepeats = 2\n");
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [%s]}' $$";
    let (outcome, _) = run_case("engine-repeat", &case, INPUT, body, honest);
    assert!(
        matches!(outcome, Outcome::Fail(why) if why.contains("differs between repeat 1 and repeat 2"))
    );
}

#[test]
fn an_engine_that_sees_the_orientation_fails_the_rotation() {
    let body = "while read -r line; do case \"$line\" in cell_bohr*) c=\"$line\";; esac; done < input.toml\nprintf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [%s]}' ${#c}";
    let (outcome, _) = run_case("engine-rotate", CASE, INPUT, body, honest);
    assert!(matches!(outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `")));
}

const UPRIGHT: &str = "while read -r line; do case \"$line\" in cell_bohr*) c=\"$line\";; esac; done < input.toml\nup='cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]'";

#[test]
fn a_tiny_orientation_effect_still_fails_the_rotation() {
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && e=0.5 || e=0.5000001\nprintf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1]}}' $e"
    );
    let (outcome, _) = run_case("engine-tiny", CASE, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `eigenvalues_ha` by 9.99") && why.contains("e-8 > 1.005")),
        "{outcome:?}"
    );
}

#[test]
fn a_rotated_scf_may_stop_at_another_step() {
    for turned in [13, 14, 40] {
        let body = format!(
            "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && s=12 || s={turned}\nprintf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"scf_steps\": %s}}' $s"
        );
        let (outcome, _) = run_case("engine-steps", CASE, INPUT, &body, honest);
        assert_eq!(outcome, Outcome::Pass, "{turned}");
    }
}

#[test]
fn a_rotated_step_count_must_still_be_a_number() {
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && s=12 || s='\"twelve\"'\nprintf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"scf_steps\": %s}}' \"$s\""
    );
    let (outcome, _) = run_case("engine-steps-shape", CASE, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("`scf_steps` changed shape")),
        "{outcome:?}"
    );
}

#[test]
fn a_rotated_run_may_not_add_a_field() {
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && {GOOD} || printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"spin\": 0}}'"
    );
    let (outcome, _) = run_case("engine-adds", CASE, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rotated output adds `spin`")),
        "{outcome:?}"
    );
}

#[test]
fn a_rotated_run_may_not_change_a_shape() {
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && {GOOD} || printf '{{\"eigenvalues_ha\": [[[0.5, 0.7]]], \"basis_sizes\": [1]}}'"
    );
    let (outcome, _) = run_case("engine-shape", CASE, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("`eigenvalues_ha` changed shape")),
        "{outcome:?}"
    );
}

#[test]
fn a_hung_engine_times_out() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 1\n");
    let (outcome, _) = run_case(
        "engine-timeout",
        &case,
        INPUT,
        "while :; do :; done",
        honest,
    );
    assert!(matches!(outcome, Outcome::Fail(why) if why.contains("timeout after 1 s")));
}

#[test]
fn a_hung_engine_is_gone_when_its_run_times_out() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 1\n");
    let pid = pid_file("engine-gone");
    let body = format!("echo $$ > '{}'\nwhile :; do :; done", pid.display());
    let (outcome, _) = run_case("engine-gone", &case, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("timeout after 1 s")),
        "{outcome:?}"
    );
    let pid = std::fs::read_to_string(&pid).unwrap();
    let alive = std::process::Command::new("sh")
        .args(["-c", "kill -0 \"$1\" 2>/dev/null", "sh", pid.trim()])
        .status()
        .unwrap();
    assert!(!alive.success(), "the engine {} still runs", pid.trim());
}

fn pid_file(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("xtask-{name}-{}.pid", std::process::id()))
}

#[cfg(target_os = "linux")]
fn runs(file: &std::path::Path) -> bool {
    let pid = std::fs::read_to_string(file).unwrap();
    let state = || {
        let stat =
            std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())).unwrap_or_default();
        stat.rsplit_once(") ")
            .is_some_and(|(_, rest)| !rest.starts_with('Z'))
    };
    let start = std::time::Instant::now();
    while state() {
        if start.elapsed() > std::time::Duration::from_secs(10) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

#[cfg(target_os = "linux")]
#[test]
fn a_stopped_shim_ends_at_the_harness_deadline_and_leaves_no_engine() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 1\n");
    let pid = pid_file("engine-stopped");
    let body = format!(
        "echo $$ > '{}'\nkill -s STOP $PPID\nwhile :; do :; done",
        pid.display()
    );
    let start = std::time::Instant::now();
    let (outcome, _) = run_case("engine-stopped", &case, INPUT, &body, honest);
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the shim exited with signal: 9 (SIGKILL): no report 7 s past the run's timeout")),
        "{outcome:?}"
    );
    assert!(!runs(&pid), "the engine still runs");
}

#[cfg(target_os = "linux")]
#[test]
fn a_stopped_shim_ends_at_the_harness_deadline_without_a_path() {
    let test = "harness::engine::a_stopped_shim_ends_at_the_harness_deadline_and_leaves_no_engine";
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact"])
        .env("PATH", "")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 passed"), "{text}");
}

fn shim_into(name: &str, stdout: std::process::Stdio) -> (std::process::ExitStatus, String) {
    use std::io::Read as _;
    let dir = tree(name, &[]);
    let mut shim = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["--peak-rss-shim", "stderr", "5000", "true"])
        .current_dir(&*dir)
        .stdin(std::process::Stdio::piped())
        .stdout(stdout)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let control = shim.stdin.take();
    let mut errors = String::new();
    shim.stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    let status = shim.wait().unwrap();
    drop(control);
    (status, errors)
}

#[test]
fn a_shim_whose_report_has_no_reader_fails_and_says_why() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let (status, errors) = shim_into("engine-shim-no-reader", writer.into());
    assert!(!status.success(), "{status}");
    assert!(
        errors.contains("--peak-rss-shim: the report on stdout: Broken pipe"),
        "{errors}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_shim_whose_report_finds_no_space_fails_and_says_why() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let (status, errors) = shim_into("engine-shim-full", full.into());
    assert!(!status.success(), "{status}");
    assert!(
        errors.contains("--peak-rss-shim: the report on stdout: No space left on device"),
        "{errors}"
    );
}

fn end_with(
    script: &str,
    group: fn(u32) -> Result<(), String>,
    alone: fn(&mut std::process::Child) -> std::io::Result<()>,
) -> Result<std::process::ExitStatus, String> {
    let child = std::process::Command::new("sh")
        .args(["-c", script])
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        done.send(xtask::harness::end_group(
            child,
            group,
            alone,
            xtask::harness::polled,
        ))
    });
    let ended = (result.recv_timeout(std::time::Duration::from_secs(20)))
        .unwrap_or_else(|_| panic!("end_group did not return within 20 s; its child is pid {pid}"));
    if ended
        .as_ref()
        .is_err_and(|why| why.contains(" still runs "))
    {
        let _ = std::process::Command::new("sh")
            .args(["-c", "kill -s KILL \"$1\"", "sh", &pid.to_string()])
            .status();
        let start = std::time::Instant::now();
        while wait4::try_wait4_pid(pid).is_ok_and(|reaped| reaped.is_none())
            && start.elapsed() < std::time::Duration::from_secs(5)
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    ended
}

fn denied_group(pgid: u32) -> Result<(), String> {
    Err(format!("killpg of process group {pgid}: EPERM (injected)"))
}
fn denied_alone(_: &mut std::process::Child) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(1))
}

#[test]
fn a_shim_that_no_signal_reaches_is_left_by_name_within_the_deadline() {
    let start = std::time::Instant::now();
    let why = end_with("kill -s STOP $$", denied_group, denied_alone).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
    assert!(
        why.contains(
            "EPERM (injected); its own kill: Operation not permitted (os error 1); it, pid "
        ) && why.ends_with(" still runs 5 s later; the engine may still run"),
        "{why}"
    );
}

#[test]
fn a_shim_that_no_signal_reaches_but_exits_is_reaped_and_named() {
    let why = end_with(":", denied_group, denied_alone).unwrap_err();
    assert!(
        why.ends_with("its own kill: Operation not permitted (os error 1); exit status: 0; the engine may still run"),
        "{why}"
    );
}

#[test]
fn a_shim_that_no_signal_reaches_but_exits_within_the_deadline_is_reaped_and_named() {
    let mut child = std::process::Command::new("sh")
        .args(["-c", "read line; exit 0"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut held = child.stdin.take();
    let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = std::sync::Arc::clone(&pending);
    let poll = move |child: &mut std::process::Child| {
        let polled = xtask::harness::polled(child);
        if matches!(polled, Ok(None)) {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.take();
        }
        polled
    };
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        done.send(xtask::harness::end_group(
            child,
            denied_group,
            denied_alone,
            poll,
        ))
    });
    let why = (result.recv_timeout(std::time::Duration::from_secs(20)))
        .unwrap_or_else(|_| panic!("end_group did not return within 20 s; its child is pid {pid}"))
        .unwrap_err();
    assert!(pending.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    assert!(
        why.ends_with("its own kill: Operation not permitted (os error 1); exit status: 0; the engine may still run"),
        "{why}"
    );
}

#[test]
fn a_shim_whose_group_signal_fails_is_ended_alone_and_named() {
    let why = end_with("kill -s STOP $$", denied_group, std::process::Child::kill).unwrap_err();
    assert!(
        why.ends_with(
            "EPERM (injected); it alone was ended (signal: 9 (SIGKILL)); the engine may still run"
        ),
        "{why}"
    );
}

#[test]
fn a_shim_whose_group_ends_is_reaped_with_its_status() {
    let status = end_with("exit 3", |_| Ok(()), denied_alone).unwrap();
    assert_eq!(status.code(), Some(3));
}

#[cfg(target_os = "linux")]
#[test]
fn a_killed_shim_fails_the_run_by_name_and_leaves_no_engine() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 20\n");
    let pid = pid_file("engine-orphan");
    let body = format!(
        "echo $$ > '{}'\nkill -s KILL $PPID\nwhile :; do :; done",
        pid.display()
    );
    let start = std::time::Instant::now();
    let (outcome, _) = run_case("engine-orphan", &case, INPUT, &body, honest);
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the shim exited with signal: 9 (SIGKILL): no report;")),
        "{outcome:?}"
    );
    assert!(!runs(&pid), "the engine still runs");
}

#[cfg(target_os = "linux")]
#[test]
fn a_process_that_the_engine_leaves_behind_ends_with_its_run() {
    let pid = pid_file("engine-left");
    let body = format!(
        "sleep 600 >/dev/null 2>&1 3>&- &\necho $! > '{}'\n{GOOD}",
        pid.display()
    );
    let (outcome, _) = run_case("engine-left", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
    assert!(!runs(&pid), "the engine's sleep still runs");
}

#[cfg(target_os = "linux")]
#[test]
fn a_run_is_timed_by_the_shim_from_the_engines_spawn_to_its_exit_not_by_its_own() {
    use std::io::Read as _;
    let dir = tree("engine-shim-hold", &[]);
    let start = std::time::Instant::now();
    let mut shim = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["--peak-rss-shim", "stderr", "20000", "sleep", "1"])
        .current_dir(&*dir)
        .env("ORBITAR_SHIM_HOLD_MS", "1000")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let control = shim.stdin.take();
    let mut report = String::new();
    shim.stdout
        .take()
        .unwrap()
        .read_to_string(&mut report)
        .unwrap();
    assert!(shim.wait().unwrap().success());
    let whole = start.elapsed().as_secs_f64();
    drop(control);
    let report: serde_json::Value = serde_json::from_str(&report).unwrap();
    let engine = report["Ok"]["seconds"].as_f64().unwrap();
    assert!(engine >= 1.0 && engine + 2.0 <= whole, "{engine} {whole}");
}

#[test]
fn a_run_is_timed_from_the_engines_spawn_to_its_exit() {
    let seconds = peak("engine-clock", "sleep 1", "runs_s");
    assert!((1.0..2.5).contains(&seconds), "{seconds}");
}

#[test]
fn a_large_output_does_not_block_the_engine() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 20\n");
    let body = "i=0; printf '{\"eigenvalues_ha\": [[[0.5'; while [ $i -lt 40000 ]; do printf ', 0.5'; i=$((i+1)); done; printf ']]], \"basis_sizes\": [1]}'";
    let (outcome, _) = run_case("engine-large", &case, INPUT, body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn the_sandbox_holds_only_the_engine_and_its_input_and_no_environment() {
    let body = format!(
        "[ -n \"$CARGO\" ] && exit 4\nn=0; for f in *; do n=$((n+1)); done\n[ \"$n\" = 3 ] || exit 5\n[ -e ../benchmarks ] && exit 6\n{GOOD}"
    );
    let (outcome, _) = run_case("engine-sandbox", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

const MPI_ROOT: [&str; 2] = ["OMPI_ALLOW_RUN_AS_ROOT", "OMPI_ALLOW_RUN_AS_ROOT_CONFIRM"];

#[test]
fn the_engine_sees_each_mpi_root_switch_exactly_when_the_harness_has_it() {
    let mut body = String::from("[ -n \"$OMPI_MCA_rmaps_base_oversubscribe\" ] && exit 4\n");
    for key in MPI_ROOT {
        match std::env::var(key) {
            Ok(value) => body.push_str(&format!("[ \"${key}\" = '{value}' ] || exit 5\n")),
            Err(_) => body.push_str(&format!("[ -z \"${{{key}+set}}\" ] || exit 6\n")),
        }
    }
    body.push_str(GOOD);
    let (outcome, _) = run_case("engine-mpi-root", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn the_harness_passes_both_mpi_root_switches_to_the_engine_and_no_other_mpi_variable() {
    let test =
        "harness::engine::the_engine_sees_each_mpi_root_switch_exactly_when_the_harness_has_it";
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact"])
        .envs(MPI_ROOT.map(|key| (key, "1")))
        .env("OMPI_MCA_rmaps_base_oversubscribe", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 passed"), "{text}");
}

fn cells(name: &str, seed: Option<&str>) -> Vec<[[f64; 3]; 3]> {
    let log = std::env::temp_dir().join(format!("xtask-{name}-{}.cells", std::process::id()));
    let body = format!(
        "while read -r line; do case \"$line\" in cell_bohr*) echo \"$line\" >> '{}';; esac; done < input.toml\n{GOOD}",
        log.display()
    );
    let outcome = match seed {
        None => run_case(name, CASE, INPUT, &body, honest).0,
        Some(seed) => {
            let files = [
                ("benchmarks/1_a/case.toml", CASE),
                ("benchmarks/1_a/input.toml", INPUT),
            ];
            let root = tree(name, &files);
            let bin = engine(&root, &body);
            let var = |key: &str| match key {
                "ORBITAR_BIN" => Some(bin.clone().into_os_string()),
                "ORBITAR_SEED" => Some(seed.into()),
                "ORBITAR_RECORD_DIR" => Some(root.join("records").into_os_string()),
                _ => None,
            };
            let session = Session::from_vars(&root, var).unwrap().unwrap();
            session.evaluate("1_a", honest)
        }
    };
    assert_eq!(outcome, Outcome::Pass);
    let text = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_file(&log);
    let number = |v: &toml::Value| v.as_float().or(v.as_integer().map(|i| i as f64)).unwrap();
    text.lines()
        .map(|line| {
            let table: toml::Table = toml::from_str(line).unwrap();
            let rows = table["cell_bohr"].as_array().unwrap();
            std::array::from_fn(|i| {
                std::array::from_fn(|j| number(&rows[i].as_array().unwrap()[j]))
            })
        })
        .collect()
}

#[test]
fn the_rotated_run_turns_the_cell_rigidly_and_by_a_new_angle_each_session() {
    let upright = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]];
    let dot = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let apart = |a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]| {
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    };
    let mut turns = Vec::new();
    for name in ["engine-turn-a", "engine-turn-b"] {
        let seen = cells(name, None);
        assert!(seen.contains(&upright), "{seen:?}");
        let turned: Vec<_> = seen.into_iter().filter(|c| *c != upright).collect();
        assert_eq!(turned.len(), 1, "one rotated run: {turned:?}");
        let t = turned[0];
        for i in 0..3 {
            for j in 0..3 {
                let kept = dot(&t[i], &t[j]) - dot(&upright[i], &upright[j]);
                assert!(kept.abs() < 1e-9, "lengths and angles stay: {t:?}");
            }
        }
        assert!(apart(&t, &upright) > 1e-3, "the cell moves: {t:?}");
        turns.push(t);
    }
    assert!(
        apart(&turns[0], &turns[1]) > 1e-3,
        "each session turns anew: {turns:?}"
    );
}

#[test]
fn a_fixed_seed_replays_its_turn() {
    let upright = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]];
    let turn = |name| {
        cells(name, Some("7"))
            .into_iter()
            .find(|c| *c != upright)
            .unwrap()
    };
    let (a, b) = (turn("engine-seed-a"), turn("engine-seed-b"));
    assert_eq!(a, b);
    let seven = [
        [2.1621692616929553, 2.0336066019656696, 5.214352143098104],
        [-0.01836780300667784, -5.5873156310767245, 2.186679368937848],
        [5.5968461393540965, -0.8039578516112402, -2.0072282045721153],
    ];
    for (x, y) in a.iter().flatten().zip(seven.iter().flatten()) {
        assert!((x - y).abs() < 1e-12, "{a:?}");
    }
}

fn at_three(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = honest(evidence)?;
    if evidence.input("input")?.threads()?.map(|n| n.get()) != Some(3) {
        *metrics.entry("error".to_owned()).or_default() += 1.0;
    }
    Ok(metrics)
}

#[test]
fn a_tier_without_a_sweep_runs_each_input_at_its_own_thread_count() {
    let body = format!(
        "grep -qx 'threads = 3' input.toml && {GOOD} || printf '{{\"eigenvalues_ha\": [[[1.0]]], \"basis_sizes\": [1]}}'"
    );
    let input = format!("{INPUT}\n[parallel]\nthreads = 3\n");
    for name in ["push", "node"] {
        let case = CASE.replace("\"push\"", &format!("\"{name}\""));
        let root = tree(
            &format!("engine-threads-{name}"),
            &[
                ("benchmarks/1_a/case.toml", &case),
                ("benchmarks/1_a/input.toml", &input),
            ],
        );
        let bin = engine(&root, &body);
        let tier = if name == "node" {
            Tier::Node
        } else {
            Tier::Push
        };
        let outcome =
            Session::new(&root, &bin, tier, &root.join("records")).evaluate("1_a", at_three);
        assert_eq!(outcome, Outcome::Pass, "{name}");
    }
}

fn peak_rss(name: &str, body: &str) -> f64 {
    peak(name, body, "peak_rss_bytes")
}

fn peak(name: &str, body: &str, field: &str) -> f64 {
    let (outcome, root) = run_case(name, CASE, INPUT, &format!("{body}\n{GOOD}"), honest);
    assert_eq!(outcome, Outcome::Pass);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    record[field][0].as_f64().unwrap()
}

#[test]
fn each_timed_run_records_the_peak_rss_of_the_engine_and_the_children_it_waited_for() {
    let heavy = peak_rss(
        "engine-rss-heavy",
        "head -c 100000000 /dev/zero | tail -c 100000000 > /dev/null",
    );
    let light = peak_rss("engine-rss-light", ":");
    assert!(heavy > 100e6, "{heavy}");
    assert!(light > 0.0 && light < 20e6, "{light}");
}

#[test]
fn the_peak_rss_of_a_run_holds_none_of_the_harness_memory() {
    let held = std::hint::black_box(vec![1_u8; 100_000_000]);
    let light = peak_rss("engine-rss-held", ":");
    assert!(light > 0.0 && light < 20e6, "{light}");
    drop(held);
}

#[test]
fn an_engine_whose_child_holds_its_stderr_past_its_exit_fails_by_name() {
    let case = CASE.replace("analytic\"\n", "analytic\"\ntimeout_s = 20\n");
    let body = format!("sleep 8 &\n{GOOD}");
    let (outcome, _) = run_case("engine-held", &case, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the engine exited, but its stderr stayed open 2 s later: a child process holds it")),
        "{outcome:?}"
    );
}

#[test]
fn the_peak_pss_sums_the_processes_that_run_side_by_side_and_counts_shared_pages_once() {
    let hold = "head -c 80000000 /dev/zero | tail -c 80000000 | sleep 1";
    let body = format!("( {hold} ) & ( {hold} ) & wait");
    let both = peak("engine-pss-tree", &body, "peak_pss_bytes");
    assert!((160e6..185e6).contains(&both), "{both}");
    assert!(peak_rss("engine-pss-rss", &body) < 100e6);
    let light = peak("engine-pss-light", "sleep 0.2", "peak_pss_bytes");
    assert!(light > 0.0 && light < both / 10.0, "{light} against {both}");
}

#[test]
fn an_engine_whose_child_holds_its_stderr_briefly_past_its_exit_still_passes() {
    let body = format!("sleep 0.5 &\n{GOOD}");
    let (outcome, _) = run_case("engine-brief", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn forces_must_turn_with_the_cell() {
    for (forces, passes) in [("[[0.0, 0.0, 0.0]]", true), ("[[0.0, 0.0, 0.25]]", false)] {
        let body = format!(
            "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"forces_ha_bohr\": {forces}}}'"
        );
        let (outcome, _) = run_case("engine-forces", CASE, INPUT, &body, honest);
        if passes {
            assert_eq!(outcome, Outcome::Pass, "{forces}");
        } else {
            assert!(
                matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn forces_along_a_lattice_vector_turn_with_it() {
    let body = r#"a1=$(sed -n 's/^cell_bohr = \[\(\[[^]]*\]\).*/\1/p' input.toml)
[ -n "$a1" ] || exit 7
printf '{"eigenvalues_ha": [[[0.5]]], "basis_sizes": [1], "forces_ha_bohr": [%s]}' "$a1""#;
    let input = INPUT.replace("[[6.0, 0.0, 0.0],", "[[6.0, 1.0, 2.0],");
    let (outcome, _) = run_case("engine-forces-turn", CASE, &input, body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn forces_that_are_not_three_numbers_per_atom_fail() {
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"forces_ha_bohr\": [[0.0, 0.0]]}'";
    let (outcome, _) = run_case("engine-forces-shape", CASE, INPUT, body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("`forces_ha_bohr` changed shape")),
        "{outcome:?}"
    );
}

#[test]
fn stress_must_turn_with_the_cell() {
    for (stress, passes) in [
        ("[[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]]", true),
        (
            "[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.25]]",
            false,
        ),
    ] {
        let body = format!(
            "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"stress_ha_bohr3\": {stress}}}'"
        );
        let (outcome, _) = run_case("engine-stress", CASE, INPUT, &body, honest);
        if passes {
            assert_eq!(outcome, Outcome::Pass, "{stress}");
        } else {
            assert!(
                matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn a_stress_along_a_lattice_vector_turns_with_it() {
    let body = r#"a1=$(sed -n 's/^cell_bohr = \[\[\([^]]*\)\].*/\1/p' input.toml)
[ -n "$a1" ] || exit 7
t=$(echo "$a1" | awk -F', *' '{ printf "[[%.17g, %.17g, %.17g], [%.17g, %.17g, %.17g], [%.17g, %.17g, %.17g]]", $1*$1, $1*$2, $1*$3, $2*$1, $2*$2, $2*$3, $3*$1, $3*$2, $3*$3 }')
printf '{"eigenvalues_ha": [[[0.5]]], "basis_sizes": [1], "stress_ha_bohr3": %s}' "$t""#;
    let input = INPUT.replace("[[6.0, 0.0, 0.0],", "[[6.0, 1.0, 2.0],");
    let (outcome, _) = run_case("engine-stress-turn", CASE, &input, body, honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_stress_that_is_not_three_by_three_fails() {
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"stress_ha_bohr3\": [[0.0, 0.0, 0.0]]}'";
    let (outcome, _) = run_case("engine-stress-shape", CASE, INPUT, body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("`stress_ha_bohr3` changed shape")),
        "{outcome:?}"
    );
}

fn two_fields(set: &str) -> String {
    format!(
        "{UPRIGHT}\nif [ \"$c\" = \"$up\" ]; then e=-658.0; w=0.25; else {set}; fi\nprintf '{{\"eigenvalues_ha\": [[[0.5, %s]]], \"basis_sizes\": [1], \"ewald_energy_ha\": %s}}' $e $w"
    )
}

fn rotation_of(root: &std::path::Path) -> serde_json::Value {
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&record).unwrap();
    serde_json::json!([
        record["rotation_max"],
        record["rotation_used"],
        record["rotation_field"]
    ])
}

#[test]
fn a_change_at_rounding_scale_of_a_large_field_passes_and_one_past_its_bound_fails() {
    let body = two_fields("e=-658.0000000005; w=0.25");
    let (outcome, root) = run_case("engine-scale-pass", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
    let [max, used, field] = rotation_of(&root)
        .as_array()
        .unwrap()
        .clone()
        .try_into()
        .unwrap();
    let (max, used) = (max.as_f64().unwrap(), used.as_f64().unwrap());
    assert!(
        (max - 5e-10).abs() < 1e-12 && (used - max / 7.58e-10).abs() < 1e-3,
        "{max} {used}"
    );
    assert_eq!(field, "eigenvalues_ha");
    let body = two_fields("e=-658.0000000008; w=0.25");
    let (outcome, root) = run_case("engine-scale-fail", CASE, INPUT, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `eigenvalues_ha` by 8.0") && why.contains("e-10 > 7.58") && why.contains("= 1e-10 + 1e-12 × its largest magnitude")),
        "{outcome:?}"
    );
    assert_eq!(rotation_of(&root)[2], "eigenvalues_ha");
    assert!(rotation_of(&root)[1].as_f64().unwrap() > 1.0);
}

#[test]
fn the_field_nearest_its_own_bound_is_named_not_the_largest_change() {
    let body = two_fields("e=-658.0000000005; w=0.25000000009");
    let (outcome, root) = run_case("engine-scale-share", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
    let rotation = rotation_of(&root);
    assert!(
        (rotation[0].as_f64().unwrap() - 5e-10).abs() < 1e-12,
        "{rotation}"
    );
    assert!(
        (rotation[1].as_f64().unwrap() - 9e-11 / 1.0025e-10).abs() < 1e-3,
        "{rotation}"
    );
    assert_eq!(rotation[2], "ewald_energy_ha");
}

#[test]
fn a_change_of_exactly_its_bound_passes() {
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && w=0.0 || w=1e-10\nprintf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"ewald_energy_ha\": %s}}' $w"
    );
    let (outcome, root) = run_case("engine-scale-tie", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
    assert_eq!(
        rotation_of(&root),
        serde_json::json!([1e-10, 1.0, "ewald_energy_ha"])
    );
}

fn smeared(table: &str) -> String {
    format!("{INPUT}\n[occupations]\n{table}")
}

type State<'a> = (&'a [f64], f64, &'a [f64]);

fn occupied(spins: usize, upright: State, turned: State) -> String {
    let json = |(e, mu, f): State| {
        let per_spin = |v: &[f64]| vec![vec![v.to_vec()]; spins];
        let output = serde_json::json!({
            "eigenvalues_ha": per_spin(e),
            "basis_sizes": [1],
            "fermi_level_ha": mu,
            "occupations": per_spin(f),
        });
        output.to_string()
    };
    format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && printf '%s' '{}' || printf '%s' '{}'",
        json(upright),
        json(turned)
    )
}

fn theta(smearing: &str, x: f64) -> f64 {
    use statrs::function::erf::erfc;
    let pi = std::f64::consts::PI;
    match smearing {
        "fermi-dirac" => 1.0 / (1.0 + x.exp()),
        "gaussian" => erfc(x) / 2.0,
        "methfessel-paxton" => erfc(x) / 2.0 - x * (-x * x).exp() / (2.0 * pi.sqrt()),
        "marzari-vanderbilt" => {
            let u = x + 0.5_f64.sqrt();
            erfc(u) / 2.0 + (-u * u).exp() / (2.0 * pi).sqrt()
        }
        other => unreachable!("{other}"),
    }
}

fn steepest(smearing: &str) -> f64 {
    let h = 1e-4;
    (-80_000..80_000)
        .map(|i| {
            let x = f64::from(i) * h;
            (theta(smearing, x + h / 2.0) - theta(smearing, x - h / 2.0)).abs() / h
        })
        .fold(0.0, f64::max)
}

#[test]
fn an_occupation_may_move_as_far_as_its_smearing_carries_its_eigenvalue_and_the_fermi_level() {
    let (e, mu) = (0.5 + 6e-11, 0.4 + 3e-11);
    let moved = (e - 0.5) + (mu - 0.4);
    let cases = [
        ("fermi-dirac", 1),
        ("fermi-dirac", 2),
        ("gaussian", 1),
        ("methfessel-paxton", 1),
        ("marzari-vanderbilt", 1),
    ];
    for (smearing, spins) in cases {
        let input = smeared(&format!("smearing = \"{smearing}\"\nwidth_ha = 0.01\n"));
        let bound = 1.01e-10 + 2.0 / spins as f64 * steepest(smearing) / 0.01 * moved;
        let body = |share: f64| {
            let f = [1.0 + share * bound];
            occupied(spins, (&[0.5], 0.4, &[1.0]), (&[e], mu, &f))
        };
        let (outcome, root) = run_case("engine-occ-pass", CASE, &input, &body(0.99), honest);
        assert_eq!(outcome, Outcome::Pass, "{smearing} {spins}");
        let rotation = rotation_of(&root);
        assert_eq!(rotation[2], "occupations", "{smearing} {spins}");
        assert!(
            (rotation[1].as_f64().unwrap() - 0.99).abs() < 1e-3,
            "{smearing} {spins} {rotation}"
        );
        let (outcome, _) = run_case("engine-occ-fail", CASE, &input, &body(1.01), honest);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `occupations` by") && why.contains("× its largest magnitude + what the smearing carries from that state's eigenvalue and the Fermi level")),
            "{smearing} {spins} {outcome:?}"
        );
    }
}

#[test]
fn an_occupation_carries_only_its_own_eigenvalue() {
    let input = smeared("smearing = \"fermi-dirac\"\nwidth_ha = 0.01\n");
    let upright: State = (&[0.5, 0.6], 0.4, &[1.0, 1.0]);
    let moved = [0.5 + 6e-11, 0.6];
    let body = occupied(1, upright, (&moved, 0.4, &[1.0 + 1e-9, 1.0 + 1e-10]));
    let (outcome, root) = run_case("engine-occ-own", CASE, &input, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
    let rotation = rotation_of(&root);
    assert!(
        rotation[2] == "occupations" && (rotation[1].as_f64().unwrap() - 0.99).abs() < 1e-3,
        "{rotation}"
    );
    let body = occupied(1, upright, (&moved, 0.4, &[1.0, 1.0 + 1.02e-10]));
    let (outcome, _) = run_case("engine-occ-own", CASE, &input, &body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `occupations` by 1.0") && why.contains("e-10 > 1.01e-10 = ")),
        "{outcome:?}"
    );
}

#[test]
fn fixed_occupations_carry_nothing_from_their_eigenvalues() {
    let e = [0.5 + 6e-11];
    for input in [INPUT.to_owned(), smeared("smearing = \"fixed\"\n")] {
        let body = occupied(1, (&[0.5], 0.4, &[1.0]), (&e, 0.4, &[1.0 + 1.0e-10]));
        let (outcome, _) = run_case("engine-occ-fixed", CASE, &input, &body, honest);
        assert_eq!(outcome, Outcome::Pass, "{input}");
        let body = occupied(1, (&[0.5], 0.4, &[1.0]), (&e, 0.4, &[1.0 + 1.02e-10]));
        let (outcome, _) = run_case("engine-occ-fixed", CASE, &input, &body, honest);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("rigid rotation changed `occupations` by") && why.contains("e-10 > 1.01e-10 = 1e-10 + 1e-12 × its largest magnitude + what")),
            "{input} {outcome:?}"
        );
    }
}

#[test]
fn occupations_need_eigenvalues_of_their_shape_and_a_fermi_level() {
    let input = smeared("smearing = \"fermi-dirac\"\nwidth_ha = 0.01\n");
    let two_bands: State = (&[0.5, 0.6], 0.4, &[1.0]);
    let no_fermi_level = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"occupations\": [[[1.0]]]}'";
    for body in [occupied(1, two_bands, two_bands), no_fermi_level.to_owned()] {
        let (outcome, _) = run_case("engine-occ-shape", CASE, &input, &body, honest);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("`occupations` needs `eigenvalues_ha` of its shape and a number `fermi_level_ha` in both outputs")),
            "{body} {outcome:?}"
        );
    }
}

#[test]
fn an_occupation_bound_needs_a_known_smearing_and_a_positive_width() {
    let width = "a smearing needs a positive occupations.width_ha";
    for (table, why) in [
        (
            "smearing = \"cold\"\nwidth_ha = 0.01\n",
            "no occupation slope for smearing `cold`",
        ),
        ("smearing = \"gaussian\"\nwidth_ha = 0.0\n", width),
        ("smearing = \"gaussian\"\nwidth_ha = -0.01\n", width),
        ("smearing = \"gaussian\"\nwidth_ha = 1e-320\n", width),
        ("smearing = \"gaussian\"\n", width),
    ] {
        let state: State = (&[0.5], 0.4, &[1.0]);
        let (outcome, _) = run_case(
            "engine-occ-input",
            CASE,
            &smeared(table),
            &occupied(1, state, state),
            honest,
        );
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(why)),
            "{table} {outcome:?}"
        );
    }
}

#[test]
fn a_rotated_run_must_read_the_same_dataset_files() {
    let si = "JTH-PBE-v2.0/Si/Si.xml";
    let pin = crate::harness::hex(&<sha2::Sha256 as sha2::Digest>::digest("si"));
    let files = |sha: &str| {
        format!("\"datasets\": {{\"Si\": {{\"path\": \"{si}\", \"sha256\": \"{sha}\"}}}}")
    };
    for (turned, pass) in [(pin.as_str(), true), ("01", false)] {
        let body = format!(
            "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && s={pin} || s={turned}\nprintf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], {}}}' $s",
            files("%s")
        );
        let input = INPUT.replace(
            "[basis]",
            "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n\n[basis]",
        );
        let root = crate::tree(
            "engine-files",
            &[
                ("benchmarks/1_a/case.toml", CASE),
                ("benchmarks/1_a/input.toml", &input),
                ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
            ],
        );
        let url = format!("file://{}/remote", root.display());
        crate::write(
            &root,
            "datasets/jth-2.0.toml",
            &crate::harness::jth(&url, &[("pbe", "Si", "si", None)]),
        );
        let bin = crate::harness::engine(&root, &body);
        let outcome =
            Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
        match pass {
            true => assert_eq!(outcome, Outcome::Pass),
            false => assert!(
                matches!(&outcome, Outcome::Fail(why) if why.contains("\"sha256\":\"01\"")),
                "{outcome:?}"
            ),
        }
    }
}

#[test]
fn a_rotated_run_whose_dataset_entry_differs_in_any_field_fails_the_rotation() {
    let si = "JTH-PBE-v2.0/Si/Si.xml";
    let pin = crate::harness::hex(&<sha2::Sha256 as sha2::Digest>::digest("si"));
    let entry = format!("\"path\": \"{si}\", \"sha256\": \"{pin}\"");
    let body = format!(
        "{UPRIGHT}\n[ \"$c\" = \"$up\" ] && x='' || x=', \"read\": \"twice\"'\nprintf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {{\"Si\": {{{entry}%s}}}}}}' \"$x\""
    );
    let input = INPUT.replace(
        "[basis]",
        "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n\n[basis]",
    );
    let root = crate::tree(
        "engine-files-entry",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", &input),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    crate::write(
        &root,
        "datasets/jth-2.0.toml",
        &crate::harness::jth(&url, &[("pbe", "Si", "si", None)]),
    );
    let bin = crate::harness::engine(&root, &body);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("invariance: the rotated run read other files: `datasets`")),
        "{outcome:?}"
    );
}

fn discrete(choose: &str, drop: &str) -> String {
    let fields = [
        r#""operations": [{"rotation": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0.0, 0.0, 0.0]}, {"rotation": $r, "translation": [0.0, 0.0, 0.0]}]"#,
        r#""coarse_grid": [8, 8, 8]"#,
        r#""fine_grid": [12, 12, 12]"#,
        r#""plane_waves": "0123456789abcdef""#,
        r#""density_spheres": ["0123456789abcdef", "fedcba9876543210"]"#,
        r#""nyquist": false"#,
    ];
    let kept: Vec<&str> = fields
        .into_iter()
        .filter(|f| drop.is_empty() || !f.starts_with(&format!("\"{drop}\"")))
        .collect();
    format!(
        "{UPRIGHT}\n{choose}\ncat <<EOF\n{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"discretization\": {{{}}}}}\nEOF",
        kept.join(", ")
    )
}

const C2Z: &str = "[[-1, 0, 0], [0, -1, 0], [0, 0, 1]]";
const C2X: &str = "[[1, 0, 0], [0, -1, 0], [0, 0, -1]]";

#[test]
fn a_rotated_run_must_make_the_same_discrete_choices() {
    for (turned, pass) in [(C2Z, true), (C2X, false)] {
        let choose = format!("[ \"$c\" = \"$up\" ] && r='{C2Z}' || r='{turned}'");
        let (outcome, _) = run_case(
            "engine-discrete",
            CASE,
            INPUT,
            &discrete(&choose, ""),
            honest,
        );
        match pass {
            true => assert_eq!(outcome, Outcome::Pass),
            false => assert!(
                matches!(&outcome, Outcome::Fail(why) if why.contains("made other discrete choices: `discretization`")),
                "{outcome:?}"
            ),
        }
    }
}

#[test]
fn a_discretization_record_without_one_of_its_fields_fails() {
    for field in [
        "operations",
        "coarse_grid",
        "fine_grid",
        "plane_waves",
        "density_spheres",
        "nyquist",
    ] {
        let (outcome, _) = run_case(
            "engine-whole",
            CASE,
            INPUT,
            &discrete(&format!("r='{C2Z}'"), field),
            honest,
        );
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("is not a whole record") && why.contains(field)),
            "{field}: {outcome:?}"
        );
    }
}

#[test]
fn a_record_that_accepted_no_operation_is_whole() {
    let body = discrete("r=''", "").replace(
        r#"[{"rotation": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "translation": [0.0, 0.0, 0.0]}, {"rotation": $r, "translation": [0.0, 0.0, 0.0]}]"#,
        "[]",
    );
    assert!(body.contains(r#""operations": []"#), "{body}");
    let (outcome, _) = run_case("engine-no-operation", CASE, INPUT, &body, honest);
    assert_eq!(outcome, Outcome::Pass);
}
