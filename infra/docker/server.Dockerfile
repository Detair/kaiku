# syntax=docker/dockerfile:1

# ============================================================================
# Build Stage
# ============================================================================
FROM rust:1.75-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY server/ server/

# Build release binary
RUN cargo build --release --package vc-server

# ============================================================================
# Runtime Stage
# ============================================================================
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false voicechat

# Copy binary from builder
COPY --from=builder /app/target/release/vc-server /app/vc-server

# Set ownership
RUN chown -R voicechat:voicechat /app

USER voicechat

# Expose ports
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run
ENTRYPOINT ["/app/vc-server"]
