# FileHub backend — Rust/Axum.
#
# Multi-stage: rust toolchain in the builder, a slim Debian runtime with just
# ca-certificates + tzdata. sqlx has no compile-time DB checks here
# (migrations run at boot via sqlx::migrate!), so the build needs NO database.
#
# Office→PDF preview shells out to `soffice` IF present (backend/src/p1.rs
# degrades gracefully when it's missing). LibreOffice adds ~400 MB, so the
# SIT image ships without it — office files fall back to download-only.
# To enable previews, add:  apt-get install -y --no-install-recommends libreoffice
#
# Build (from repo root):
#   docker build --platform=linux/amd64 -t avalantglobal/filehub-api:VERSION -f backend/Dockerfile backend/

FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
# Cache cargo registry + build dir across builds (Jenkins agent keeps the
# BuildKit cache), so source-only changes don't recompile every dependency.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/filehub-backend /usr/local/bin/filehub-backend

FROM debian:bookworm-slim AS runtime
ENV TZ=Asia/Bangkok
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata curl \
    && rm -rf /var/lib/apt/lists/* \
    && adduser --system --group --home /app appuser \
    && mkdir -p /data/storage \
    && chown -R appuser:appuser /data/storage
COPY --from=builder /usr/local/bin/filehub-backend /usr/local/bin/filehub-backend
USER appuser
WORKDIR /app
# Release builds fail-fast without DATABASE_URL / CORS_ORIGIN / WOPI_SECRET —
# they come from the K8s Secret (envFrom), never baked into the image.
ENV PORT=8090 STORAGE_ROOT=/data/storage
EXPOSE 8090
CMD ["filehub-backend"]
