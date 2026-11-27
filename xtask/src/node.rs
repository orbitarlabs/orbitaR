use crate::{Findings, bench, harness::rival::Rival};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

const PACKAGES: [&str; 6] = [
    "libblas",
    "libopenblas",
    "fftw",
    "mpich",
    "openmpi",
    "libxc",
];

pub(crate) fn block(root: &Path, rivals: Option<&Path>) -> toml::Table {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let first = |key: &str| {
        (cpuinfo.lines())
            .find(|l| l.split(':').next().is_some_and(|k| k.trim() == key))
            .and_then(|l| l.split_once(':'))
            .map_or_else(|| "unknown".to_owned(), |(_, v)| v.trim().to_owned())
    };
    let cores: BTreeSet<String> = (fs::read_dir("/sys/devices/system/cpu")
        .into_iter()
        .flatten()
        .flatten())
    .filter_map(|cpu| fs::read_to_string(cpu.path().join("topology/core_cpus_list")).ok())
    .map(|list| list.trim().to_owned())
    .collect();
    let file = |path: &str| {
        fs::read_to_string(path).map_or_else(|_| "unknown".to_owned(), |t| t.trim().to_owned())
    };
    let allowed = (fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines())
    .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
    .map_or_else(|| "unknown".to_owned(), |v| v.trim().to_owned());
    let logical = std::thread::available_parallelism().map_or(0, |n| n.get());
    let mut host = toml::Table::new();
    host.insert("cpu".into(), first("model name").into());
    host.insert("cores".into(), (cores.len() as i64).into());
    host.insert(
        "core_cpus".into(),
        toml::Value::Array(cores.into_iter().map(Into::into).collect()),
    );
    host.insert("memory_bytes".into(), memory_bytes());
    host.insert("logical_cpus".into(), (logical as i64).into());
    host.insert("cpus_allowed".into(), allowed.into());
    host.insert(
        "smt".into(),
        file("/sys/devices/system/cpu/smt/control").into(),
    );
    host.insert("kernel".into(), file("/proc/sys/kernel/osrelease").into());
    host.insert("microcode".into(), first("microcode").into());
    let governor = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
    host.insert("governor".into(), file(governor).into());
    let profile = (fs::read_to_string(root.join("Cargo.toml")).ok())
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("profile")?.get("release")?.as_table().cloned())
        .unwrap_or_default();
    let mut orbitar = toml::Table::new();
    orbitar.insert(
        "build".into(),
        format!("cargo {}", bench::BUILD.join(" ")).into(),
    );
    orbitar.insert("profile_release".into(), profile.into());
    let mut block = toml::Table::from_iter([
        ("host".into(), host.into()),
        ("orbitar".into(), orbitar.into()),
    ]);
    for code in Rival::all().iter().filter(|_| rivals.is_some()) {
        let meta = rivals
            .unwrap_or(root)
            .join(code.adapter().name())
            .join("conda-meta");
        let mut table = toml::Table::new();
        for record in fs::read_dir(&meta).into_iter().flatten().flatten() {
            let json = (fs::read_to_string(record.path()).ok())
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .unwrap_or_default();
            let field = |k: &str| json[k].as_str().unwrap_or("unknown").to_owned();
            let name = field("name");
            if name == code.adapter().name() || PACKAGES.contains(&name.as_str()) {
                let pinned = format!(
                    "{} {} sha256:{}",
                    field("version"),
                    field("build"),
                    field("sha256")
                );
                table.insert(name, pinned.into());
            }
        }
        block.insert(code.adapter().name().into(), table.into());
    }
    block
}

pub(crate) fn identity(root: &Path) -> serde_json::Value {
    let mut host = block(root, None)
        .get("host")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    host.remove("memory_bytes");
    let machine = fs::read_to_string("/etc/machine-id").map_or_else(
        |_| "unknown".to_owned(),
        |id| crate::lock::hex(&Sha256::digest(id.trim())),
    );
    let total_gib = (fs::read_to_string("/proc/meminfo")
        .unwrap_or_default()
        .lines())
    .find_map(|l| l.strip_prefix("MemTotal:"))
    .and_then(|v| v.trim().strip_suffix("kB")?.trim().parse::<f64>().ok())
    .map(|kib| (kib / f64::from(1u32 << 20)).round());
    serde_json::json!({
        "host": host,
        "machine_id_sha256": machine,
        "memory_total_gib_rounded": total_gib,
        "cgroup_limits": cgroup_limits(),
    })
}

const CGROUP_LIMITS: [&str; 7] = [
    "memory.max",
    "memory.high",
    "memory.swap.max",
    "cpu.max",
    "cpu.weight",
    "cpuset.cpus.effective",
    "pids.max",
];

