# Configuration Guide

## Server URL Configuration

The VoiceChat client can be configured with a default server URL to avoid manual entry on the login screen.

### Development Setup

1. The client has a `.env` file with the default development server URL:
   ```bash
   VITE_SERVER_URL=http://localhost:8080
   ```

2. When you open the login page at http://localhost:5173, the server URL field will be pre-filled with `http://localhost:8080`.

3. You can still change the server URL if needed for testing different servers.

### Production Setup

For production deployments using Docker Compose:

1. Copy `.env.example` to `.env`:
   ```bash
   cp .env.example .env
   ```

2. Edit `.env` and set your server URL:
   ```bash
   SERVER_URL=https://chat.yourdomain.com
   ```

3. Build and deploy with docker-compose:
   ```bash
   cd infra/compose
   docker compose build
   docker compose up -d
   ```

The client will be built with the configured `SERVER_URL` baked into the production build.

### Environment Variables

#### Client Environment Variables (client/.env)

- `VITE_SERVER_URL`: Default server URL shown in login/register screens
  - Development: `http://localhost:8080`
  - Production: Set via `SERVER_URL` in root `.env` file

#### Server Environment Variables

See `.env.example` in the root directory for all available server configuration options.

### Testing the Configuration

1. Start the development server:
   ```bash
   cd client
   bun run dev
   ```

2. Open http://localhost:5173/login in your browser

3. The "Server URL" field should be pre-filled with `http://localhost:8080`

4. You can still edit this field if you need to connect to a different server
