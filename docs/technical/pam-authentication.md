# PAM Authentication - Implementation Guide

This document explains the PAM (Pluggable Authentication Modules) integration in RustSocks.

## Overview

RustSocks supports two PAM authentication methods:

1. **pam.address** - Authentication based on IP/hostname only (no username/password)
   - Used in client-level rules (before SOCKS handshake)
   - Useful for trusted networks
   - Example: PAM rhosts module

2. **pam.username** - Authentication with username and password
   - Used in SOCKS-level rules (after SOCKS handshake)
   - Standard SOCKS5 username/password (RFC 1929)
   - ⚠️ Password transmitted in clear-text

## System Requirements

### Build-Time Requirements
```bash
# Debian/Ubuntu
sudo apt-get install libpam0g-dev

# RHEL/CentOS/Fedora
sudo dnf install pam-devel gcc

# Arch Linux
sudo pacman -S pam

# Verification
ls -la /usr/include/security/pam_appl.h
ls -la /usr/lib/x86_64-linux-gnu/libpam.so
```

### Runtime Requirements
- Server must start as root (for PAM verification)
- PAM configuration file: `/etc/pam.d/rustsocks` (or custom)
- Unprivileged user for privilege dropping: `socks`

### Dependencies in Cargo.toml
```toml
[target.'cfg(unix)'.dependencies]
pam = "0.7"           # PAM bindings
nix = { version = "0.27", features = ["user"] }
libc = "0.2"          # For PAM syscalls
```

## Core Implementation

### PAM Configuration

```toml
# config/rustsocks.toml

[server]
bind_address = "0.0.0.0"
bind_port = 1080

[auth]
# Client-level auth (before SOCKS handshake)
client_method = "none"        # or "pam.address"
client_pam_service = "rustsocks-client"

# SOCKS-level auth (after SOCKS handshake)
socks_method = "pam.username" # or "pam.address", "userpass", "none"
socks_pam_service = "rustsocks"

[auth.pam]
# Default user for pam.address
default_user = "rhostusr"
default_ruser = "rhostusr"

# Enable verbose PAM logging
verbose = false

# Verify PAM service files exist at startup
verify_service = false
```

### PAM Service Files

**`/etc/pam.d/rustsocks` (for socks-rules - username/password auth)**
```bash
%PAM-1.0

# Authentication
auth       required     pam_unix.so
auth       required     pam_env.so

# Account management
account    required     pam_unix.so
account    required     pam_nologin.so

# Session management (optional)
session    optional     pam_limits.so
```

**`/etc/pam.d/rustsocks-client` (for client-rules - IP-based auth)**
```bash
%PAM-1.0

# Use pam_rhosts or similar for IP-based auth
auth       required     pam_rhosts.so
account    required     pam_permit.so
```

### Installation

```bash
# Copy PAM service files to system directory
sudo cp config/pam.d/rustsocks /etc/pam.d/rustsocks
sudo cp config/pam.d/rustsocks-client /etc/pam.d/rustsocks-client

# Set correct permissions
sudo chmod 644 /etc/pam.d/rustsocks*

# Verify
sudo pamtester rustsocks username authenticate
```

## Two-Tier Authentication

RustSocks supports dual authentication for defense in depth:

### Client-Level Authentication (Before SOCKS)

Performed before SOCKS handshake when client connects:

```toml
[auth]
client_method = "pam.address"    # Authenticate based on IP
client_pam_service = "rustsocks-client"
```

Benefits:
- Early rejection of unauthorized IPs
- Prevents SOCKS negotiation with untrusted clients
- Lower latency (fails fast)

### SOCKS-Level Authentication (After SOCKS)

Performed after SOCKS handshake:

```toml
[auth]
socks_method = "pam.username"    # Authenticate with username/password
socks_pam_service = "rustsocks"
```

### Combined (Defense in Depth)

```toml
[auth]
client_method = "pam.address"     # IP filtering
socks_method = "pam.username"     # Username/password
```

This requires:
1. Client IP must pass PAM pam_rhosts check
2. AND username/password must pass PAM pam_unix check

## API Endpoints

PAM integration provides REST endpoints:

```bash
# Get current auth method
curl http://127.0.0.1:9090/api/auth/method

# Get PAM status
curl http://127.0.0.1:9090/api/auth/pam/status

# Test PAM authentication (requires valid credentials)
curl -X POST http://127.0.0.1:9090/api/auth/pam/test \
  -H "Content-Type: application/json" \
  -d '{"username": "testuser", "password": "testpass"}'
```

## Monitoring & Logging

PAM authentication attempts are logged and metered:

```bash
# View PAM logs
sudo tail -f /var/log/auth.log

# Check PAM metrics
curl http://127.0.0.1:9090/metrics | grep pam
```

Expected metrics:
- `rustsocks_pam_auth_total{method,service,result}` - Authentication attempts
- `rustsocks_pam_auth_duration_seconds{method,service}` - Auth latency

## Testing PAM Setup

### Manual Testing

