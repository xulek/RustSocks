# Docker Deployment

This page summarizes the Docker workflow. For full detail and troubleshooting, see `README-DOCKER.md` in the repository root.

## Build Image

```bash
docker build -t rustsocks:latest .
```

## Run with Docker Compose

```bash
docker-compose up -d
docker-compose logs -f rustsocks
docker-compose down
```

## Access Endpoints

- Dashboard: `http://localhost:9090/`
- Swagger UI: `http://localhost:9090/swagger-ui/`
- Health check: `http://localhost:9090/health`

## Configuration Options

### Edit the Docker Template

```bash
nano docker/configs/rustsocks.docker.toml
docker-compose restart rustsocks
```

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
