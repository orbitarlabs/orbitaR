pub mod harness;

pub use release::mac::{
    Build, Built, Installed, MATURIN, PYTHON, Steps, about_config, about_pin, about_plan,
    ase_verdict, c_flags, data_plan, floor_plan, maturin_pin, maturin_plan, maturin_requirement,
    metadata_plan, one_wheel, pair_verdict, proof, python_plan, readme_plan, remaps, tools_plan,
    tracked,
};
pub use release::{
    Image, Install, Invocation, Machine, PAW_SCAN, PAW_SCAN_BYTES, Program, Wheels, baked, clear,
    host_paths, image_verdict, in_venv, install, install_plan, install_verdict, machine,
    readme_lines,
};
pub use reproduce::stamp;

mod atompaw;
mod bench;
mod case;
mod docs;
mod family;
mod layers;
mod layout;
mod lock;
mod manifest;
mod native;
mod node;
mod pr_text;
mod publish;
mod release;
mod reproduce;
mod results;
mod same;
mod science;
mod strain;
mod syntax;
mod tune;
mod version;
mod walk;

use clap::{CommandFactory, Parser};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

pub type Findings = Vec<String>;

#[derive(Clone, Debug)]
pub struct Env {
    pub ci: bool,
    pub cargo: OsString,
    pub rustflags: Option<String>,
    pub runner: Option<String>,
    pub rivals: Option<PathBuf>,
    pub case_filter: bool,
    pub atompaw: Option<PathBuf>,
    pub path: Option<OsString>,
}

impl Env {
    pub fn from_process() -> Env {
        Env {
            ci: std::env::var("CI").as_deref() == Ok("true"),
            cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
            rustflags: (["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"].into_iter())
                .find(|var| std::env::var_os(var).is_some())
                .map(str::to_owned),
            runner: std::env::var("RUNNER_ENVIRONMENT").ok(),
            rivals: std::env::var_os("ORBITAR_RIVALS").map(PathBuf::from),
            case_filter: std::env::var_os("ORBITAR_CASE_FILTER").is_some(),
            atompaw: std::env::var_os("ORBITAR_ATOMPAW").map(PathBuf::from),
            path: std::env::var_os("PATH"),
        }
    }
}