```bash
# Test PAM service
sudo pamtester rustsocks username authenticate
# Output: pamtester: successfully authenticated

# Test PAM service fails on wrong password
sudo pamtester rustsocks username authenticate
# (enter wrong password)
# Output: pamtester: Authentication failure
```

### Integration Testing

```bash
# Run PAM tests
cargo test --all-features pam -- --ignored

# Tests included:
# - pam.address authentication
# - pam.username authentication
# - Cross-platform compatibility
# - Error handling
```

## Security Considerations

### 1. Password Transmission
- ⚠️ **CRITICAL**: SOCKS5 username/password transmits credentials in **clear-text**
- Use only in trusted networks
- Recommended: Wrap with SOCKS over TLS (future feature)

### 2. Privilege Management
- Server must start as **root** for PAM access
- **Drop privileges IMMEDIATELY** after binding socket
- Verify privilege drop succeeded
- Never run as root during request handling

### 3. PAM Service Configuration
- **CRITICAL**: Non-existent PAM service may allow all traffic on some systems!
- Always verify PAM config exists: `/etc/pam.d/<service>`
- Test both successful and failed authentication
- Monitor PAM logs: `/var/log/auth.log` or `/var/log/secure`

### 4. Defense in Depth

Combine PAM with other security layers:
```toml
[auth]
client_method = "pam.address"     # IP filtering
socks_method = "pam.username"     # Username check

[acl]
enabled = true                     # ACL rules
watch = true                       # Hot reload
```

## Troubleshooting

### PAM Authentication Always Fails

```bash
# Check PAM logs
sudo tail -f /var/log/auth.log

# Check service file exists
ls -la /etc/pam.d/rustsocks

# Test manually
pamtester rustsocks username authenticate

# Check permissions
ls -la /etc/shadow  # Must be readable by root
```

### Permission Denied Errors

```bash
# Check if running as root
id

# Check user exists
id socks

# Verify user can be created
useradd -r -s /bin/false socks

# Test privilege drop
sudo ./target/release/rustsocks --config config/rustsocks.toml
```

### PAM Module Not Found

```bash
# Install PAM development packages
sudo apt-get install libpam0g-dev

# Recompile
cargo clean
cargo build --release
```

## Performance

PAM authentication typically adds minimal overhead:

- **pam.address**: 1-5ms (depends on PAM service)
- **pam.username**: 10-50ms (depends on auth backend)
- Background: Runs in `spawn_blocking` to avoid blocking async runtime

## Best Practices

1. **Use pam.address for IP filtering**
   - Fast (1-5ms)
   - No credentials required
   - Good for trusted networks

2. **Use pam.username for user-based access**
   - Slower (10-50ms) but more flexible
   - Use PAM pam_unix for system authentication
   - Consider LDAP integration for large deployments

3. **Combine both for defense in depth**
   - IP filter first (fails fast)
   - Then username/password (second factor)

4. **Monitor and log**
   ```bash
   # Enable debug logging
   log_level = "debug"

   # Monitor PAM logs
   sudo tail -f /var/log/auth.log | grep rustsocks
   ```

5. **Verify configuration**
   ```bash
   # At startup
   [auth.pam]
   verify_service = true  # Check PAM files exist
   ```

## Example Deployment

### Step 1: Create PAM Service Files

```bash
# /etc/pam.d/rustsocks
%PAM-1.0
auth       required     pam_unix.so
account    required     pam_unix.so
session    optional     pam_limits.so
```

### Step 2: Configure RustSocks

```toml
[auth]
socks_method = "pam.username"
socks_pam_service = "rustsocks"

[auth.pam]
default_user = "rhostusr"
verbose = true
verify_service = true
```

### Step 3: Create Unprivileged User

```bash
# Create socks user
sudo useradd -r -s /bin/false socks

# Check user exists
id socks
```

### Step 4: Run Server

```bash
# Start as root
sudo ./target/release/rustsocks --config config/rustsocks.toml

# Server will:
# 1. Load PAM service "rustsocks"
# 2. Bind socket (requires root)
# 3. Drop privileges to 'socks' user
# 4. Accept connections as unprivileged user
```

### Step 5: Test Authentication

```bash
# Test with valid system account
curl -x socks5://someuser:somepass@127.0.0.1:1080 http://example.com

# Monitor logs
sudo tail -f /var/log/auth.log

# Check RustSocks logs
tail -f rustsocks.log
```

## Cross-Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| Linux | ✅ Full | PAM fully supported |
| BSD | ✅ Full | PAM available on most BSD systems |
| macOS | ⚠️ Limited | OpenPAM available, limited by system restrictions |
| Windows | ❌ None | No PAM equivalent, use LDAP or custom auth |

## Summary

RustSocks PAM authentication provides:

✅ **pam.address** - IP-only auth for trusted networks
✅ **pam.username** - Username/password auth via SOCKS5
✅ **Two-tier authentication** - Defense in depth
✅ **Privilege dropping** - Secure privilege management
✅ **Production-ready** - Comprehensive error handling and logging
✅ **Testable** - Unit and integration tests

---

**Last Updated:** 2025-11-02
**Version:** 0.9
**Status:** ✅ Production Ready
