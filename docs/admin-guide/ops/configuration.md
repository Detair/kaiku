# Configuration Guide

## Server URL Configuration

The Kaiku client can be configured with a default server URL to avoid manual entry on the login screen.

### Development Setup

1. The client has a `.env` file with the default development server URL:
   ```bash
   VITE_SERVER_URL=http://localhost:8080
   ```

2. When you open the login page at http://localhost:5173, the server URL field will be pre-filled with `http://localhost:8080`.

3. You can still change the server URL if needed for testing different servers.

### Production Setup

For production deployments using Docker Compose there is nothing to set for the
web client: the `kaiku-web` image is the same for every installation and talks
to the server on its own origin (see [Web client](web-client.md)). Users of the
desktop app and the Android app enter your server's address (for example
`https://chat.yourdomain.com`) when they sign in.

The server's own settings live in `infra/compose/.env` (start from `.env.example`
in the repository root); see [Deployment](deployment.md).

### Environment Variables

#### Client Environment Variables (client/.env)

- `VITE_SERVER_URL`: Default server URL shown in login/register screens
  - Development: `http://localhost:8080`
  - Production: not used; the web client uses its own origin and the apps ask for the address

#### Server Environment Variables

See `.env.example` in the root directory and the [environment variables](../configuration/environment.md) reference for the server configuration options.

### Testing the Configuration

1. Start the development server:
   ```bash
   cd client
   bun run dev
   ```

2. Open http://localhost:5173/login in your browser

3. The "Server URL" field should be pre-filled with `http://localhost:8080`

4. You can still edit this field if you need to connect to a different server
