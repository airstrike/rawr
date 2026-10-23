# rawr-play-bot image (thin LTO: fat LTO of rawr runs out of memory in the
# builder VM). From the repository root:
#   docker build --platform linux/amd64 -f play/Dockerfile -t rawr-play .
# The build stages run on the build host's own architecture. They cross-compile
# for the target platform (amd64 or arm64) when it differs, so no emulation is
# needed, and build natively when it matches (GitHub Actions on x86-64).
# Rendering is tiny-skia on the CPU with the embedded Fira Sans font: the
# runtime needs no GPU, no fonts and no CA bundle (rustls bundles its roots).

FROM --platform=$BUILDPLATFORM rust:1.99.0-slim-bookworm AS toolchain
ARG TARGETARCH
ENV CARGO_PROFILE_RELEASE_STRIP=symbols \
    CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
# A native build (amd64 on an x86-64 host) uses the image's own gcc. A cross
# build installs the target's gcc and names it in /cross.env.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64 gcc=x86-64 ;; \
      arm64) arch=aarch64 gcc=aarch64 ;; \
      *) echo "unsupported TARGETARCH $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    : > /cross.env; \
    if [ "$(uname -m)" != "$arch" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gcc-linux-gnu" "libc6-dev-$TARGETARCH-cross"; \
      rm -rf /var/lib/apt/lists/*; \
      upper="$(echo "$arch" | tr a-z A-Z)"; \
      echo "export CARGO_TARGET_${upper}_UNKNOWN_LINUX_GNU_LINKER=$arch-linux-gnu-gcc CC_${arch}_unknown_linux_gnu=$arch-linux-gnu-gcc" > /cross.env; \
    fi; \
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
# Parallel rustc jobs. The CI build passes 1, as rawr's thin-LTO compile needs
# most of a 16 GB runner.
ARG CARGO_BUILD_JOBS=2
COPY --from=plan /recipe.json /recipe.json
RUN . /cross.env \
    && cargo chef cook --release --locked --target "$(cat /target)" -p rawr-play --features bot --bins --recipe-path /recipe.json
COPY . .
RUN . /cross.env && target="$(cat /target)" \
    && cargo build --release --locked --target "$target" -p rawr-play --features bot --bins \
    && install -D -m 0755 -t /out "target/$target/release/rawr-play" "target/$target/release/rawr-play-bot"

# The fonts the renderer falls back to (CJK, emoji, symbols, mono), pinned
# and checked by play/fetch-fonts.sh. Outside /usr/share/fonts, so iced's
# system scan skips them; `--fonts` (PLAY_FONTS) maps them only.
FROM --platform=$BUILDPLATFORM debian:bookworm-slim AS fonts
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY play/fetch-fonts.sh /fetch-fonts.sh
RUN sh /fetch-fonts.sh /fonts

FROM gcr.io/distroless/cc-debian12:nonroot
# The commit the image was built from, reported on the dashboard.
ARG PLAY_VERSION=dev
COPY --from=build /out/ /usr/local/bin/
COPY --from=fonts /fonts/ /usr/share/rawr-play/fonts/
ENV PLAY_WORKERS=1 PLAY_MEMORY_MB=1024 PLAY_VERSION=$PLAY_VERSION PLAY_FONTS=/usr/share/rawr-play/fonts
ENTRYPOINT ["/usr/local/bin/rawr-play-bot"]