/// `cargo xtask <command>`. The doc comments are the command documentation; docs may name only these.
#[derive(Parser)]
#[command(
    name = "cargo xtask",
    disable_help_subcommand = true,
    disable_help_flag = true
)]
enum Cli {
    /// Print the commands, the layout, the layer order, and the tier policy
    Help,
    /// The CHECK cargo steps, then the levers layout, layers, manifest, docs, science, native, version, and the
    /// file rules of publish; `cargo xtask publish --base <rev> --head <rev>` reads the commits of a range
    Check,
    /// Prove It Works: build the release engine, run the push cases, check their records
    Smoke,
    /// bench <tier>: the same for one tier; `cargo xtask help` lists the tiers
    Bench { tier: String },
    /// new-case <slug> <reference>: scaffold the next <N>_<slug>; regenerate the case table
    NewCase { slug: String, reference: String },
    /// lock --reviewer <name> --reason <text>: hash into benchmarks/LOCK; each changed block gets a new review line
    Lock {
        #[arg(long)]
        reviewer: String,
        #[arg(long)]
        reason: String,
    },
    /// wheel [--check <file>] [--target <Mac>]: build orbitar's and orbitar-jth's wheels (in manylinux_2_28 or on a Mac), check each, install both, run README
    Wheel {
        #[arg(long)]
        check: Option<PathBuf>,
        #[arg(long)]
        target: Option<String>,
    },
    /// image [--scan <file>]: build the runtime and bench images; each runs case 1's input as user 1000, read-only
    Image {
        #[arg(long)]
        scan: Option<PathBuf>,
    },
    /// reproduce [--check <file>]: two clean engine builds give one sha256; a binary embeds only the fixed build date
    Reproduce {
        #[arg(long)]
        check: Option<PathBuf>,
    },
    /// Mirrors, module declarations, no inline tests, layer roots, the generated case table
    Layout,
    /// Each src/ layer uses only layers before it in ORDER; format crates stay in src/io/
    Layers,
    /// Exact pins in every dependency table, members, no build script, one version per tool
    Manifest,
    /// Prose files, symlinks, line kinds and length, sentences, voice, terms, paths, commands, README proofs, a linked private method
    /// With --base <rev>, also check added code comments
    Docs {
        #[arg(long)]
        base: Option<String>,
    },
    /// S1–S8 and S10 anti-cheat scans; science --base <rev> adds S9 against a base revision
    Science {
        #[arg(long)]
        base: Option<String>,
    },
    /// publish [--base <rev>] [--head <rev>]: no workflow, container file, or packaging lever publishes; no workflow
    /// asks for more than contents: read; no file names a person, an account, a private link, or a home directory;
    /// each commit of <base>..<head> (origin/main..HEAD) has well-formed identity lines (git fsck --strict), neither
    /// the Claude name nor address, no half of the maintainer or GitHub identity, and a message, merges too, that
    /// names no one and no home directory
    Publish {
        #[arg(long, default_value = publish::BASE)]
        base: String,
        #[arg(long, default_value = publish::HEAD)]
        head: String,
    },
    /// pr-text [--body <file>] --base <rev> [--head <rev>]: the pull request text and each commit message of
    /// <base>..<head> (head: HEAD) hold no tool's generated-by line and no co-author trailer
    PrText {
        #[arg(long)]
        body: Option<PathBuf>,
        #[arg(long)]
        base: String,
        #[arg(long, default_value = "HEAD")]
        head: String,
    },
    /// Cargo.toml's version equals origin/main's or its patch + 1; xtask/version-steps.toml names any other step;
    /// CITATION.cff cites Cargo.toml's version and license
    Version,
    /// A local manifest names a provider, proof, and the options tried for every dependency and code file
    Native,
    /// same --base <rev>: build the engine at <rev> and here. Name each push and pr input whose stdout bytes differ
    ///
    /// Each input runs at 1 and at max(N, 2) threads, N the cores here. --time <case>/<input> times those inputs
    /// instead, in cold runs per engine in alternating order, step energies on: wall time and T(δ). It writes
    /// target/same/time.log.
    Same {
        #[arg(long)]
        base: String,
        #[arg(long)]
        time: Vec<String>,
        /// Time at the cutoff ABINIT froze in benchmarks/TUNED.toml as it stands, the claim's cutoff for orbitaR; each
        /// run prints a `same: run` line as it ends (a gaas timing there takes about 50 minutes)
        #[arg(long, requires = "time")]
        claim_cutoff: bool,
    },
    /// records <tier>: print the last records of a tier
    Records { tier: String },
    /// results collect [--tier <tier> …] [--records <dir>] | results render: benchmarks/RESULTS.toml, its page and chart
    ///
    /// collect reads <dir>/<tier>/*.json (default target/benchmarks; every tier there when none is named) and writes
    /// a row per active case: what it checks and its headline metric (benchmarks/results-what.toml), each bound's
    /// measured value and ratio, the outcome, the record's commit and engine. It refuses a dirty record or one whose
    /// commit is no ancestor of HEAD. render writes benchmarks/README.md and assets/benchmarks/results-{light,dark}.svg from RESULTS.toml alone (results.rs).
    Results {
        #[command(subcommand)]
        step: Step,
    },
    /// strain <centre.toml> --direction K --h h [--stem s]: write a stress triple's <stem>-plus and -minus inputs
    ///
    /// K is 9 numbers, row-major and symmetric; each end takes the cell A (1 ± h K / ‖K‖), the rest of the centre,
    /// and no [outputs] (strain.rs).
    Strain {
        centre: PathBuf,
        #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
        direction: Vec<f64>,
        #[arg(long)]
        h: f64,
        #[arg(long)]
        stem: Option<String>,
    },
    /// family <new-name> --from <table.toml>: write datasets/<new-name>.toml from a tuning table
    ///
    /// Each own row takes the sha256 of its file and keeps its hint; base "jth-2.0" gives every other element. A
    /// name that exists is refused (family.rs).
    Family {
        name: String,
        #[arg(long)]
        from: PathBuf,
    },
    /// readout <case> [--dir <records>]: print R, G, each pair, and each block of a claim record (default: the claim tier's)
    Readout {
        case: String,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// shards <case> <dir>: the records in <dir> cover each input of the case once, at one commit and engine
    ///
    /// A node-tier run with ORBITAR_CASE_FILTER=<selector>,… writes a shard (harness.rs `selected`).
    Shards { case: String, dir: PathBuf },
    /// epsilon <case> <dir> [--baseline <dir>] [selector …]: per tuning crystal, ε and ν against the all-electron average
    ///
    /// Each crystal shows VASP's ε and the baseline records' ε; each element and the set show the median ε,
    /// share(ε ≤ 0.06) and the count above 1.0. A crystal without ε is lost and counts as +∞. An exam system stops it.
    /// A node-tier run with ORBITAR_DATASET_TABLE=<table.toml> runs a candidate dataset table (datasets.rs `Table`).
    Epsilon {
        case: String,
        dir: PathBuf,
        #[arg(long)]
        baseline: Option<PathBuf>,
        selectors: Vec<String>,
    },
    /// On the claim node: write its [host] and [orbitar] blocks, and with the rivals [qe] and [abinit], into benchmarks/NODE.toml
    Node,
    /// rivals <job> [code …] [--cell <path> …]: on the claim node, freeze each rival's cutoff, mixing, or layout into benchmarks/TUNED.toml
    Rivals {
        job: tune::Job,
        /// The rivals to tune; every one when none is named. The others keep what TUNED.toml holds.
        #[arg(value_enum)]
        codes: Vec<harness::rival::Rival>,
        /// calibrate only: the calibration cells to run; the others keep their TUNED.toml rows.
        #[arg(long)]
        cell: Vec<String>,
    },
    /// atompaw [<family>[/<element>] …] [--install <dir>] [--write]: build each datasets/<family>/<element>.in twice
    /// with the pinned ATOMPAW; --write publishes element by element
    ///
    /// Both builds give the same bytes; each number is within REBUILD_REL of the committed
    /// datasets/<family>/<element>.xml, which its family table datasets/<family>.toml pins; the log derivatives match
    /// the all-electron atom with no ghost state; target/atompaw/<family>/<element>.json records the numbers.
    /// --install <dir> first creates the pinned install there with micromamba; else ORBITAR_ATOMPAW names it
    /// (atompaw.rs).
    Atompaw {
        inputs: Vec<String>,
        #[arg(long)]
        install: Option<PathBuf>,
        /// Write each built file to datasets/<family>/<element>.xml and print its sha256 for the tuning table, in
        /// place of the numeric check against the committed file. Elements publish one by one: a later element's
        /// failure leaves the files written before it.
        #[arg(long)]
        write: bool,
    },
}

#[derive(clap::Subcommand)]
enum Step {
    Collect {
        #[arg(long)]
        tier: Vec<String>,
        #[arg(long)]
        records: Option<PathBuf>,
    },
    Render,
}

pub fn commands() -> Vec<(String, String)> {
    Cli::command()
        .get_subcommands()
        .map(|c| {
            let about = c.get_about().map(ToString::to_string).unwrap_or_default();
            (c.get_name().to_owned(), about)
        })
        .collect()
}

pub const CHECK: [&[&str]; 5] = [
    &["fmt", "--all", "--", "--check"],
    &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--",
        "-D",
        "warnings",
    ],
    &["test", "--workspace", "--all-features", "--locked"],
    &["build", "--workspace", "--all-features", "--locked"],
    &["package", "--locked"],
];

