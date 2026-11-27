# Contributing to orbitaR

## Start

- Run `cargo xtask help`. It prints the commands, the layout, the layer order, and the tiers.
- Follow the work loop in AGENTS.md.

## Add a case

```text
cargo xtask new-case <slug> <reference>
  benchmarks/<N>_<slug>/case.toml        status planned · tier push, or claim for vasp-measured · empty [tolerance]
  tests/benchmarks/<N>_<slug>/case.rs    a measure() stub that returns Err
  tests/benchmarks.rs                    the case table, generated again
<N>                                      the highest case number in benchmarks/ or benchmarks/LOCK, plus one
```

```text
you write
  benchmarks/<N>_<slug>/input.toml       the engine input; more inputs go in inputs/<stem>.toml
  benchmarks/<N>_<slug>/case.toml        [tolerance]: <metric>_max or <metric>_min; the harness judges, not measure()
  tests/benchmarks/<N>_<slug>/case.rs    measure(): the metrics of the engine output
  benchmarks/<N>_<slug>/reference/       the reference files, with SOURCE.toml
```

- `reference/SOURCE.toml` names the kind, citation, locator, convention, and license of the reference.
- `cargo xtask science` checks SOURCE.toml (S8) and LOCK (S5).
- `cargo xtask lock --reviewer <name> --reason <text>` hashes the case into `benchmarks/LOCK`.
- A changed hash in `benchmarks/LOCK` needs a full review: see `## Pull requests`.

## Pull requests

- Each pull request passes `cargo xtask check`, `smoke`, `science --base origin/main`, and `docs --base origin/main`.
- Each pull request passes `cargo xtask bench pr` too, as CI runs it.
- A full review covers each change under `src/` or `benchmarks/` and each changed hash in `benchmarks/LOCK`.
- A full review also covers each change to a claim or to how a result is judged, wherever it lies.
- A change in doubt, or one that overlaps a full review, takes a full review.
- In a full review, a reviewer reads the diff and the command output. The reviewer is not the author.
- The pull request records each reviewer who agrees. A LOCK change names its reviewer and its reason.
- The commands and CI judge every other change.
- No pull request text or commit message holds a generated-by line or a co-author trailer: `cargo xtask pr-text`.
- A commit may have any author but Claude. Its message names no one and no home directory: `cargo xtask publish`.

## Cases against a rival code

- orbitaR and the rival meet one accuracy target. Each timed run reports its own accuracy.
- A failed run counts as a loss.
- A held-out variant stays outside the repository until the `cargo xtask bench claim` run.

## License

- LICENSE holds the terms of the repository: PolyForm Noncommercial 1.0.0.
- A merged change becomes part of the software that LICENSE covers.
