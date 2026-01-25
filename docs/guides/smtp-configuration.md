# SMTP Configuration

RustSocks supports SMTP settings stored in the database and managed through the web dashboard. Passwords are encrypted at rest using AES-256-GCM with your `sessions.api_token` as the encryption key.

## Requirements

- Build with the `database` feature (or `--all-features`).
- Enable session storage with a database URL.
- Set `sessions.api_token` so SMTP passwords can be encrypted.

Example snippet:

```toml
[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
stats_api_enabled = true
dashboard_enabled = true
api_token = "change-me" # Required for SMTP password encryption
```

## Dashboard Workflow

1. Open the dashboard and navigate to **SMTP**.
2. Choose a connection mode and fill in host, port, and sender details.
3. If the mode requires auth, enter the username and password (leave blank to keep the existing password).
4. Configure notification recipients and toggle which alerts should be delivered.
5. Save the configuration.
6. Use **Test Connection** to send a test email.

## Notification Settings

The SMTP page includes per-category switches and a recipient list used for operational alerts:

- **Critical failures** (blocked startup or configuration errors)
- **Security incidents** (failed dashboard logins)
- **Configuration changes** (high-impact updates via the dashboard)
- **Service status** (start/restart notifications)
- **Resource pressure** (CPU/RAM/disk usage above thresholds)
- **Connection pressure** (active sessions approaching the configured limit)

Enter one or more recipient addresses separated by commas or new lines. Notifications are only sent when SMTP is enabled and at least one recipient is configured.

### Cooldown and Thresholds

To avoid flooding inboxes, alerts share a configurable cooldown (in seconds) applied per category. Resource and connection alerts use adjustable thresholds (percentage values).

Defaults:
- Cooldown: 60 minutes (3600s)
- CPU/RAM: 85%
- Disk: 90%
- Connection utilization: 85% of the effective limit (min of `server.max_connections` and the Linux file descriptor soft limit when available)

Resource checks run periodically (every ~30 seconds) and respect the cooldown per category. The dashboard presents cooldown in minutes and converts to seconds internally.

## API Endpoints

- `GET /api/smtp/modes` - Supported SMTP modes for dropdowns.
- `GET /api/smtp/config` - Current configuration (password omitted).
- `PUT /api/smtp/config` - Update configuration (password optional).
- `POST /api/smtp/test` - Send a test email.

## Notes

- SMTP settings are stored as a singleton row in the database.
- Passwords are only decrypted when `sessions.api_token` is configured.
- If the database feature is disabled, SMTP endpoints return an error.
