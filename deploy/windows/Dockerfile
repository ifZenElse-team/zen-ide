# Cross-builds the Zen IDEs and Zen Toolbox for Windows (x86_64, GNU toolchain) on any machine with
# Docker: `cargo xtask release-windows` (docs/releasing.md). Pure Rust, so only the MinGW linker and
# its runtime are needed from outside Rust.
FROM rust:1-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 zip \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-pc-windows-gnu
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
WORKDIR /src
