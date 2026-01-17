# syntax=docker/dockerfile:1

# ============================================================================
# Build Stage - Rust compilation
# ============================================================================
FROM rust:1.83-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.lock ./
COPY shared/Cargo.toml shared/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
COPY client/src-tauri/Cargo.toml client/src-tauri/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p shared/src server/src client/src-tauri/src && \
    echo "fn main() {}" > shared/src/lib.rs && \
    echo "fn main() {}" > server/src/lib.rs && \
    echo "fn main() {}" > server/src/main.rs && \
    echo "fn main() {}" > client/src-tauri/src/lib.rs && \
    echo "fn main() {}" > client/src-tauri/src/main.rs

# Build dependencies only (this layer will be cached)
RUN cargo build --release --package vc-server 2>/dev/null || true

# Copy actual source code
COPY shared/ shared/
COPY server/ server/

# Build the actual binary
RUN cargo build --release --package vc-server

# ============================================================================
# Runtime Stage - Hardened minimal image (Bitnami minideb)
# ============================================================================
FROM bitnami/minideb:bookworm AS runtime

LABEL org.opencontainers.image.title="VoiceChat Server"
LABEL org.opencontainers.image.description="Self-hosted voice and text chat platform"
LABEL org.opencontainers.image.vendor="Canis"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Install minimal runtime dependencies
RUN install_packages ca-certificates libssl3 curl

# Create non-root user (Bitnami convention: UID 1001)
RUN useradd -r -u 1001 -g root -s /sbin/nologin voicechat

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/vc-server /app/vc-server

# Copy migrations for runtime application
COPY server/migrations /app/migrations

# Set permissions (Bitnami convention: group writable)
RUN chown -R 1001:0 /app && chmod -R g=u /app

USER 1001

# Expose HTTP port
EXPOSE 8080

# Expose WebRTC UDP ports (configurable via RTP_PORT_MIN/MAX)
EXPOSE 10000-10100/udp

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/health || exit 1

# Run server
ENTRYPOINT ["/app/vc-server"]
