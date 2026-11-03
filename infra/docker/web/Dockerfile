# syntax=docker/dockerfile:1

# ============================================================================
# Kaiku web client: the Solid.js app (client/) as static files behind nginx.
#
# Build from the repository root:
#   docker build -f infra/docker/web/Dockerfile --build-arg KAIKU_VERSION=1.0.0 .
#
# Serve it on the same origin as the server: a reverse proxy sends /api/,
# /auth/, /ws and /health to the server and everything else here (see
# docs/admin-guide/ops/web-client.md). Nothing about the server is baked in;
# the app talks to its own origin.
# ============================================================================

# ----------------------------------------------------------------------------
# Build stage
# ----------------------------------------------------------------------------
FROM oven/bun:1.3.14-alpine AS build

WORKDIR /build/client

# Dependencies first for layer caching
COPY client/package.json client/bun.lock ./
RUN bun install --frozen-lockfile

COPY client/ ./

ARG KAIKU_VERSION=dev
ENV VITE_APP_VERSION=${KAIKU_VERSION}
# client/.env.production sets VITE_SERVER_URL for the desktop builds. The
# web client must use its own origin, so override it with an empty value
# (process env wins over .env files) and fail if the URL still got in.
ENV VITE_SERVER_URL=
RUN bun run build \
 && baked="$(sed -n 's/^VITE_SERVER_URL=//p' .env.production 2>/dev/null | tr -d '\r')" \
 && if [ -n "$baked" ] && grep -rqF "$baked" dist; then \
        echo "VITE_SERVER_URL from .env.production ended up in the web build" >&2; exit 1; \
    fi \
 && find dist -name '*.md' -delete

# ----------------------------------------------------------------------------
# Runtime stage: unprivileged nginx (UID 101), port 8080
# ----------------------------------------------------------------------------
FROM nginxinc/nginx-unprivileged:1.30.4-alpine AS runtime

LABEL org.opencontainers.image.title="Kaiku Web"
LABEL org.opencontainers.image.description="Kaiku web client (static files)"
LABEL org.opencontainers.image.vendor="Kaiku"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Our own config and entrypoint replace the base image's: its entrypoint
# scripts rewrite files under /etc/nginx, which a read-only root filesystem
# does not allow. Everything written at runtime goes to /tmp.
COPY --chown=0:0 infra/docker/web/nginx.conf /etc/nginx/nginx.conf
COPY --chown=0:0 infra/docker/web/kaiku.conf.template /etc/nginx/kaiku/kaiku.conf.template
COPY --chown=0:0 --chmod=0755 infra/docker/web/entrypoint.sh /usr/local/bin/kaiku-web-entrypoint
COPY --chown=0:0 --from=build /build/client/dist /usr/share/nginx/html

# Origins (space separated) allowed for media and fetches beyond this origin,
# e.g. the public S3/files host that serves attachment downloads.
ENV KAIKU_MEDIA_ORIGINS=""

USER 101
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/kaiku-web-entrypoint"]
