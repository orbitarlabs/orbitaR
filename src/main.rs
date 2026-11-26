use orbitar::io::{Family, run_file, serve};
use orbitar::ks::Failure;
use orbitar::model::DEFAULT_FAMILY;
use std::path::Path;
use std::process::ExitCode;

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const USAGE: &str = "usage: orbitar run <input.toml> | orbitar ipi --unix <socket> <input.toml> \
                     | orbitar datasets --directory <dir> [--family <name>] | orbitar --version";

fn main() -> ExitCode {
    #[expect(clippy::disallowed_methods, reason = "the CLI reads its arguments")]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "run" => run_file(path, |step, free_ha| {
            eprintln!("scf_step {step} free_energy_ha {free_ha}");
        }),
        [command, flag, socket, path] if command == "ipi" && flag == "--unix" => {
            serve(path, socket, |line| println!("{line}")).map(|()| String::new())
        }
        [command, flag, directory, rest @ ..] if command == "datasets" && flag == "--directory" => {
            match rest {
                [] => Ok(DEFAULT_FAMILY),
                [flag, name] if flag == "--family" => Ok(name.as_str()),
                _ => Err(Failure::Invalid(USAGE.into())),
            }
            .and_then(|name| Family::read(Path::new(directory), name).map_err(Failure::Invalid))
            .map(|family| family.json())
        }
        [flag] if flag == "--version" => Ok(version()),
        _ => Err(Failure::Invalid(USAGE.into())),
    };
    match result {
        Ok(json) => {
            if !json.is_empty() {
                println!("{json}");
            }
            ExitCode::SUCCESS
        }
        Err(Failure::Invalid(message)) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
        Err(Failure::Unsupported(message)) => {
            eprintln!("{message}");
            ExitCode::from(3)
        }
        Err(Failure::Unconverged(message)) => {
            eprintln!("{message}");
            ExitCode::from(4)
        }
    }
}

#[expect(
    clippy::disallowed_macros,
    reason = "the version and the commit are build-time facts that only the CLI prints"
)]
fn version() -> String {
    let commit = option_env!("ORBITAR_COMMIT").unwrap_or("unknown");
    format!("orbitar {} (commit {commit})", env!("CARGO_PKG_VERSION"))
}
