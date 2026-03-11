# ── builder ───────────────────────────────────────────────────────────────────
FROM rust:slim-bookworm AS rust-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# cache dependencies before copying source
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release -p control-plane

# ── frontend builder ──────────────────────────────────────────────────────────
FROM node:lts-slim AS frontend-builder

RUN corepack enable

WORKDIR /build

COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY frontend/ ./
RUN pnpm build

# ── runtime ───────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /sbin/nologin spwn

WORKDIR /app

COPY --from=rust-builder /build/target/release/spwn-control-plane ./
COPY --from=frontend-builder /build/dist ./frontend/dist

RUN mkdir -p /var/lib/spwn/static && chown spwn:spwn /var/lib/spwn/static

USER spwn

ENV LISTEN_ADDR=0.0.0.0:3019 \
    GRPC_LISTEN_ADDR=0.0.0.0:5000 \
    FRONTEND_PATH=/app/frontend/dist \
    STATIC_FILES_PATH=/var/lib/spwn/static

EXPOSE 3019 5000

ENTRYPOINT ["./spwn-control-plane"]