pub(crate) const DENY_VERSION: &str = "0.20.2";
pub(crate) const DENY_ACTION: &str =
    "EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25";

type Lever = fn(&Path) -> Findings;

const LEVERS: [Lever; 8] = [
    layout::check,
    layers::check,
    manifest::check,
    docs::check,
    |root| science::check(root, None),
    native::check,
    version::check,
    publish::tree,
];

pub fn run(args: &[&str], root: &Path, env: &Env) -> Findings {
    if args.is_empty() {
        return help();
    }
    let command = match Cli::try_parse_from(std::iter::once("xtask").chain(args.iter().copied())) {
        Ok(command) => command,
        Err(e) => {
            let text = e.to_string();
            let mut lines = text.lines();
            let first = lines.next().unwrap_or("bad arguments");
            let values = lines.find_map(|l| l.trim().strip_prefix("[possible values: "));
            let values = values.map_or(String::new(), |v| {
                format!(" (one of {}", v.replace(']', ")"))
            });
            return vec![format!("{first}{values}; run `cargo xtask help`")];
        }
    };
    match command {
        Cli::Help => help(),
        Cli::Check => check(root, env),
        Cli::Smoke => bench::tier(root, "push", env),
        Cli::Bench { tier } => bench::tier(root, &tier, env),
        Cli::NewCase { slug, reference } => layout::new_case(root, &slug, &reference),
        Cli::Lock { reviewer, reason } => lock::write(root, &reviewer, &reason),
        Cli::Wheel { check, target } => {
            release::wheel(root, check.as_deref(), target.as_deref(), env)
        }
        Cli::Image { scan } => release::image(root, scan.as_deref()),
        Cli::Reproduce { check } => reproduce::check(root, check.as_deref(), env),
        Cli::Layout => layout::check(root),
        Cli::Layers => layers::check(root),
        Cli::Manifest => manifest::check(root),
        Cli::Docs { base } => {
            let mut findings = docs::check(root);
            if let Some(base) = base {
                findings.extend(docs::added_comments(root, &base));
            }
            findings
        }
        Cli::Science { base } => science::check(root, base.as_deref()),
        Cli::Native => native::check(root),
        Cli::Version => version::check(root),
        Cli::Publish { base, head } => publish::check(root, &base, &head),
        Cli::PrText { body, base, head } => pr_text::check(root, body.as_deref(), &base, &head),
        Cli::Same {
            base,
            time,
            claim_cutoff,
        } => same::check(root, &base, &time, claim_cutoff, env),
        Cli::Records { tier } => bench::records(root, &tier),
        Cli::Results {
            step: Step::Collect { tier, records },
        } => results::collect(root, &tier, records.as_deref()),
        Cli::Results { step: Step::Render } => results::render(root),
        Cli::Strain {
            centre,
            direction,
            h,
            stem,
        } => strain::write(root, &centre, &direction, h, stem.as_deref()),
        Cli::Family { name, from } => family::write(root, &name, &from),
        Cli::Readout { case, dir } => harness::readout(
            &dir.unwrap_or_else(|| bench::records_dir(root, case::Tier::Claim)),
            &case,
        ),
        Cli::Shards { case, dir } => harness::union(root, &case, &dir),
        Cli::Epsilon {
            case,
            dir,
            baseline,
            selectors,
        } => match harness::acwf::readout(root, &case, &dir, baseline.as_deref(), &selectors) {
            Ok(text) => {
                print!("{text}");
                Vec::new()
            }
            Err(e) => vec![format!("epsilon: {e}")],
        },
        Cli::Node => node::write(root, env.rivals.as_deref()),
        Cli::Rivals { job, codes, cell } => tune::run(root, job, &codes, &cell, env),
        Cli::Atompaw {
            inputs,
            install,
            write,
        } => atompaw::run(
            root,
            install.as_deref(),
            &inputs,
            env.atompaw.as_deref(),
            env.path.as_deref(),
            write,
        ),
    }
}

