ARG COMMIT=unknown

FROM quay.io/pypa/manylinux_2_28_x86_64@sha256:d086f64cddcace250ef0f0073583c51b8298159303290a730c9f59bd91edfd2c AS toolchain
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:/opt/python/cp312-cp312/bin:$PATH
ENV SOURCE_DATE_EPOCH=1767225600
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    --mount=type=bind,source=rust-toolchain.toml,target=/tmp/rust-toolchain.toml \
    /with-ca sh -c 'curl -sSfL -o /tmp/rustup-init \
      https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init \
 && echo "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal \
      --default-toolchain "$(sed -n "s/^channel = \"\(.*\)\"$/\1/p" /tmp/rust-toolchain.toml)" \
 && rm /tmp/rustup-init'

FROM toolchain AS about
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    /with-ca cargo install --locked --version 0.9.3 --features cli --root /opt/about cargo-about

FROM toolchain AS wheel-build
ARG COMMIT
RUN --mount=type=bind,source=.,target=/context \
    mkdir /src \
 && git -c safe.directory='*' -C /context ls-files -z > /tmp/tracked \
 && tar -C /context --null -T /tmp/tracked -cf /tmp/tracked.tar && tar -xf /tmp/tracked.tar -C /src \
 && rm /tmp/tracked /tmp/tracked.tar
WORKDIR /src
COPY --from=about /opt/about/bin/cargo-about /usr/local/bin/cargo-about
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    echo "maturin==1.9.6 --hash=sha256:0246202377c49449315305209f45c8ecef6e2d6bd27a04b5b6f1ab3e4ea47238" > /tmp/maturin.txt \
 && /with-ca python -m pip install -q --no-deps --require-hashes --only-binary :all: -r /tmp/maturin.txt \
 && /with-ca cargo about generate --locked --fail --config container/about.toml --output-file THIRD-PARTY-NOTICES \
      container/about.hbs \
 && ORBITAR_COMMIT=$COMMIT /with-ca maturin build --release --locked --compatibility manylinux_2_28 --out /dist

FROM scratch AS wheel
COPY --from=wheel-build /dist/ /

FROM ghcr.io/astral-sh/uv@sha256:96f40d3904aad8d0bd010539001d36c8e2188b981e1334471a90c4a1d93bb490 AS uv

FROM python@sha256:935c3c5b12357a7e10caa9c1e6ca5692344d97cf229832ef584be2cec9fc03df AS slim

FROM slim AS jth-build
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    --mount=type=bind,from=uv,source=/uv,target=/usr/local/bin/uv \
    --mount=type=bind,from=wheel,target=/tmp/wheel \
    --mount=type=bind,source=packages/orbitar-jth,target=/tmp/jth \
    uv pip install --system --no-cache --no-deps /tmp/wheel/*.whl \
 && cp -R /tmp/jth /src \
 && /with-ca python -I /src/stage.py /tmp/fetched \
 && cd /src && uv build --offline --no-cache --wheel --out-dir /dist

FROM scratch AS jth-wheel
COPY --from=jth-build /dist/ /

FROM slim AS runtime
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    --mount=type=bind,from=uv,source=/uv,target=/usr/local/bin/uv \
    --mount=type=bind,source=python/pylock.toml,target=/tmp/install/pylock.toml \
    --mount=type=bind,from=wheel,target=/tmp/wheel \
    /with-ca uv pip install --system --no-cache --no-build -r /tmp/install/pylock.toml \
 && uv pip install --system --no-cache --no-deps /tmp/wheel/*.whl \
 && useradd --uid 1000 --create-home --home-dir /home/orbitar orbitar \
 && install -d -o 1000 -g 1000 /work
ENV HOME=/work
USER 1000
WORKDIR /work
CMD ["orbitar", "--version"]

FROM toolchain AS bench-build
ARG COMMIT
RUN --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    --mount=type=bind,from=uv,source=/uv,target=/usr/local/bin/uv \
    --mount=type=bind,source=python/pylock.toml,target=/tmp/install/pylock.toml \
    --mount=type=bind,from=wheel,target=/tmp/wheel \
    /with-ca uv pip install --python /opt/python/cp312-cp312/bin/python --no-cache --no-build -r /tmp/install/pylock.toml \
 && uv pip install --python /opt/python/cp312-cp312/bin/python --no-cache --no-deps /tmp/wheel/*.whl \
 && useradd --uid 1000 --create-home --home-dir /home/orbitar orbitar
RUN --mount=type=bind,source=.,target=/context \
    --mount=type=secret,id=ca,required=false --mount=type=bind,source=container/with-ca.sh,target=/with-ca \
    test "$(git -c safe.directory='*' -C /context rev-parse HEAD)" = "$COMMIT" \
 && test -z "$(git -c safe.directory='*' -C /context status --porcelain --untracked-files=no)" \
 && mkdir -p /work/orbitar \
 && git -c safe.directory='*' -C /context archive "$COMMIT" | tar -x -C /work/orbitar \
 && cd /work/orbitar \
 && export CARGO_HOME=/work/cargo-home \
 && /with-ca cargo fetch --locked \
 && cargo build --release --locked --package orbitar --bin orbitar \
 && cargo test --locked --package xtask --test benchmarks --no-run \
 && cargo build --locked --package xtask \
 && sha256sum target/release/orbitar | tee /opt/engine.sha256 \
 && mv /work/orbitar /opt/orbitar && mv /work/cargo-home /opt/cargo-home \
 && chmod -R a+rX /opt/orbitar /opt/cargo-home /opt/cargo /opt/rustup \
 && install -d -o 1000 -g 1000 /work
COPY container/bench.sh /usr/local/bin/orbitar-bench

FROM scratch AS bench
COPY --from=bench-build / /
ENV PATH=/opt/cargo/bin:/opt/python/cp312-cp312/bin:/opt/rh/gcc-toolset-14/root/usr/bin:/usr/local/bin:/usr/bin:/bin \
    LD_LIBRARY_PATH=/opt/rh/gcc-toolset-14/root/usr/lib64:/opt/rh/gcc-toolset-14/root/usr/lib \
    LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 RUSTUP_HOME=/opt/rustup CARGO_HOME=/work/cargo-home HOME=/work
ENV SOURCE_DATE_EPOCH=1767225600
USER 1000
WORKDIR /work
ENTRYPOINT ["orbitar-bench"]
CMD ["help"]