fn cgroup_limits() -> serde_json::Value {
    let own = (fs::read_to_string("/proc/self/cgroup")
        .unwrap_or_default()
        .lines())
    .find_map(|l| l.strip_prefix("0::").map(str::to_owned))
    .unwrap_or_default();
    let uid = (fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines())
    .find_map(|l| {
        l.strip_prefix("Uid:")?
            .split_whitespace()
            .next()
            .map(str::to_owned)
    })
    .unwrap_or_default();
    let user = format!("/user.slice/user-{uid}.slice/user@{uid}.service");
    let chain = |group: &str| -> serde_json::Value {
        (CGROUP_LIMITS.iter())
            .map(|file| {
                let values: Vec<String> = (Path::new(group).ancestors())
                    .filter_map(|dir| {
                        let at =
                            Path::new("/sys/fs/cgroup").join(dir.strip_prefix("/").unwrap_or(dir));
                        fs::read_to_string(at.join(file)).ok()
                    })
                    .map(|v| v.trim().to_owned())
                    .collect();
                ((*file).to_owned(), serde_json::json!(values))
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    serde_json::json!({ "harness": chain(&own), "user_service": chain(&user) })
}

fn memory_bytes() -> toml::Value {
    let total = (fs::read_to_string("/proc/meminfo")
        .unwrap_or_default()
        .lines())
    .find_map(|l| l.strip_prefix("MemTotal:"))
    .and_then(|v| v.trim().strip_suffix("kB")?.trim().parse::<i64>().ok());
    let Some(total) = total else {
        return "unknown".into();
    };
    let group = (fs::read_to_string("/proc/self/cgroup")
        .unwrap_or_default()
        .lines())
    .find_map(|l| l.strip_prefix("0::").map(str::to_owned))
    .unwrap_or_default();
    (Path::new(&group).ancestors())
        .filter_map(|dir| {
            let file = Path::new("/sys/fs/cgroup").join(dir.strip_prefix("/").unwrap_or(dir));
            fs::read_to_string(file.join("memory.max"))
                .ok()?
                .trim()
                .parse::<i64>()
                .ok()
        })
        .fold(total * 1024, i64::min)
        .into()
}

fn cpus(list: &str) -> Option<BTreeSet<u32>> {
    let mut set = BTreeSet::new();
    for part in list.trim().split(',') {
        let (low, high) = part.split_once('-').unwrap_or((part, part));
        set.extend(low.parse::<u32>().ok()?..=high.parse::<u32>().ok()?);
    }
    Some(set)
}

pub(crate) const PATH: &str = "benchmarks/NODE.toml";

const MEMORY_JITTER: u64 = 64 << 20;

pub(crate) fn write(root: &Path, rivals: Option<&Path>) -> Findings {
    let path = root.join(PATH);
    let mut node = match fs::read_to_string(&path) {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(node) => node,
            Err(e) => return vec![format!("{PATH}: {e}")],
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return vec![format!("{PATH}: {e}")],
    };
    node.extend(block(root, rivals));
    let text = match toml::to_string(&node) {
        Ok(text) => text,
        Err(e) => return vec![format!("{PATH}: {e}")],
    };
    match fs::write(&path, &text) {
        Ok(()) => {
            print!("{text}");
            Vec::new()
        }
        Err(e) => vec![format!("{PATH}: {e}")],
    }
}

pub(crate) fn declared(root: &Path) -> Result<toml::Table, String> {
    match fs::read_to_string(root.join(PATH)) {
        Ok(text) => text.parse().map_err(|e| format!("claim: {PATH}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("claim: {PATH} is missing; declare the node first"))
        }
        Err(e) => Err(format!("claim: {PATH}: {e}")),
    }
}

pub(crate) fn check(
    root: &Path,
    declared: &toml::Table,
    rivals: Option<&Path>,
) -> Result<(), String> {
    let here = block(root, rivals);
    let tables: Vec<&str> = here.keys().map(String::as_str).collect();
    let differ = differences(&here, declared, &tables);
    match differ.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "claim: this host is not the declared node: {}; on the node, run `cargo xtask node`",
            differ.join("; ")
        )),
    }
}

pub(crate) fn differences(
    here: &toml::Table,
    declared: &toml::Table,
    tables: &[&str],
) -> Vec<String> {
    let mut differ = Vec::new();
    for &table in tables {
        let of = |t: &toml::Table| {
            (t.get(table).and_then(toml::Value::as_table))
                .cloned()
                .unwrap_or_default()
        };
        let (ours, theirs) = (of(here), of(declared));
        let names: BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
        for key in names {
            let (value, shown) = (ours.get(key), theirs.get(key));
            let int = |v: Option<&toml::Value>| v.and_then(toml::Value::as_integer);
            let apart = match (table, key.as_str(), int(value), int(shown)) {
                ("host", "memory_bytes", Some(a), Some(b)) => Some(a.abs_diff(b)),
                _ => None,
            };
            if apart.map_or(value != shown, |d| d > MEMORY_JITTER) {
                let text = |v: Option<&toml::Value>| {
                    v.map_or_else(|| "nothing".to_owned(), ToString::to_string)
                };
                let beyond = apart.map_or_else(String::new, |d| {
                    format!(" ({d} B apart, more than {MEMORY_JITTER} B)")
                });
                differ.push(format!(
                    "{table}.{key} is {} here, {} in NODE.toml{beyond}",
                    text(value),
                    text(shown)
                ));
            }
        }
    }
    differ
}

fn cpulist(key: &str, value: Option<&toml::Value>) -> Result<BTreeSet<u32>, String> {
    (value.and_then(toml::Value::as_str))
        .and_then(cpus)
        .ok_or_else(|| format!("claim: NODE.toml host.{key} holds no cpulist: {value:?}"))
}

fn host<'a>(declared: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    declared.get("host").and_then(|h| h.get(key))
}

pub(crate) fn allowed_cpus(declared: &toml::Table) -> Result<usize, String> {
    cpulist("cpus_allowed", host(declared, "cpus_allowed")).map(|allowed| allowed.len())
}

pub(crate) fn fits(declared: &toml::Table, threads: usize) -> Result<(), String> {
    let allowed = cpulist("cpus_allowed", host(declared, "cpus_allowed"))?;
    let mut cores = 0;
    for core in (host(declared, "core_cpus").and_then(toml::Value::as_array))
        .into_iter()
        .flatten()
    {
        cores += usize::from(!cpulist("core_cpus", Some(core))?.is_disjoint(&allowed));
    }
    let logical = allowed.len();
    if cores.min(logical) >= threads {
        Ok(())
    } else {
        Err(format!(
            "claim: the sweep runs {threads} threads, but NODE.toml allows {cores} cores and {logical} logical CPUs"
        ))
    }
}