pub fn root() -> Result<PathBuf, String> {
    let built = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(here) = std::env::var_os("CARGO_MANIFEST_DIR")
        && Path::new(&here) != built
    {
        return Err(format!(
            "this xtask binary was built in {} but runs for {}; run `cargo clean -p xtask`",
            built.display(),
            Path::new(&here).display()
        ));
    }
    built
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live inside the workspace".into())
}

fn help() -> Findings {
    for (name, rule) in commands() {
        println!("cargo xtask {name:<9} {rule}");
    }
    println!("\ncheck runs:");
    for step in CHECK {
        println!("  cargo {}", step.join(" "));
    }
    println!("  (locally the package step adds --allow-dirty; with CI=true it does not)");
    println!("  cargo deny check   (cargo-deny {DENY_VERSION}; CI uses the pinned action)");
    println!("\nlayout:\n{}", layout::MAP);
    println!("\nlayer order (a layer uses only layers before it):");
    println!("  {}", layers::ORDER.join(" < "));
    println!("\ntier     includes            budget_s  repeats  timeout_s  cases  threads");
    for tier in case::Tier::ALL {
        let p = tier.policy();
        let includes: Vec<&str> = case::Tier::ALL
            .iter()
            .filter(|t| tier.includes(**t))
            .map(|t| t.name())
            .collect();
        let budget = p
            .budget_s
            .map_or_else(|| "none".to_owned(), |s| s.to_string());
        println!(
            "{:<8} {:<19} {budget:<9} {:<8} {:<10} {:<6} {}",
            tier.name(),
            includes.join(" "),
            p.repeats,
            p.timeout_s,
            if p.one_case_at_a_time { "1" } else { "all" },
            p.speed.map_or_else(
                || "input".to_owned(),
                |s| (s.threads.iter().map(ToString::to_string))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        );
    }
    Vec::new()
}

fn check(root: &Path, env: &Env) -> Findings {
    let mut findings = Vec::new();
    for step in CHECK {
        let mut step = step.to_vec();
        if step[0] == "package" && !env.ci {
            step.push("--allow-dirty");
        }
        findings.extend(cargo(env, root, &step, &[]));
    }
    findings.extend(package_list(root, env));
    if !env.ci {
        findings.extend(deny_version(root, env));
        findings.extend(
            cargo(env, root, &["deny", "check"], &[])
                .into_iter()
                .map(|f| {
                    format!(
                        "{f} (install: cargo install cargo-deny --version {DENY_VERSION} --locked)"
                    )
                }),
        );
    }
    for lever in LEVERS {
        findings.extend(lever(root));
    }
    findings
}

fn deny_version(root: &Path, env: &Env) -> Findings {
    let install = format!("cargo install cargo-deny --version {DENY_VERSION} --locked");
    match Command::new(&env.cargo)
        .args(["deny", "--version"])
        .current_dir(root)
        .output()
    {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if text.split_whitespace().nth(1) == Some(DENY_VERSION) {
                Vec::new()
            } else {
                vec![format!(
                    "`{text}` is installed; check expects cargo-deny {DENY_VERSION} ({install})"
                )]
            }
        }
        _ => vec![format!("cargo-deny is missing ({install})")],
    }
}

