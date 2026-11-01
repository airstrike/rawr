# rawr-play-bot image (thin LTO: fat LTO of rawr runs out of memory in the
# builder VM). From the repository root:
#   docker build --platform linux/amd64 -f play/Dockerfile -t rawr-play .
# The build stages run on the build host's own architecture. They cross-compile
# for the target platform (amd64 or arm64) when it differs, so no emulation is
# needed, and build natively when it matches (GitHub Actions on x86-64).
# A snippet runs on the universe runner (the `universe` stage below), which
# needs no GPU. TLS (Discord's gateway and API) is OpenSSL through native-tls: the
# build links the target's libssl-dev, and the distroless cc runtime already
# ships libssl3 and the CA bundle.

FROM --platform=$BUILDPLATFORM rust:1.99.0-slim-bookworm AS toolchain
ARG TARGETARCH
ENV CARGO_PROFILE_RELEASE_STRIP=symbols \
    CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
# A native build (amd64 on an x86-64 host) uses the image's own gcc and
# libssl-dev, found through pkg-config. A cross build installs the target's
# gcc and, through Debian multiarch, the target's libssl-dev, and names them
# in /cross.env. The cross gcc searches /usr/include/<triplet>, where the
# target's openssl/configuration.h lives.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64 gcc=x86-64 ;; \
      arm64) arch=aarch64 gcc=aarch64 ;; \
      *) echo "unsupported TARGETARCH $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    : > /cross.env; \
    if [ "$(uname -m)" = "$arch" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends libssl-dev pkg-config; \
    else \
      dpkg --add-architecture "$TARGETARCH"; \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gcc-linux-gnu" "libc6-dev-$TARGETARCH-cross" "libssl-dev:$TARGETARCH"; \
      upper="$(echo "$arch" | tr a-z A-Z)"; \
      echo "export CARGO_TARGET_${upper}_UNKNOWN_LINUX_GNU_LINKER=$arch-linux-gnu-gcc CC_${arch}_unknown_linux_gnu=$arch-linux-gnu-gcc ${upper}_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/$arch-linux-gnu ${upper}_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include" > /cross.env; \
    fi; \
    rm -rf /var/lib/apt/lists/*; \
    rustup target add "$arch-unknown-linux-gnu"; \
    echo "$arch-unknown-linux-gnu" > /target; \
    cargo install cargo-chef --locked
WORKDIR /src

# The dependency list alone, so the next stage's cache survives source edits.
FROM toolchain AS plan
COPY . .
RUN cargo chef prepare --recipe-path /recipe.json

# Dependencies compile in their own layer, cached until Cargo.lock or a
# manifest changes. The workspace crates (rawr among them) compile after.
FROM toolchain AS build
# Parallel rustc jobs, which also bound the codegen and thin-LTO threads. They
# change build time only, never the binary's speed. CI passes 4 (16 GB runner
# plus swap); 2 keeps a small local builder VM within its memory.
ARG CARGO_BUILD_JOBS=2
COPY --from=plan /recipe.json /recipe.json
RUN . /cross.env \
    && cargo chef cook --release --locked --target "$(cat /target)" -p rawr-play --features bot --bins --recipe-path /recipe.json
COPY . .
RUN . /cross.env && target="$(cat /target)" \
    && cargo build --release --locked --target "$target" -p rawr-play --features bot --bins \
    && install -D -m 0755 -t /out "target/$target/release/rawr-play" "target/$target/release/rawr-play-bot"

# The universe runner (#434, #436): rawr-import builds generated bindings
# for play/universe (iced at the pinned rev, iced_test and tokio) and the
# runner that links them, once, here. iced's shell there is ?play's own
# (play/universe/shell), which draws an iced program's first settled frame
# to a PNG instead of opening a window. The bot runs each snippet on that
# runner with no Cargo and no nightly. This stage runs on the target
# platform, since the project's own Cargo builds the runner for the machine
# it runs on. The universe keeps its build path, /universe, in the runtime
# image: a snippet is compiled beside that Cargo.toml. The universe's own
# toolchain is the extraction nightly, as it is on a machine whose default
# is that nightly: a build script that reads the compiler's channel (libm's)
# then gives the extraction and the runner one API.
FROM rust:1.99.0-slim-bookworm AS universe
ARG CARGO_BUILD_JOBS=2
ENV RUSTUP_TOOLCHAIN=nightly-2026-07-11
RUN apt-get update \
    && apt-get install -y --no-install-recommends git libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && rustup toolchain install nightly-2026-07-11 --profile minimal --component rustc-dev,rust-src --no-self-update
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p rawr-import-cli --bin rawr-import \
    && cargo build --release --locked -p rawr-import --features rustc-driver --bin rawr-import-driver \
    && cp -r play/universe /universe \
    && RAWR_UNIVERSE_RUNNER=1 /src/target/release/rawr-import run /universe \
    && install -D -m 0755 /universe/target/rawr/runner/target/debug/play /out/universe/runner \
    && install -D -m 0644 /universe/Cargo.toml /out/universe/Cargo.toml

FROM gcr.io/distroless/cc-debian12:nonroot
# The commit the image was built from, reported on the dashboard.
ARG PLAY_VERSION=dev
COPY --from=build /out/ /usr/local/bin/
COPY --from=universe /out/universe/ /universe/
ENV PLAY_WORKERS=1 PLAY_MEMORY_MB=1024 PLAY_VERSION=$PLAY_VERSION PLAY_UNIVERSE=/universe
ENTRYPOINT ["/usr/local/bin/rawr-play-bot"]
