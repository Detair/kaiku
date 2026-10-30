# Web client

Kaiku ships its web client as a separate container image next to the server
image. The server image serves no frontend.

| | |
|---|---|
| Image | `ghcr.io/detair/kaiku-web:<version>` (same version as `kaiku-server`) |
| Port | `8080` (HTTP; TLS terminates at your reverse proxy) |
| User | UID `101`, no capabilities needed |
| Filesystem | read-only root works; mount a tmpfs on `/tmp` |
| Health check | `GET /healthz` returns `ok` (the server's own is `/health`) |

The web client talks to the server on **its own origin** (same scheme, host
and port). Nothing about the server is baked into the image, so one image
works for every installation. Web client and server must therefore share one
public origin, e.g. `https://chat.example.com`. A separate host such as
`app.example.com` does not work: the sign-in cookie, the WebSocket and the
sign-in popup all rely on the same origin.

## Routing

Put both containers behind one reverse proxy and route by path:

| Path | Goes to |
|---|---|
| `/api/` (all of it, including `/api/webhooks/…`, `/api/files/…`, `/api/gateway/bot`, `/api/docs`) | server |
| `/auth/` (sign-in, refresh, SSO callback pages) | server |
| `/ws` (WebSocket) | server |
| `/health` | server |
| everything else | web client |

Proxy requirements for the server paths:

- `/ws` and `/api/gateway/bot` are WebSockets: pass `Upgrade`/`Connection`
  and use a long read timeout (an hour, for example).
- Pass `Host`, `X-Forwarded-For` and `X-Forwarded-Proto`.
- Allow request bodies up to your upload limit (`MAX_UPLOAD_SIZE`).
- Do **not** add the web client's Content-Security-Policy to the server
  paths. The SSO callback pages under `/auth/` are served by the server and
  would be blocked by it.

The web client sets its own headers (see below). Do not add a second
`Content-Security-Policy` for the web paths at the proxy; browsers apply
both, and the stricter one wins.

### Caddy

The compose setup in `infra/compose/` does exactly this:

```caddy
chat.example.com {
	handle /api/* {
		reverse_proxy server:8080
	}
	handle /auth/* {
		reverse_proxy server:8080
	}
	handle /health {
		reverse_proxy server:8080
	}
	handle /ws {
		reverse_proxy server:8080
	}
	handle {
		reverse_proxy web:8080
	}
}
```

Caddy passes WebSockets and forwarded headers on its own.

### nginx

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 443 ssl;
    http2 on;
    server_name chat.example.com;
    # ssl_certificate / ssl_certificate_key ...

    client_max_body_size 100m;   # >= MAX_UPLOAD_SIZE

    location /api/ {
        proxy_pass http://kaiku-server:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;          # /api/gateway/bot
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 1h;
    }
    location /auth/ {
        proxy_pass http://kaiku-server:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    location = /ws {
        proxy_pass http://kaiku-server:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 1h;
    }
    location = /health {
        proxy_pass http://kaiku-server:8080;
    }
    location / {
        proxy_pass http://kaiku-web:8080;
        proxy_set_header Host $host;
    }
}
```

### Kubernetes Ingress

Use one host with path rules: `Prefix` `/api` and `/auth`, `Exact` `/ws` and
`/health` to the server Service; `Prefix` `/` to the web Service. Most
ingress controllers pick the longest matching path, so the order does not
matter. Make sure the controller's WebSocket timeout covers long-lived
connections.

## Running the container

```bash
docker run -d --name kaiku-web \
  --read-only --tmpfs /tmp \
  --cap-drop ALL --security-opt no-new-privileges \
  -e KAIKU_MEDIA_ORIGINS="https://files.example.com" \
  -p 127.0.0.1:8081:8080 \
  ghcr.io/detair/kaiku-web:1.0.0
```

### Configuration

The only setting is read when the container starts:

| Variable | Default | Meaning |
|---|---|---|
| `KAIKU_MEDIA_ORIGINS` | empty | Space-separated origins (`https://host[:port]`, no path) of a separate files/S3 host that serves attachments, e.g. the origin of `S3_PUBLIC_URL`. Videos and audio attachments play from there and need it in the CSP. Leave empty when files come from your Kaiku domain. The container refuses to start on a value that is not a plain origin. |

## What the image sends

- **Caching:** `index.html` and other unversioned files `no-cache`; the
  fingerprinted `/assets/*` one year, `immutable`; fonts, theme images and
  sounds one day. After an upgrade, browsers pick up the new version on the
  next page load.
- **Routing:** unknown paths return the app (client-side routes such as
  `/login` or `/invite/…`). A missing `/assets/…` file is a `404`. `/api/…`,
  `/auth/…`, `/ws` and `/health` return `404` from this container, so a proxy
  that sends them here by mistake fails visibly.
- **Security headers:**
  - `Content-Security-Policy`:
    - scripts only from the same origin, no inline scripts;
    - styles from the same origin plus inline styles;
    - images from the same origin, `data:`, `blob:` and any `https:` origin
      (bot embeds, link previews);
    - media and fetches from the same origin plus `KAIKU_MEDIA_ORIGINS`;
    - WebSocket to the same host;
    - no frames, no plugins;
    - not embeddable (`frame-ancestors 'none'`).
  - `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`,
    `Referrer-Policy: strict-origin-when-cross-origin`.
  - `Permissions-Policy`: microphone, camera and screen capture for this
    origin only.
  - `Cross-Origin-Opener-Policy: same-origin-allow-popups` (the sign-in popup
    needs its opener).
  - HSTS is up to your TLS proxy (the server also sends it on its paths).

## Browser support and limits

Current Chrome, Edge, Firefox and Safari. Compared with the desktop app:

- **Push to talk** only works while the Kaiku tab is focused.
- **Notifications** need the browser's permission; phone browsers show none.
- **Screen sharing** works in desktop browsers only.