fn package_list(root: &Path, env: &Env) -> Findings {
    match Command::new(&env.cargo)
        .args(["package", "--list", "--locked"])
        .args((!env.ci).then_some("--allow-dirty"))
        .current_dir(root)
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with("scratch/") || l.contains("/scratch/"))
            .map(|l| format!("package: {l} is private scratch; keep it out of the package"))
            .collect(),
        Ok(out) => vec![format!("cargo package --list exited with {}", out.status)],
        Err(e) => vec![format!("cargo package --list: {e}")],
    }
}

pub(crate) fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_owned();
    if out.status.success() {
        Ok(text(&out.stdout))
    } else {
        Err(text(&out.stderr))
    }
}

pub(crate) struct Commit {
    pub(crate) short: String,
    raw: String,
}

impl Commit {
    pub(crate) fn object(&self) -> &str {
        &self.raw
    }

    pub(crate) fn headers_and_message(&self) -> (&str, &str) {
        self.raw.split_once("\n\n").unwrap_or((&self.raw, ""))
    }
}

pub(crate) fn commits(root: &Path, range: &str) -> Result<Vec<Commit>, String> {
    let ids = git(root, &["log", "--format=%H %h", range])?;
    (ids.lines())
        .map(|line| {
            let (id, short) =
                (line.split_once(' ')).ok_or_else(|| format!("git log printed `{line}`"))?;
            let raw = git(root, &["cat-file", "commit", id])?;
            Ok(Commit {
                short: short.to_owned(),
                raw,
            })
        })
        .collect()
}

pub(crate) fn cargo(env: &Env, root: &Path, args: &[&str], vars: &[(&str, OsString)]) -> Findings {
    let mut command = Command::new(&env.cargo);
    command
        .args(args)
        .current_dir(root)
        .env_remove("ORBITAR_SEED");
    for (key, value) in vars {
        command.env(key, value);
    }
    match command.status() {
        Ok(s) if s.success() => Vec::new(),
        Ok(s) => vec![format!("cargo {} exited with {s}", args.join(" "))],
        Err(e) => vec![format!("cargo {}: {e}", args.join(" "))],
    }
}
