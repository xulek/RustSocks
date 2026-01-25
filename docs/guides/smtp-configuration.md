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
4. Save the configuration.
5. Use **Test Connection** to send a test email.

## API Endpoints

- `GET /api/smtp/modes` - Supported SMTP modes for dropdowns.
- `GET /api/smtp/config` - Current configuration (password omitted).
- `PUT /api/smtp/config` - Update configuration (password optional).
- `POST /api/smtp/test` - Send a test email.

## Notes

- SMTP settings are stored as a singleton row in the database.
- Passwords are only decrypted when `sessions.api_token` is configured.
- If the database feature is disabled, SMTP endpoints return an error.
