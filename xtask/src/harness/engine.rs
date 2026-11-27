use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    f64::consts::FRAC_2_SQRT_PI,
    ffi::OsString,
    fs::{self, File},
    hash::{BuildHasher, RandomState},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::symlink,
        process::{CommandExt as _, ExitStatusExt as _},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant, SystemTime},
};
use wait4::Wait4;

const ROTATION_ABS: f64 = 1e-10;
const ROTATION_REL: f64 = 1e-12;

pub(crate) fn rigid(used: f64) -> bool {
    used <= 1.0
}

pub(crate) fn excess(change: &Change) -> Option<String> {
    let (key, by, bound) = change.field.as_ref().filter(|_| !rigid(change.used))?;
    let carried = match FIELDS.iter().find(|(name, _)| name == key) {
        Some((_, Transform::Occupations)) => {
            " + what the smearing carries from that state's eigenvalue and the Fermi level (none for fixed occupations)"
        }
        _ => "",
    };
    Some(format!(
        "a rigid rotation changed `{key}` by {by:e} > {bound:e} = {ROTATION_ABS:e} + {ROTATION_REL:e} × its largest \
         magnitude{carried}"
    ))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Change {
    pub(crate) max: f64,
    pub(crate) used: f64,
    pub(crate) field: Option<(String, f64, f64)>,
}

impl Change {
    pub(crate) fn add(&mut self, other: Change) {
        self.max = self.max.max(other.max);
        if other.used > self.used {
            self.used = other.used;
            self.field = other.field;
        }
    }
}

#[derive(Clone, Copy)]
enum Transform {
    Invariant,
    Vectors,
    Tensor,
    Occupations,
    Steps,
    Files,
    Discrete,
}

const FIELDS: [(&str, Transform); 25] = [
    ("eigenvalues_ha", Transform::Invariant),
    ("basis_sizes", Transform::Invariant),
    ("ewald_energy_ha", Transform::Invariant),
    ("hartree_energy_ha", Transform::Invariant),
    ("xc_energy_density_ha_bohr3", Transform::Invariant),
    ("paw_xc_energy_ha", Transform::Invariant),
    ("paw_xc_d_ha", Transform::Invariant),
    ("kpoints_fractional", Transform::Invariant),
    ("kpoint_weights", Transform::Invariant),
    ("free_energy_ha", Transform::Invariant),
    ("internal_energy_ha", Transform::Invariant),
    ("kinetic_energy_ha", Transform::Invariant),
    ("xc_energy_ha", Transform::Invariant),
    ("psp_core_energy_ha", Transform::Invariant),
    ("local_energy_ha", Transform::Invariant),
    ("one_center_energy_ha", Transform::Invariant),
    ("smearing_energy_ha", Transform::Invariant),
    ("fermi_level_ha", Transform::Invariant),
    ("occupations", Transform::Occupations),
    ("magnetization_bohr_magneton", Transform::Invariant),
    ("forces_ha_bohr", Transform::Vectors),
    ("stress_ha_bohr3", Transform::Tensor),
    ("scf_steps", Transform::Steps),
    ("datasets", Transform::Files),
    ("discretization", Transform::Discrete),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Step {
    pub(crate) seconds: f64,
    pub(crate) free_energy_ha: f64,
}

pub(crate) type StepLine = fn(&str) -> Option<f64>;

#[derive(Clone, Copy)]
pub(crate) enum Stream {
    Stderr,
    Stdout,
}

impl Stream {
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Stream::Stderr => ("stderr", "stdout"),
            Stream::Stdout => ("stdout", "stderr"),
        }
    }
}

pub(crate) struct Run {
    pub(crate) stdout: String,
    pub(crate) seconds: f64,
    pub(crate) peak_rss_bytes: u64,
    pub(crate) peak_pss_bytes: u64,
    pub(crate) steps: Vec<Step>,
    pub(crate) lines: Vec<(f64, String)>,
}

pub(crate) const POLL: Duration = Duration::from_millis(50);
const HELD: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
pub(crate) enum Failure {
    Harness(String),
    Engine(String),
}

impl Failure {
    pub(crate) fn map(self, f: impl FnOnce(String) -> String) -> Failure {
        match self {
            Failure::Harness(e) => Failure::Harness(f(e)),
            Failure::Engine(e) => Failure::Engine(f(e)),
        }
    }
}

const TEXT_FILE_BUSY: i32 = 26;

