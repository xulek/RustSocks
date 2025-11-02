# PAM Configuration Files for RustSocks

This directory contains example PAM configuration files for RustSocks SOCKS5 server.

## Overview

RustSocks supports two PAM authentication methods:

1. **pam.address** - IP/hostname-based authentication (no username/password)
2. **pam.username** - Traditional username/password authentication via SOCKS5

## Installation

### 1. Copy PAM service files to system directory

```bash
# For production use
sudo cp config/pam.d/rustsocks /etc/pam.d/rustsocks
sudo cp config/pam.d/rustsocks-client /etc/pam.d/rustsocks-client

# For testing
sudo cp config/pam.d/rustsocks-test /etc/pam.d/rustsocks-test
sudo cp config/pam.d/rustsocks-client-test /etc/pam.d/rustsocks-client-test
```

### 2. Set proper permissions

```bash
sudo chmod 644 /etc/pam.d/rustsocks*
sudo chown root:root /etc/pam.d/rustsocks*
```

### 3. Verify PAM configuration

```bash
# Test username/password authentication (requires pamtester)
sudo apt-get install pamtester  # Debian/Ubuntu
pamtester rustsocks <username> authenticate

# Check PAM logs
sudo tail -f /var/log/auth.log   # Debian/Ubuntu
sudo tail -f /var/log/secure     # RHEL/CentOS
```

## File Descriptions

### rustsocks

Main PAM service for SOCKS5 username/password authentication.

- **Used by**: `socks_method = "pam.username"`
- **Service name**: Configured in `auth.pam.username_service`
- **Authentication**: Username + password via SOCKS5 protocol (RFC 1929)
- **Default**: Uses `pam_unix.so` for system user authentication

⚠️ **Security Warning**: SOCKS5 username/password transmits credentials in clear-text over the network. Only use in trusted networks or with additional encryption (e.g., SSH tunnel, VPN, TLS wrapper).

### rustsocks-client

PAM service for client IP-based authentication (before SOCKS handshake).

- **Used by**: `client_method = "pam.address"`
- **Service name**: Configured in `auth.pam.address_service`
- **Authentication**: IP address only, no password
- **Default**: Uses `pam_permit.so` (allows all) - customize for your needs

**Common configurations**:
- `pam_permit.so` - Allow all (testing/internal networks)
- `pam_rhosts.so` - Use `.rhosts`/`.shosts` files
- `pam_access.so` - Use `/etc/security/access.conf` for IP-based ACLs
- Custom PAM module - Implement your own IP validation logic

### rustsocks-test / rustsocks-client-test

Permissive PAM configurations for integration tests.

- **Used by**: `tests/pam_integration.rs`
- **Authentication**: Always succeeds (`pam_permit.so`)
- **DO NOT USE IN PRODUCTION**

## Configuration in rustsocks.toml

### Example 1: PAM username/password authentication

```toml
[auth]
client_method = "none"
socks_method = "pam.username"

[auth.pam]
username_service = "rustsocks"
default_user = "rhostusr"
verbose = false
verify_service = true
```

### Example 2: PAM IP-based authentication (client-level)

```toml
[auth]
client_method = "pam.address"
socks_method = "none"

[auth.pam]
address_service = "rustsocks-client"
default_user = "rhostusr"
verbose = false
verify_service = true
```

### Example 3: Both PAM methods (defense in depth)

```toml
[auth]
client_method = "pam.address"      # IP filtering before SOCKS
socks_method = "pam.username"      # Username/password after SOCKS

[auth.pam]
username_service = "rustsocks"
address_service = "rustsocks-client"
default_user = "rhostusr"
verbose = true
verify_service = true
```

## Privilege Requirements

PAM authentication requires specific privileges:

### Running as root

PAM typically requires root privileges to:
- Read `/etc/shadow` for password verification
- Call PAM modules that need elevated permissions

**Recommended approach**:
1. Start server as root
2. Bind to privileged port (if needed)
3. Drop privileges after binding
4. PAM operations still work via saved UID

```bash
# Start as root, server will drop privileges after binding
sudo ./rustsocks --config config/rustsocks.toml
```

### Running as non-root

Some PAM configurations (e.g., `pam_permit.so`) work without root:

```bash
# Non-privileged mode (limited PAM functionality)
./rustsocks --config config/rustsocks.toml --bind 0.0.0.0 --port 1080
```

## Troubleshooting

### Authentication always fails

```bash
# Check PAM logs
sudo tail -f /var/log/auth.log

# Common issues:
# 1. Service file not found
ls -la /etc/pam.d/rustsocks

# 2. Permission denied on /etc/shadow
sudo ls -la /etc/shadow

# 3. User doesn't exist
id username

# 4. Account locked
sudo passwd -S username
```

### PAM module not found

```bash
# Install PAM development packages
sudo apt-get install libpam0g-dev    # Debian/Ubuntu
sudo dnf install pam-devel gcc nodejs rust cargo  # RHEL/CentOS
sudo pacman -S pam                   # Arch Linux

# Verify PAM library
ls -la /usr/lib/x86_64-linux-gnu/libpam.so*
```

### Test PAM authentication manually

```bash
# Install pamtester
sudo apt-get install pamtester

# Test authentication
pamtester rustsocks username authenticate
# Enter password when prompted

# Test with specific user
pamtester rustsocks alice authenticate

# Verbose mode
pamtester -v rustsocks alice authenticate
```

## Security Best Practices

1. **Use pam.username only in trusted networks** - Password transmitted in clear-text
2. **Combine with TLS/VPN** - Encrypt SOCKS5 traffic
3. **Monitor PAM logs** - Watch `/var/log/auth.log` for failed attempts
4. **Implement account lockout** - Use `pam_faillock.so` to prevent brute-force
5. **Use ACLs in addition to PAM** - Defense in depth with RustSocks ACL engine
6. **Regular security audits** - Review PAM configurations and user accounts

## Additional Resources

- [Linux PAM Documentation](http://www.linux-pam.org/Linux-PAM-html/)
- [pam_unix.so manual](https://linux.die.net/man/8/pam_unix)
- [pam_access.so manual](https://linux.die.net/man/8/pam_access)
- [RustSocks Documentation](../CLAUDE.md)

## License

These example configurations are provided as-is for reference purposes.
Customize according to your security requirements.
