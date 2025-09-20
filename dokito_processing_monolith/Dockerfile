FROM rust:1.88 AS base
RUN curl https://google.com
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall sccache --version ^0.7
RUN cargo binstall cargo-chef --version ^0.1
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

FROM base AS planner
WORKDIR /app
COPY dokito-backend/dokito_processing_monolith/ ./dokito_processing_monolith/
COPY dokito-backend/dokito_types/ ./dokito_types/
COPY scrapers/openscraper_types/ ./scrapers/openscraper_types/
WORKDIR /app/dokito_processing_monolith
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
  cargo chef prepare --recipe-path recipe.json

FROM base as builder
WORKDIR /app
COPY --from=planner /app/dokito_processing_monolith/recipe.json ./dokito_processing_monolith/recipe.json
COPY dokito-backend/dokito_types/ ./dokito_types/
COPY scrapers/openscraper_types/ ./scrapers/openscraper_types/
WORKDIR /app/dokito_processing_monolith
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
  cargo chef cook --release --recipe-path recipe.json
COPY dokito-backend/dokito_processing_monolith/ .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
  cargo build --release
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/dokito_processing_monolith/target/release/dokito_processing_monolith /usr/local/bin
ENTRYPOINT ["/usr/local/bin/dokito_processing_monolith"]
