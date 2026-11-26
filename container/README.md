# orbitaR container

- This page builds the wheel and both images from a checkout, runs them with Docker, and converts them to Apptainer.
- No registry holds an orbitaR image. Build it here, or move a saved tar.
- `untested` marks each Apptainer step: the repository's checks run Docker, but no Apptainer.

## Prerequisites

```text
docker      with BuildKit (Docker 23 or later)        docker buildx version
apptainer   1.2 or later                              apptainer --version
git, cargo  this checkout at a clean commit           git status --porcelain   (prints nothing)
network     quay.io, ghcr.io, Docker Hub, PyPI, crates.io, static.rust-lang.org, raw.githubusercontent.com
```

## Build

```sh
cargo xtask wheel   # ends `xtask wheel: PASS`; it prints `wheel: <the path of the wheel>`
cargo xtask image   # ends `xtask image: PASS`; it prints `image bench: engine <sha256>  <the engine path>`
# both images: user 1000, no fetched dataset, no rival code, no .git, THIRD-PARTY-NOTICES, no account in docker history
# bench holds the commit's tree, so datasets/orbitaR-PBE-v0/ (O.xml and its input O.in): GPL-3.0, its LICENSE, NOTICE
# its PAW-XML proof: a root in each file's first 64 KiB, after comments and <?…?> only; a file it cannot read fails it
docker image ls orbitar-local   # two tags: runtime, bench
```

## Run with Docker

```sh
# from the checkout: /work is the checkout, writable; the image stays read-only
docker run --rm --read-only --user 1000 --volume "$PWD:/work" orbitar-local:runtime orbitar --version
# orbitar <version> (commit <hash>)
docker run --rm --read-only --user 1000 --volume "$PWD:/work" orbitar-local:runtime \
  orbitar run benchmarks/1_free_electron/input.toml
# one JSON line: "basis_sizes", "eigenvalues_ha", "kpoint_weights", "kpoints_fractional"
docker run --rm --read-only --user 1000 --entrypoint cat orbitar-local:bench /opt/engine.sha256
# <sha256>  <the engine path>
```

## Convert to Apptainer

```sh
# untested: each tar stays on this machine until you move it
docker save orbitar-local:runtime --output orbitar-runtime.tar
docker save orbitar-local:bench --output orbitar-bench.tar
apptainer build orbitar-runtime.sif docker-archive://orbitar-runtime.tar
apptainer build orbitar-bench.sif docker-archive://orbitar-bench.tar
```

## Run with Apptainer

```sh
# untested: the image is read-only; the bound directory is the one writable directory
apptainer exec --bind "$PWD:/work" --pwd /work orbitar-runtime.sif orbitar run benchmarks/1_free_electron/input.toml
# the same JSON line as with Docker
mkdir -p "$HOME/orbitar-work"
apptainer run --bind "$HOME/orbitar-work:/work" --env ORBITAR_CASE_FILTER=Al-X/FCC@1.00 orbitar-bench.sif bench node
# untested: case 10 on one Al volume; it fetches the pinned datasets and runs the prebuilt engine
# the records land in $HOME/orbitar-work/orbitar/target/benchmarks/node
```

## Limits of bench

- The bench image holds a `git archive` snapshot of one commit: no `.git`, no history.
- The harness stamps `commit` and `dirty` from git, so a bench record reads `commit unknown`, `dirty true`.
- `cargo xtask shards` refuses such a record. A bench run of the image checks the image; it records no commit.

## Report a failure in an issue

```text
engine   the `/opt/engine.sha256` line
failure  the step that failed, and the last 40 lines of its output
```
