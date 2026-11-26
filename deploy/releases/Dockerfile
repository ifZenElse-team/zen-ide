# The Zen IDE release server (ADR-0020). Built from the workspace root:
#   docker build -f deploy/releases/Dockerfile .
FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --package zen-releases

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin zen
COPY --from=build /src/target/release/zen-releases /usr/local/bin/zen-releases
USER zen
VOLUME /data
EXPOSE 8787
# ZEN_RELEASES_PUBLIC_KEY and ZEN_RELEASES_TOKEN come from the environment (compose env_file).
ENTRYPOINT ["zen-releases", "serve", "--listen", "0.0.0.0:8787", "--data", "/data"]
