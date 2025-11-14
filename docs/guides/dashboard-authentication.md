# Dashboard Authentication Guide

Complete guide to setting up authentication for the RustSocks dashboard with optional Altcha CAPTCHA.

## Table of Contents

- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Altcha CAPTCHA](#altcha-captcha)
- [Security Best Practices](#security-best-practices)
- [Troubleshooting](#troubleshooting)

## Quick Start

### Basic Authentication (Username/Password Only)

1. **Enable authentication in config**:

```toml
[sessions]
enabled = true
stats_api_enabled = true
dashboard_enabled = true

[sessions.dashboard_auth]
enabled = true
altcha_enabled = false
session_secret = "your-super-secret-random-string-here"
session_duration_hours = 24

[[sessions.dashboard_auth.users]]
username = "admin"
password = "SecurePassword123"
```

2. **Start the server**:

```bash
./target/release/rustsocks --config config/rustsocks.toml
```

3. **Access dashboard**: Navigate to `http://127.0.0.1:9090`

4. **Login** with credentials from config

## Configuration

### Session Settings

```toml
[sessions.dashboard_auth]
# Enable/disable authentication
enabled = true

# Session secret for token generation
# CRITICAL: Use a long, random string in production
# Generate with: openssl rand -base64 32
session_secret = "change-me-to-very-long-random-secret-string"

# How long sessions remain valid (in hours)
session_duration_hours = 24  # Default: 24 hours
```

### User Management

Add multiple users with different credentials:

```toml
[[sessions.dashboard_auth.users]]
username = "admin"
password = "AdminPassword123"

[[sessions.dashboard_auth.users]]
username = "monitor"
password = "MonitorPassword456"

[[sessions.dashboard_auth.users]]
username = "operator"
password = "OperatorPassword789"
```

**Note**: In future versions, user management will support:
- Password hashing (bcrypt/argon2)
- Role-based access control
- LDAP/Active Directory integration

### Base Path Support

Dashboard authentication respects base path configuration:

```toml
[sessions]
base_path = "/rustsocks"
```

All auth endpoints will be accessible at:
- `/rustsocks/api/auth/login`
- `/rustsocks/api/auth/logout`
- `/rustsocks/api/auth/check`

## Altcha CAPTCHA

Altcha is a self-hosted, privacy-first proof-of-work CAPTCHA that works completely offline without external services.

### How Altcha Works

1. **Server** generates a cryptographic challenge (HMAC-SHA256 signed hash)
2. **Browser** solves proof-of-work puzzle by finding a secret number
3. **Server** verifies the solution before allowing login

### Features

✅ **Fully self-hosted** - No external dependencies
✅ **Privacy-first** - No tracking, no third-party services
✅ **Offline-capable** - Works without internet
✅ **Open source** - Based on [Altcha](https://github.com/altcha-org/altcha)
✅ **HMAC-secured** - Cryptographically signed challenges
✅ **Configurable difficulty** - Adjust PoW complexity

### Enabling Altcha

**1. Update configuration**:

```toml
[sessions.dashboard_auth]
enabled = true
altcha_enabled = true  # Enable Altcha CAPTCHA
session_secret = "your-secret-key-used-for-hmac-signing"
```

**2. Rebuild dashboard** (if modified):

```bash
cd dashboard
npm run build
cd ..
```

**3. Restart server**:

```bash
./target/release/rustsocks --config config/rustsocks.toml
```

**4. Test login page**:
- Navigate to dashboard
- You'll see Altcha widget on login page
- Widget will solve PoW challenge automatically
- Submit form after challenge is solved

### Built-in vs External Altcha

#### Built-in Endpoint (Default)

When `altcha_enabled = true` with no `altcha_challenge_url`:

```toml
[sessions.dashboard_auth]
altcha_enabled = true
# No altcha_challenge_url specified - uses built-in
```

**Endpoint**: `GET /api/auth/altcha-challenge`

**Response format**:
```json
{
  "algorithm": "SHA-256",
  "challenge": "hex_hash_of_salt",
  "salt": "random_string?expires=timestamp",
  "signature": "hmac_sha256_signature",
  "maxnumber": 50000
}
```

**Parameters**:
- `algorithm`: Hash algorithm (SHA-256)
- `challenge`: SHA-256 hash of salt
- `salt`: Random salt with expiration (20 minutes)
- `signature`: HMAC-SHA256(challenge, secret_key)
- `maxnumber`: Proof-of-work difficulty (0-50000)

#### External Altcha API (Optional)

Use hosted Altcha service:

```toml
[sessions.dashboard_auth]
altcha_enabled = true
altcha_challenge_url = "https://eu.altcha.org/api/v1/challenge?apiKey=YOUR_KEY"
```

Or self-host Altcha server:

```toml
altcha_challenge_url = "http://your-altcha-server.com/api/challenge"
```

### Difficulty Configuration

Adjust `maxnumber` in `src/api/auth.rs`:

```rust
maxnumber: Some(50000),  // Difficulty: 0-50000
```

**Recommended values**:
- `10000` - Easy (fast, less security)
- `50000` - Medium (default, balanced)
- `100000` - Hard (slow, more security)
- `250000` - Very hard (very slow, maximum security)

**Note**: Higher values increase solve time but provide stronger bot protection.

## Security Best Practices

### 1. Session Secret

**❌ Bad**:
```toml
session_secret = "secret123"
```

**✅ Good**:
```toml
session_secret = "j8Kx9m2Nq5Pv7Yt3Zw1Bc4Df6Gh8Jl0Mn2Op4Qr6St8Uv0Wx2Yz4Ab6Cd8Ef0"
```

**Generate secure secret**:
```bash
openssl rand -base64 48
```

### 2. Strong Passwords

**Minimum requirements**:
- 12+ characters
- Mix of uppercase, lowercase, numbers, symbols
- No dictionary words
- Unique per user

### 3. HTTPS in Production

Always use HTTPS when deploying dashboard:

```toml
[server.tls]
enabled = true
certificate_path = "/path/to/cert.pem"
private_key_path = "/path/to/key.pem"
```

### 4. Session Duration

Adjust based on security needs:

```toml
session_duration_hours = 8  # Work day
# session_duration_hours = 1  # High security
# session_duration_hours = 168  # 1 week (convenience)
```

### 5. Firewall Protection

Restrict dashboard access:

```bash
# Allow only from specific IP
iptables -A INPUT -p tcp -s 192.168.1.0/24 --dport 9090 -j ACCEPT
iptables -A INPUT -p tcp --dport 9090 -j DROP
```

Or use reverse proxy (nginx/Apache) with IP whitelist.

## Troubleshooting

### Login Page Not Showing

**Symptoms**: Browser shows basic auth popup instead of login page

**Solution**: Check config:
```toml
[sessions.dashboard_auth]
enabled = true  # Must be true
```

Rebuild dashboard if modified:
```bash
cd dashboard && npm run build
```

### Altcha Widget Not Loading

**Check browser console** for errors

**Common issues**:

1. **JavaScript disabled** - Enable JavaScript
2. **HTTPS required** - Altcha uses Web Crypto API (requires secure context)
3. **Old browser** - Use modern browser with Web Components support

**Test Altcha endpoint**:
```bash
curl http://127.0.0.1:9090/api/auth/altcha-challenge
```

Should return JSON challenge.

### Session Expires Immediately

**Check**:
1. System clock - ensure server time is correct
2. Session secret - verify it's set correctly
3. Cookie settings - check browser accepts cookies

**Debug**:
```bash
# Check server logs for session creation
grep "User logged in" /var/log/rustsocks.log
```

### Base Path Issues

**Symptoms**: Login works at root but not at `/rustsocks`

**Solution**: Verify base path consistency:

```toml
[sessions]
base_path = "/rustsocks"  # Must match deployment
```

Rebuild dashboard:
```bash
cd dashboard && npm run build
```

### "Invalid credentials" Error

**Check**:
1. Username/password in config match exactly (case-sensitive)
2. No extra spaces in config
3. TOML syntax is correct

**Test credentials**:
```bash
# Print configured users (excluding passwords)
grep -A1 'dashboard_auth.users' config/rustsocks.toml
```

## API Endpoints

### POST /api/auth/login

Login with credentials.

**Request**:
```json
{
  "username": "admin",
  "password": "password123",
  "altcha": "optional_altcha_payload"
}
```

**Response (success)**:
```json
{
  "success": true,
  "message": "Login successful",
  "username": "admin"
}
```

**Sets cookie**: `rustsocks_session=<token>; HttpOnly; SameSite=Strict`

### POST /api/auth/logout

Logout and destroy session.

**Response**:
```json
{
  "success": true,
  "message": "Logged out successfully"
}
```

### GET /api/auth/check

Check if current session is valid.

**Response (authenticated)**:
```json
{
  "authenticated": true,
  "username": "admin"
}
```

**Response (not authenticated)**:
```json
{
  "authenticated": false
}
```

### GET /api/auth/altcha-config

Get Altcha configuration.

**Response**:
```json
{
  "enabled": true,
  "challenge_url": null
}
```

### GET /api/auth/altcha-challenge

Generate Altcha challenge (when enabled).

**Response**:
```json
{
  "algorithm": "SHA-256",
  "challenge": "a1b2c3...",
  "salt": "xyz123?expires=1234567890",
  "signature": "9f8e7d...",
  "maxnumber": 50000
}
```

## Example Configurations

### Simple Auth (No CAPTCHA)

```toml
[sessions]
enabled = true
dashboard_enabled = true

[sessions.dashboard_auth]
enabled = true
altcha_enabled = false
session_secret = "your-secret-here"

[[sessions.dashboard_auth.users]]
username = "admin"
password = "AdminPass123"
```

### With Altcha (Self-Hosted)

```toml
[sessions]
enabled = true
dashboard_enabled = true

[sessions.dashboard_auth]
enabled = true
altcha_enabled = true  # Enable CAPTCHA
session_secret = "your-secret-for-hmac-signing"

[[sessions.dashboard_auth.users]]
username = "admin"
password = "AdminPass123"
```

### With Base Path

```toml
[sessions]
enabled = true
dashboard_enabled = true
base_path = "/socks"  # Dashboard at /socks

[sessions.dashboard_auth]
enabled = true
session_secret = "your-secret-here"

[[sessions.dashboard_auth.users]]
username = "admin"
password = "AdminPass123"
```

Access at: `http://127.0.0.1:9090/socks`

## Future Enhancements

Planned features for future versions:

- [ ] Password hashing (bcrypt/argon2)
- [ ] Two-factor authentication (TOTP)
- [ ] Role-based access control (RBAC)
- [ ] LDAP/Active Directory integration
- [ ] API key authentication
- [ ] Audit logging
- [ ] Brute-force protection
- [ ] Session management UI

---

**Need help?** Check [README.md](../../README.md) or open an issue on GitHub.
