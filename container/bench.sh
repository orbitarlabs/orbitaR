#!/bin/sh
set -eu
[ -d /work/orbitar ] || cp -a /opt/orbitar /work/orbitar
[ -d /work/cargo-home ] || cp -a /opt/cargo-home /work/cargo-home
export CARGO_HOME=/work/cargo-home
cd /work/orbitar
sha256sum --check --quiet /opt/engine.sha256
exec cargo xtask "$@"
