# Builds the Zen IDEs and Zen Toolbox for Linux (x86_64) on any machine with Docker, as part of
# `cargo xtask release` (docs/releasing.md). Ubuntu 22.04, so that the binaries run on it and on
# every later Ubuntu (glibc 2.35). The binaries link only glibc; X11, xkbcommon and Wayland are
# loaded at run time from the desktop. The GTK and xdo headers are for crates that look for them.
FROM --platform=linux/amd64 ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl build-essential pkg-config \
       libgtk-3-dev libxdo-dev libayatana-appindicator3-dev dpkg-dev \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
WORKDIR /src
