use std::process::ExitCode;

fn main() -> ExitCode {
    let raw: Vec<_> = std::env::args_os().skip(1).collect();
    if let [first, rest @ ..] = &raw[..]
        && first == xtask::harness::SHIM
    {
        return xtask::harness::shim(rest);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let name = words.first().copied().unwrap_or("help");
    let findings = match xtask::root() {
        Ok(root) => xtask::run(&words, &root, &xtask::Env::from_process()),
        Err(e) => vec![e],
    };
    if findings.is_empty() {
        println!("xtask {name}: PASS");
        ExitCode::SUCCESS
    } else {
        findings.iter().for_each(|f| eprintln!("FAIL {f}"));
        eprintln!("xtask {name}: {} finding(s)", findings.len());
        ExitCode::FAILURE
    }
}
