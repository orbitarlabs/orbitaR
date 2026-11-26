# orbitaR agent guide

## Commands

```sh
cargo xtask help        # every command, the layout, the layer order, the tiers
cargo xtask check       # fmt, clippy, the tests, the build, the package, cargo deny, every lever
cargo xtask smoke       # the push tier of the benchmarks: the proof that a change works
cargo xtask science --base origin/main   # the science rules on the change
cargo xtask docs        # the doc rules
cargo test --workspace --all-features --locked   # the tests alone
```

## Work loop

```text
feature  cargo xtask new-case <slug> <reference> → code → cargo xtask check, smoke → pull request (CONTRIBUTING.md)
bug      a failing test → the fix
status   planned ─passes─▶ active ─▶ retired
```

- CONTRIBUTING.md lists what each pull request must pass.
