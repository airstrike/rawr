# rawr-play-bot image (thin LTO: fat LTO of rawr runs out of memory in the
# builder VM). From the repository root:
#   docker build --platform linux/amd64 -f play/Dockerfile -t rawr-play .
# The build stage runs on the build host's own architecture and cross-compiles
# for the target platform (amd64 or arm64), so no emulation is needed.
# Rendering is tiny-skia on the CPU with the embedded Fira Sans font: the
# runtime needs no GPU, no fonts and no CA bundle (rustls bundles its roots).

FROM --platform=$BUILDPLATFORM rust:1-slim-bookworm AS build
ARG TARGETARCH
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CARGO_PROFILE_RELEASE_STRIP=symbols \
    CARGO_PROFILE_RELEASE_LTO=thin \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
    CARGO_BUILD_JOBS=1
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) arch=x86_64 gcc=x86-64 ;; \
      arm64) arch=aarch64 gcc=aarch64 ;; \
      *) echo "unsupported TARGETARCH $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ "$(uname -m)" != "$arch" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gcc-linux-gnu" "libc6-dev-$TARGETARCH-cross"; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$arch-unknown-linux-gnu"; \
    echo "$arch-unknown-linux-gnu" > /target
WORKDIR /src
COPY . .
RUN target="$(cat /target)" \
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