pub(crate) fn repeat(
    bin: &Path,
    input: &str,
    datasets: &[(String, Vec<u8>)],
    repeats: u32,
    timeout: Duration,
) -> Result<Vec<Run>, Failure> {
    let mut runs: Vec<Run> = Vec::new();
    for _ in 0..repeats {
        let run = once(bin, input, datasets, timeout)?;
        same_output(&runs, &run)?;
        runs.push(run);
    }
    Ok(runs)
}

pub(crate) fn same_output(runs: &[Run], next: &Run) -> Result<(), Failure> {
    match runs.first() {
        Some(first) if first.stdout != next.stdout => Err(Failure::Engine(format!(
            "output differs between repeat 1 and repeat {}",
            runs.len() + 1
        ))),
        _ => Ok(()),
    }
}

pub(crate) fn once(
    bin: &Path,
    input: &str,
    datasets: &[(String, Vec<u8>)],
    timeout: Duration,
) -> Result<Run, Failure> {
    let files: Vec<(String, &[u8])> = std::iter::once(("input.toml".to_owned(), input.as_bytes()))
        .chain(
            (datasets.iter()).map(|(path, bytes)| (format!("datasets/{path}"), bytes.as_slice())),
        )
        .collect();
    let build = |dir: &Path| {
        let engine = dir.join("orbitar");
        fs::copy(bin, &engine).map_err(|e| Failure::Harness(format!("{}: {e}", bin.display())))?;
        let mut command = Command::new(&engine);
        command.args(["run", "input.toml"]);
        Ok(command)
    };
    let run = cold(&files, build, Stream::Stderr, orbitar_step, timeout).and_then(agreed)?;
    super::datasets::agree(&run.stdout, input, datasets).map_err(Failure::Engine)?;
    Ok(run)
}

const PASSED: [&str; 2] = ["OMPI_ALLOW_RUN_AS_ROOT", "OMPI_ALLOW_RUN_AS_ROOT_CONFIRM"];

pub(crate) fn cold_env(built: &Command) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let passed = (PASSED.iter()).filter_map(|&key| Some((key.into(), std::env::var_os(key)?)));
    let own =
        (built.get_envs()).filter_map(|(key, value)| Some((key.to_owned(), value?.to_owned())));
    passed.chain(own).collect()
}

pub(crate) fn cold(
    files: &[(String, &[u8])],
    build: impl FnOnce(&Path) -> Result<Command, Failure>,
    stream: Stream,
    step: StepLine,
    timeout: Duration,
) -> Result<Run, Failure> {
    let dir = sandbox().map_err(Failure::Harness)?;
    let scope = dir.with_extension("scope");
    let result = place(&dir, files)
        .and_then(|()| build(&dir))
        .and_then(|built| {
            match CONTAINED.with_borrow(|c| c.as_ref().map(|c| c.wrap(&built, &scope))) {
                Some(wrapped) => wrapped,
                None => Ok(built),
            }
        })
        .and_then(|built| {
            let own = cold_env(&built);
            let mut command = if SHIMMED {
                let shim = shim_path()?;
                let mut command = Command::new(&shim);
                (command.arg(SHIM).arg(stream.names().0))
                    .arg(timeout.as_millis().to_string())
                    .arg(built.get_program())
                    .args(built.get_args());
                command
            } else {
                built
            };
            command
                .current_dir(&dir)
                .env_clear()
                .envs(own)
                .env("HOME", &dir)
                .env("TMPDIR", &dir);
            if SHIMMED {
                shimmed(command, timeout)
            } else {
                watched(&mut command, &dir, stream, timeout)
            }
        })
        .and_then(|watch| {
            LAST_PEAKS.set(Some([watch.peak_rss_bytes, watch.peak_pss_bytes]));
            finish(watch, stream, step)
        });
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&scope);
    result
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Limit {
    Scope,
    Prlimit,
}

impl Limit {
    fn name(&self) -> &'static str {
        match self {
            Limit::Scope => "systemd-run",
            Limit::Prlimit => "prlimit",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Containment {
    limit: Limit,
    program: PathBuf,
    max_bytes: u64,
    env: Vec<(OsString, OsString)>,
}

const SCOPE_ENV: [&str; 2] = ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"];

const EXEC_DEFAULT_PATH: &str = "/usr/bin:/bin";

impl Containment {
    pub(crate) fn probe(
        var: &dyn Fn(&str) -> Option<OsString>,
        max_bytes: u64,
    ) -> Option<Containment> {
        let path = var("PATH")?;
        let find = |name: &str| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|p| p.is_file())
        };
        let env: Vec<(OsString, OsString)> = (SCOPE_ENV.iter())
            .filter_map(|&key| Some((OsString::from(key), var(key)?)))
            .collect();
        let candidates = [(Limit::Scope, env), (Limit::Prlimit, Vec::new())];
        candidates.into_iter().find_map(|(limit, env)| {
            let containment = Containment {
                program: find(limit.name())?,
                limit,
                max_bytes,
                env,
            };
            let scope = sandbox().ok()?;
            let check = containment.wrap(&Command::new("true"), &scope);
            let works = check.is_ok_and(|mut check| {
                (check.stdout(Stdio::null()).stderr(Stdio::null()))
                    .status()
                    .is_ok_and(|s| s.success())
            });
            let _ = fs::remove_dir_all(&scope);
            works.then_some(containment)
        })
    }

