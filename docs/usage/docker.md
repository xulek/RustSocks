# Docker Deployment

This page summarizes the Docker workflow. For full detail and troubleshooting, see `README-DOCKER.md` in the repository root.

## Build Image

```bash
docker build -t rustsocks:latest .
```

## Run with Docker Compose

The Compose file requires three secrets and refuses to start without them. Create a `.env` file from the template and replace every placeholder:

```bash
cp docker/.env.example .env
${EDITOR:-vi} .env     # RUSTSOCKS_PROXY_PASSWORD, RUSTSOCKS_DASHBOARD_PASSWORD, RUSTSOCKS_DASHBOARD_SESSION_SECRET
openssl rand -base64 32   # a good value for the session secret
```

Then start it:

```bash
docker compose up -d
docker compose logs -f rustsocks
docker compose down
```

The container runs with a read-only root filesystem and all capabilities dropped; memory, CPU, process and open-file limits can be set in `.env` (see `README-DOCKER.md`).

## Access Endpoints

Ports are published on the loopback interface only (`127.0.0.1`). Change the port mappings deliberately if remote clients need access.

- SOCKS proxy: `127.0.0.1:1080`
- Dashboard: `http://localhost:9090/` (log in with the dashboard user from `.env`)
- Swagger UI: `http://localhost:9090/swagger-ui/`
- Health check: `http://localhost:9090/health` (public; every `/api/*` endpoint needs a login or token)

## Configuration Options

### Edit the Docker Template

The image carries a template (`docker/configs/rustsocks.toml`) that seeds the configuration volume the first time the container starts. Changing the template therefore only affects new volumes; to apply it to an existing deployment, edit the file inside the volume (or use the dashboard's configuration editor) and restart:

```bash
docker compose restart rustsocks
```

Secrets are read from the environment through `${VAR}` placeholders in the template, so they never need to be written into the file.

### Mount Your Own Config

```yaml
services:
  rustsocks:
    volumes:
      - ./my-rustsocks.toml:/etc/rustsocks/rustsocks.toml:ro
```

### Entrypoint Environment Variables

The Docker entrypoint reads only a small set of environment variables:

```yaml
services:
  rustsocks:
    environment:
      - RUSTSOCKS_CONFIG=/etc/rustsocks/rustsocks.toml
      - RUSTSOCKS_DB_PATH=/data/sessions.db
```

All runtime settings (bind address/port, dashboard, API, etc.) must be set in the TOML config or passed as CLI flags.

Example override for CLI flags:

```yaml
services:
  rustsocks:
    command: ["rustsocks", "--config", "/etc/rustsocks/rustsocks.toml", "--bind", "0.0.0.0", "--port", "1080", "--log-level", "debug"]
```

## Authentication Examples

User/password:

```toml
[auth]
socks_method = "userpass"

[[auth.users]]
username = "alice"
password = "secret123"
```

PAM username:

```toml
[auth]
socks_method = "pam.username"

[auth.pam]
username_service = "rustsocks"
```

PAM address:

```toml
[auth]
client_method = "pam.address"

[auth.pam]
address_service = "rustsocks-client"
```
