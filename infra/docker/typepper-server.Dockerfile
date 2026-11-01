# typepper-server. Railway builds it from the repo root (see infra/railway):
#   docker build -f infra/docker/typepper-server.Dockerfile .
# Bases are pinned by digest (dependabot bumps them); keep the tag's Rust in
# step with rust-toolchain.toml.

FROM lukemathwalker/cargo-chef:0.1.78-rust-1.99.1-slim-bookworm@sha256:e005be5aebbfe6316c3e679f39a0317bae80175dc75dfb0b2e179f82a7304780 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# dependencies only: this layer stays cached until the manifests or Cargo.lock change
RUN cargo chef cook --release --locked -p typepper-server --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked -p typepper-server

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=builder /app/target/release/typepper-server /typepper-server
# PORT (http, Railway's variable) and TYPEPPER_SSH_PORT (the TCP proxy's target)
EXPOSE 8080 2222
ENTRYPOINT ["/typepper-server"]