    fn flags(&self) -> Vec<String> {
        match self.limit {
            Limit::Scope => vec![
                "--user".into(),
                "--scope".into(),
                "--quiet".into(),
                "-p".into(),
                format!("MemoryMax={}", self.max_bytes),
                "-p".into(),
                "MemorySwapMax=0".into(),
                "--".into(),
            ],
            Limit::Prlimit => vec![format!("--as={}", self.max_bytes), "--".into()],
        }
    }

    fn wrap(&self, built: &Command, scope: &Path) -> Result<Command, Failure> {
        let tool = scope.join(self.limit.name());
        (fs::create_dir_all(scope))
            .and_then(|()| symlink(&self.program, &tool))
            .map_err(|e| Failure::Harness(format!("the scope's tool {}: {e}", tool.display())))?;
        let mut command = match self.limit {
            Limit::Scope => Command::new("systemd-run"),
            Limit::Prlimit => Command::new("prlimit"),
        };
        command
            .env_clear()
            .args(self.flags())
            .arg(built.get_program())
            .args(built.get_args());
        for (key, value) in built.get_envs() {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        let own = (built.get_envs()).find_map(|(key, value)| value.filter(|_| key == "PATH"));
        let mut path = scope.as_os_str().to_owned();
        path.push(":");
        path.push(own.unwrap_or(EXEC_DEFAULT_PATH.as_ref()));
        command.env("PATH", path).envs(self.env.iter().cloned());
        Ok(command)
    }

    pub(crate) fn tree_wide(&self) -> bool {
        self.limit == Limit::Scope
    }

    pub(crate) fn describe(&self) -> Value {
        let what = match self.limit {
            Limit::Scope => {
                "systemd-run --user --scope: MemoryMax over the whole process tree, no swap"
            }
            Limit::Prlimit => {
                "prlimit --as: the address space of each process alone, not the tree; a pilot only"
            }
        };
        serde_json::json!({
            "limit": what,
            "program": self.program,
            "max_bytes": self.max_bytes,
            "argv": self.flags(),
        })
    }
}

pub(crate) fn contained<T>(containment: Option<Containment>, run: impl FnOnce() -> T) -> T {
    let before = CONTAINED.replace(containment);
    let out = run();
    CONTAINED.set(before);
    out
}

thread_local! {
    static CONTAINED: std::cell::RefCell<Option<Containment>> = const { std::cell::RefCell::new(None) };
    static LAST_PEAKS: std::cell::Cell<Option<[u64; 2]>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn forget_peaks() {
    LAST_PEAKS.set(None);
}

pub(crate) fn last_peaks() -> Option<[u64; 2]> {
    LAST_PEAKS.get()
}

const SHIMMED: bool = cfg!(target_os = "linux");

pub const SHIM: &str = "--peak-rss-shim";

const HOLD: &str = "ORBITAR_SHIM_HOLD_MS";

const LATE: Duration = Duration::from_secs(5);

pub fn shim(args: &[OsString]) -> ExitCode {
    let [stream, millis, engine, rest @ ..] = args else {
        eprintln!("usage: xtask {SHIM} stdout|stderr <timeout ms> <engine> [args]");
        return ExitCode::FAILURE;
    };
    let stream = match stream.to_str() {
        Some("stdout") => Stream::Stdout,
        Some("stderr") => Stream::Stderr,
        _ => {
            eprintln!("{SHIM}: the stamped stream is stdout or stderr, not {stream:?}");
            return ExitCode::FAILURE;
        }
    };
    let Some(timeout) = (millis.to_str()).and_then(|m| m.parse().ok()) else {
        eprintln!("{SHIM}: the timeout is a count of ms, not {millis:?}");
        return ExitCode::FAILURE;
    };
    std::thread::spawn(|| {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        if let Err(e) = kill_group(std::process::id()) {
            eprintln!("{SHIM}: {e}");
        }
    });
    let hold = (std::env::var(HOLD).ok()).and_then(|ms| ms.parse().ok());
    let hold = hold.map_or(Duration::ZERO, Duration::from_millis);
    std::thread::sleep(hold);
    let watch = std::env::current_dir()
        .map_err(|e| Failure::Harness(format!("{SHIM}: the directory: {e}")))
        .and_then(|dir| {
            let mut command = Command::new(engine);
            command.args(rest);
            watched(&mut command, &dir, stream, Duration::from_millis(timeout))
        });
    std::thread::sleep(hold);
    let mut out = std::io::stdout().lock();
    let report = (serde_json::to_writer(&mut out, &watch).map_err(std::io::Error::from))
        .and_then(|()| out.flush());
    match report {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{SHIM}: the report on stdout: {e}");
            ExitCode::FAILURE
        }
    }
}

fn shimmed(mut command: Command, timeout: Duration) -> Result<Watched, Failure> {
    let program = command.get_program().to_string_lossy().into_owned();
    (command.process_group(0).stdin(Stdio::piped()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut shim = spawn(&mut command).map_err(|e| Failure::Harness(format!("{program}: {e}")))?;
    let pipes = shim.stdout.take().zip(shim.stderr.take());
    let (report, errors) = pipes.map(|(out, err)| (reading(out), reading(err))).unzip();
    let report = report.map(|r| r.recv_timeout(timeout + HELD + LATE));
    let status = match end_group(shim, kill_group, Child::kill, polled) {
        Ok(status) => status,
        Err(e) => return Err(Failure::Harness(format!("the shim: {e}"))),
    };
    let errors = (errors.and_then(|e| e.recv_timeout(LATE).ok()))
        .and_then(Result::ok)
        .unwrap_or_default();
    let why = |what: String| {
        Failure::Harness(format!(
            "the shim exited with {status}: {what}; its stderr: {:?}",
            errors.trim()
        ))
    };
    match report {
        None => Err(why("no pipes".into())),
        Some(Err(_)) => Err(why(format!(
            "no report {} s past the run's timeout",
            (HELD + LATE).as_secs()
        ))),
        Some(Ok(Err(e))) => Err(why(format!("its report is unreadable: {e}"))),
        Some(Ok(Ok(text))) if text.is_empty() => Err(why("no report".into())),
        Some(Ok(Ok(text))) => serde_json::from_str(&text)
            .map_err(|e| why(format!("its report is not a run: {e}: {text:.200}")))?,
    }
}

pub fn end_group(
    mut shim: Child,
    group: impl FnOnce(u32) -> Result<(), String>,
    alone: impl FnOnce(&mut Child) -> std::io::Result<()>,
    mut poll: impl FnMut(&mut Child) -> std::io::Result<Option<ExitStatus>>,
) -> Result<ExitStatus, String> {
    let pid = shim.id();
    let Err(first) = group(pid) else {
        return (shim.wait4())
            .map(|u| u.status)
            .map_err(|e| format!("its reap: {e}"));
    };
    let Err(second) = alone(&mut shim) else {
        let reaped =
            (shim.wait4()).map_or_else(|e| format!("its reap: {e}"), |u| u.status.to_string());
        return Err(format!(
            "{first}; it alone was ended ({reaped}); the engine may still run"
        ));
    };
    let start = Instant::now();
    let reaped = loop {
        match poll(&mut shim) {
            Ok(Some(status)) => break status.to_string(),
            Err(e) => break format!("its reap: {e}"),
            Ok(None) if start.elapsed() >= LATE => {
                break format!("it, pid {pid}, still runs {} s later", LATE.as_secs());
            }
            Ok(None) => std::thread::sleep(POLL),
        }
    };
    Err(format!(
        "{first}; its own kill: {second}; {reaped}; the engine may still run"
    ))
}

pub fn polled(child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
    (child.try_wait4()).map(|usage| usage.map(|u| u.status))
}

fn reading(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<std::io::Result<String>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = send.send(pipe.read_to_string(&mut text).map(|_| text));
    });
    receive
}

fn kill_group(pgid: u32) -> Result<(), String> {
    let group = i32::try_from(pgid).map_err(|e| format!("process group {pgid}: {e}"))?;
    match killpg(Pid::from_raw(group), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(e) => Err(format!("killpg of process group {pgid}: {e}")),
    }
}

fn shim_path() -> Result<PathBuf, Failure> {
    let exe = std::env::current_exe().map_err(|e| Failure::Harness(format!("the shim: {e}")))?;
    let dir = exe.parent().unwrap_or(&exe);
    let dir = if dir.ends_with("deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    Ok(dir.join(format!("xtask{}", std::env::consts::EXE_SUFFIX)))
}

fn sandbox() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tag = RandomState::new().hash_one((std::process::id(), nanos));
    let dir = std::env::temp_dir().join(format!("orbitar-{tag:016x}"));
    fs::create_dir(&dir).map_err(|e| format!("sandbox {}: {e}", dir.display()))?;
    Ok(dir)
}

fn place(dir: &Path, files: &[(String, &[u8])]) -> Result<(), Failure> {
    for (path, bytes) in files {
        let copy = dir.join(path);
        let placed = (copy.parent())
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| File::create_new(&copy))
            .and_then(|mut file| file.write_all(bytes));
        placed.map_err(|e| {
            Failure::Harness(format!(
                "{path}: {e}; the sandbox stages each file where nothing is"
            ))
        })?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Watched {
    lines: Vec<(f64, String)>,
    written: String,
    seconds: f64,
    status: i32,
    peak_rss_bytes: u64,
    peak_pss_bytes: u64,
    #[serde(default)]
    timed_out_s: Option<u64>,
}

fn watched(
    command: &mut Command,
    dir: &Path,
    stream: Stream,
    timeout: Duration,
) -> Result<Watched, Failure> {
    let program = command.get_program().to_string_lossy().into_owned();
    let harness = |e: std::io::Error| Failure::Harness(format!("{program}: {e}"));
    let (name, other) = stream.names();
    let path = dir.join(other);
    let file = |e: std::io::Error| Failure::Harness(format!("{}: {e}", path.display()));
    let sink = File::create(&path).map_err(file)?;
    command.stdin(Stdio::null());
    match stream {
        Stream::Stderr => command.stdout(sink).stderr(Stdio::piped()),
        Stream::Stdout => command.stderr(sink).stdout(Stdio::piped()),
    };
    let mut child = spawn(command).map_err(harness)?;
    let start = Instant::now();
    let pipe: Box<dyn Read + Send> = match stream {
        Stream::Stderr => Box::new(
            child
                .stderr
                .take()
                .ok_or_else(|| Failure::Harness("no stderr pipe".into()))?,
        ),
        Stream::Stdout => Box::new(
            child
                .stdout
                .take()
                .ok_or_else(|| Failure::Harness("no stdout pipe".into()))?,
        ),
    };
    let (closing, closed) = mpsc::channel::<()>();
    let listener = std::thread::spawn(move || {
        let lines = listen(pipe, start);
        drop(closing);
        lines
    });
    let mut pss: u64 = 0;
    let mut exited = None;
    while closed.recv_timeout(POLL) == Err(RecvTimeoutError::Timeout) {
        match exited {
            None => {
                pss = pss.max(linux_process_tree_pss_bytes(child.id()));
                exited = child
                    .try_wait4()
                    .map_err(harness)?
                    .map(|u| (u, start.elapsed()));
            }
            Some((_, at)) if start.elapsed() > at + HELD => {
                return Err(Failure::Engine(format!(
                    "the engine exited, but its {name} stayed open {} s later: a child process holds it",
                    HELD.as_secs()
                )));
            }
            Some(_) => {}
        }
        if exited.is_none() && start.elapsed() >= timeout {
            break;
        }
    }
    let mut timed_out_s = None;
    let (usage, elapsed) = match exited {
        Some(done) => done,
        None => loop {
            if let Some(usage) = child.try_wait4().map_err(harness)? {
                break (usage, start.elapsed());
            }
            if start.elapsed() >= timeout {
                pss = pss.max(linux_process_tree_pss_bytes(child.id()));
                let _ = child.kill();
                timed_out_s = Some(timeout.as_secs());
                break (child.wait4().map_err(harness)?, start.elapsed());
            }
            std::thread::sleep(Duration::from_micros(100));
        },
    };
    if timed_out_s.is_some() {
        return Ok(Watched {
            lines: Vec::new(),
            written: String::new(),
            seconds: elapsed.as_secs_f64(),
            status: usage.status.into_raw(),
            peak_rss_bytes: usage.rusage.maxrss,
            peak_pss_bytes: pss,
            timed_out_s,
        });
    }
    let lines =
        (listener.join()).map_err(|_| Failure::Harness(format!("the {name} reader panicked")))?;
    Ok(Watched {
        lines,
        written: fs::read_to_string(&path).map_err(file)?,
        seconds: elapsed.as_secs_f64(),
        status: usage.status.into_raw(),
        peak_rss_bytes: usage.rusage.maxrss,
        peak_pss_bytes: pss,
        timed_out_s,
    })
}

fn finish(watch: Watched, stream: Stream, step: StepLine) -> Result<Run, Failure> {
    let mut steps = Vec::new();
    let mut rest = Vec::new();
    for (at, line) in &watch.lines {
        match step(line) {
            Some(free_energy_ha) => steps.push(Step {
                seconds: *at,
                free_energy_ha,
            }),
            None => rest.push(line.as_str()),
        }
    }
    if let Some(seconds) = watch.timed_out_s {
        return Err(Failure::Engine(format!("timeout after {seconds} s")));
    }
    let status = ExitStatus::from_raw(watch.status);
    if !status.success() {
        let why = match stream {
            Stream::Stderr => rest.join("\n"),
            Stream::Stdout => format!(
                "{}\n{}",
                watch.written,
                rest[rest.len().saturating_sub(5)..].join("\n")
            ),
        };
        return Err(Failure::Engine(format!(
            "engine exited with {status}: {}",
            why.trim()
        )));
    }
    let stdout = match stream {
        Stream::Stderr => watch.written,
        Stream::Stdout => (watch.lines.iter())
            .map(|(_, l)| format!("{l}\n"))
            .collect(),
    };
    Ok(Run {
        stdout,
        seconds: watch.seconds,
        peak_rss_bytes: watch.peak_rss_bytes,
        peak_pss_bytes: watch.peak_pss_bytes,
        steps,
        lines: watch.lines,
    })
}

fn linux_process_tree_pss_bytes(pid: u32) -> u64 {
    let proc = PathBuf::from(format!("/proc/{pid}"));
    let own = (fs::read_to_string(proc.join("smaps_rollup")).ok())
        .and_then(|s| {
            let kib = s.lines().find_map(|l| l.strip_prefix("Pss:"))?;
            kib.trim().strip_suffix(" kB")?.trim().parse::<u64>().ok()
        })
        .map_or(0, |kib| kib * 1024);
    let children = (fs::read_dir(proc.join("task"))
        .into_iter()
        .flatten()
        .flatten())
    .map(|task| fs::read_to_string(task.path().join("children")).unwrap_or_default())
    .collect::<Vec<_>>();
    own + (children.iter().flat_map(|c| c.split_whitespace()))
        .filter_map(|c| c.parse().ok())
        .map(linux_process_tree_pss_bytes)
        .sum::<u64>()
}

fn listen(pipe: impl Read, start: Instant) -> Vec<(f64, String)> {
    (BufReader::new(pipe).split(b'\n'))
        .map_while(Result::ok)
        .map(|line| {
            (
                start.elapsed().as_secs_f64(),
                String::from_utf8_lossy(&line).into_owned(),
            )
        })
        .collect()
}

fn orbitar_step(line: &str) -> Option<f64> {
    match line.split(' ').collect::<Vec<_>>()[..] {
        ["scf_step", _, "free_energy_ha", f] => f.parse().ok(),
        _ => None,
    }
}

pub(crate) fn free_energy_ha(stdout: &str) -> Option<f64> {
    (serde_json::from_str::<Value>(stdout).ok()).and_then(|out| out["free_energy_ha"].as_f64())
}

fn agreed(run: Run) -> Result<Run, Failure> {
    let Some(last) = run.steps.last() else {
        return Ok(run);
    };
    let printed = free_energy_ha(&run.stdout);
    if printed == Some(last.free_energy_ha) {
        Ok(run)
    } else {
        Err(Failure::Engine(format!(
            "the last step line gives F = {}, but stdout's free_energy_ha is {printed:?}",
            last.free_energy_ha
        )))
    }
}

fn spawn(command: &mut Command) -> std::io::Result<Child> {
    for _ in 0..200 {
        match command.spawn() {
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            other => return other,
        }
    }
    command.spawn()
}

pub(crate) fn set(
    input: &mut toml::Table,
    table: &str,
    key: &str,
    value: toml::Value,
) -> Result<(), String> {
    let slot = (input.entry(table))
        .or_insert_with(|| toml::Table::new().into())
        .as_table_mut()
        .ok_or_else(|| format!("{table}: the input's {table} is not a table"))?;
    slot.insert(key.into(), value);
    Ok(())
}

pub(crate) fn rotated(input: &str, seed: u64) -> Result<(String, [[f64; 3]; 3]), String> {
    let mut table: toml::Table = toml::from_str(input).map_err(|e| format!("input: {e}"))?;
    let structure = table
        .get_mut("structure")
        .and_then(toml::Value::as_table_mut)
        .ok_or("invariance: the input has no structure.cell_bohr to rotate")?;
    let rows = structure
        .get("cell_bohr")
        .and_then(toml::Value::as_array)
        .ok_or("invariance: the input has no structure.cell_bohr to rotate")?;
    let r = rotation(seed);
    let mut turned = Vec::new();
    for row in rows {
        let v: Vec<f64> = row
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_float().or(x.as_integer().map(|i| i as f64)))
            .collect();
        let [x, y, z] = v[..] else {
            return Err("invariance: structure.cell_bohr rows need 3 numbers".into());
        };
        turned.push(toml::Value::Array(
            r.iter()
                .map(|row| (row[0] * x + row[1] * y + row[2] * z).into())
                .collect(),
        ));
    }
    structure.insert("cell_bohr".into(), toml::Value::Array(turned));
    let text = toml::to_string(&table).map_err(|e| e.to_string())?;
    Ok((text, r))
}

pub(crate) fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn rotation(seed: u64) -> [[f64; 3]; 3] {
    let mut state = seed;
    let mut uniform = || (splitmix(&mut state) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
    let q = loop {
        let q = [uniform(), uniform(), uniform(), uniform()];
        let n = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n > 0.1 && n <= 1.0 {
            break q.map(|x| x / n);
        }
    };
    let [w, x, y, z] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

pub(crate) fn deviation(
    input: &str,
    original: &str,
    turned: &str,
    rotation: [[f64; 3]; 3],
) -> Result<Change, Failure> {
    let parse = |s: &str| -> Result<serde_json::Map<String, Value>, Failure> {
        match serde_json::from_str(s) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(Failure::Engine("engine stdout is not a JSON object".into())),
            Err(e) => Err(Failure::Engine(format!("engine stdout is not JSON: {e}"))),
        }
    };
    let (a, b) = (parse(original)?, parse(turned)?);
    let mut worst = Change::default();
    for (key, value) in &a {
        let Some((_, transform)) = FIELDS.iter().find(|(name, _)| name == key) else {
            return Err(Failure::Harness(format!(
                "invariance: no rule for output field `{key}`; add one to xtask/src/harness/engine.rs"
            )));
        };
        let other = b
            .get(key)
            .ok_or_else(|| Failure::Engine(format!("invariance: rotated output lacks `{key}`")))?;
        if let Transform::Discrete = transform {
            for record in [value, other] {
                super::Discretization::of(record)
                    .map_err(|e| Failure::Engine(format!("invariance: `{key}`: {e}")))?;
            }
        }
        if let Transform::Files | Transform::Discrete = transform {
            if value != other {
                let what = match transform {
                    Transform::Files => "read other files",
                    _ => "made other discrete choices",
                };
                return Err(Failure::Engine(format!(
                    "invariance: the rotated run {what}: `{key}` {other}"
                )));
            }
            continue;
        }
        let expected = match transform {
            Transform::Vectors => turn(value, rotation),
            Transform::Tensor => turn_tensor(value, rotation),
            Transform::Invariant
            | Transform::Occupations
            | Transform::Steps
            | Transform::Files
            | Transform::Discrete => Some(value.clone()),
        };
        let change = (expected.as_ref())
            .and_then(|expected| difference(expected, other))
            .ok_or_else(|| Failure::Engine(format!("invariance: `{key}` changed shape")))?;
        let bound = ROTATION_ABS + ROTATION_REL * magnitude(value);
        worst.add(match transform {
            Transform::Steps | Transform::Files | Transform::Discrete => continue,
            Transform::Occupations => occupations(input, (&a, &b), bound)?,
            Transform::Invariant | Transform::Vectors | Transform::Tensor => Change {
                max: change,
                used: change / bound,
                field: Some((key.clone(), change, bound)),
            },
        });
    }
    if let Some(key) = b.keys().find(|k| !a.contains_key(*k)) {
        return Err(Failure::Engine(format!(
            "invariance: rotated output adds `{key}`"
        )));
    }
    Ok(worst)
}

fn occupations(
    input: &str,
    (a, b): (
        &serde_json::Map<String, Value>,
        &serde_json::Map<String, Value>,
    ),
    own: f64,
) -> Result<Change, Failure> {
    let need = || {
        Failure::Engine(
            "invariance: `occupations` needs `eigenvalues_ha` of its shape and a number `fermi_level_ha` in both \
             outputs"
                .into(),
        )
    };
    let read = |key: &str| a.get(key).zip(b.get(key)).ok_or_else(need);
    let (f, g) = read("occupations")?;
    let slope = slope(input, f)?;
    let (e, h) = read("eigenvalues_ha")?;
    let (mu, nu) = read("fermi_level_ha")?;
    let fermi = mu.as_f64().zip(nu.as_f64()).map(|(p, q)| gap(p, q));
    let (Some(states), Some(levels), Some(_), Some(fermi)) =
        (pairs(f, g), pairs(e, h), pairs(f, e), fermi)
    else {
        return Err(need());
    };
    let mut worst = Change::default();
    for ((p, q), (x, y)) in states.into_iter().zip(levels) {
        let (by, bound) = (gap(p, q), own + slope * (gap(x, y) + fermi));
        worst.add(Change {
            max: by,
            used: by / bound,
            field: Some(("occupations".into(), by, bound)),
        });
    }
    Ok(worst)
}

fn slope(input: &str, occupations: &Value) -> Result<f64, Failure> {
    let input: toml::Table = (input.parse())
        .map_err(|e| Failure::Harness(format!("invariance: the input is not TOML: {e}")))?;
    let section = input.get("occupations").and_then(toml::Value::as_table);
    let read = |key: &str| section.and_then(|t| t.get(key));
    let steepest = match read("smearing").and_then(toml::Value::as_str) {
        None | Some("fixed") => return Ok(0.0),
        Some("fermi-dirac") => 0.25,
        Some("gaussian") => FRAC_2_SQRT_PI / 2.0,
        Some("methfessel-paxton") => 0.75 * FRAC_2_SQRT_PI,
        Some("marzari-vanderbilt") => {
            let root5 = 5.0_f64.sqrt();
            (1.0 + root5) / 2.0 * (-(3.0 - root5) / 4.0).exp() * FRAC_2_SQRT_PI / 2.0
        }
        Some(other) => {
            return Err(Failure::Harness(format!(
                "invariance: no occupation slope for smearing `{other}`; add one to xtask/src/harness/engine.rs"
            )));
        }
    };
    let width = read("width_ha").and_then(|w| w.as_float().or(w.as_integer().map(|w| w as f64)));
    let Some(width) = width.filter(|w| w.is_normal() && w.is_sign_positive()) else {
        return Err(Failure::Harness(
            "invariance: a smearing needs a positive occupations.width_ha".into(),
        ));
    };
    let spins = occupations.as_array().map_or(1, Vec::len).max(1) as f64;
    Ok(2.0 / spins * steepest / width)
}

fn turn(value: &Value, rotation: [[f64; 3]; 3]) -> Option<Value> {
    let rows = value.as_array()?.iter().map(|row| {
        let f: Vec<f64> = row
            .as_array()?
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()?;
        let [x, y, z] = f[..] else { return None };
        Some(Value::from(
            rotation.map(|r| r[0] * x + r[1] * y + r[2] * z).to_vec(),
        ))
    });
    rows.collect::<Option<Vec<_>>>().map(Value::from)
}

fn turn_tensor(value: &Value, rotation: [[f64; 3]; 3]) -> Option<Value> {
    let rows = value.as_array()?;
    let t: Vec<Vec<f64>> = (rows.iter())
        .map(|row| row.as_array()?.iter().map(Value::as_f64).collect())
        .collect::<Option<_>>()?;
    if t.len() != 3 || t.iter().any(|row| row.len() != 3) {
        return None;
    }
    let r = rotation;
    let turned: Vec<Vec<f64>> = (0..3)
        .map(|i| {
            (0..3)
                .map(|j| {
                    (0..3)
                        .flat_map(|k| (0..3).map(move |l| (k, l)))
                        .map(|(k, l)| r[i][k] * t[k][l] * r[j][l])
                        .sum()
                })
                .collect()
        })
        .collect();
    Some(Value::from(turned))
}

fn magnitude(value: &Value) -> f64 {
    match value {
        Value::Array(items) => items.iter().map(magnitude).fold(0.0, f64::max),
        Value::Number(n) => n.as_f64().map_or(0.0, f64::abs),
        _ => 0.0,
    }
}

fn difference(a: &Value, b: &Value) -> Option<f64> {
    let pairs = pairs(a, b)?;
    Some(
        pairs
            .into_iter()
            .map(|(p, q)| gap(p, q))
            .fold(0.0, f64::max),
    )
}

fn pairs(a: &Value, b: &Value) -> Option<Vec<(f64, f64)>> {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            let rows = x.iter().zip(y).map(|(p, q)| pairs(p, q));
            Some(rows.collect::<Option<Vec<_>>>()?.concat())
        }
        (Value::Number(p), Value::Number(q)) => Some(vec![(p.as_f64()?, q.as_f64()?)]),
        _ => None,
    }
}

fn gap(p: f64, q: f64) -> f64 {
    let d = (p - q).abs();
    if d.is_nan() { f64::INFINITY } else { d }
}
