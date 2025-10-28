# LDAP Groups Integration - Complete Guide

## Overview

RustSocks now supports **dynamic LDAP group resolution** inspired by Dante SOCKS server. This allows ACL rules to be applied based on user's LDAP groups without manual synchronization.

**Key Feature:** ACL only checks groups **defined in ACL config** - even if user has thousands of LDAP groups, only the relevant ones are evaluated (case-insensitive).

---

## How It Works

### 1. User Authentication (PAM)

```
User "alice" → PAM authentication (pam_mysql.so) → Success
```

### 2. LDAP Group Resolution (NSS/SSSD)

```
System fetches ALL user's groups from LDAP via getgrouplist():
["alice", "developers", "engineering", "hr", "team_foo", "team_bar", ...]
```

### 3. ACL Group Filtering

```
ACL config defines ONLY:
  [[groups]]
  name = "developers"

ACL checks: Is "developers" in user's LDAP groups? → YES
ACL ignores: engineering, hr, team_foo, team_bar, ... (not in ACL config)
```

### 4. Rule Application

```
Apply rules from "developers" group → ALLOW/BLOCK connection
```

---

## Configuration

### rustsocks.toml

```toml
[server]
bind_address = "0.0.0.0"
bind_port = 1080

[auth]
client_method = "none"           # No auth before SOCKS
socks_method = "pam.username"    # PAM username/password auth

[auth.pam]
username_service = "sockd"       # Your PAM service (e.g., pam_mysql.so)
address_service = "rustsocks-client"
default_user = "rhostusr"
verbose = true
verify_service = true

[acl]
enabled = true
config_file = "config/acl.toml"
watch = true
anonymous_user = "anonymous"
```

### acl.toml (Simple - No User Mappings!)

```toml
[global]
default_policy = "block"  # Block if user has no matching groups

# Define ONLY the groups you care about (not all LDAP groups!)
[[groups]]
name = "developers"  # Case-insensitive: "Developers", "DEVELOPERS" also match

  [[groups.rules]]
  action = "allow"
  description = "Developers access to internal dev servers"
  destinations = ["*.dev.company.com", "10.0.0.0/8", "192.168.0.0/16"]
  ports = ["*"]
  protocols = ["tcp", "udp"]
  priority = 100

  [[groups.rules]]
  action = "block"
  description = "Block developers from production"
  destinations = ["*.prod.company.com", "172.16.0.0/12"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 200  # Higher priority = checked first

[[groups]]
name = "admins"

  [[groups.rules]]
  action = "allow"
  description = "Admins full access"
  destinations = ["*"]
  ports = ["*"]
  protocols = ["tcp", "udp"]
  priority = 100

# Optional: Per-user overrides (if needed)
[[users]]
username = "alice"
# No need to specify groups - they come from LDAP automatically!

  [[users.rules]]
  action = "block"
  description = "Alice blocked from specific admin panel"
  destinations = ["admin.company.com"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 1000  # Overrides group rules
```

---

## Example Scenarios

### Scenario 1: User with Multiple LDAP Groups

**LDAP groups for user "alice":**
```
["alice", "developers", "engineering", "hr", "team_marketing", ...]
```

**ACL config defines:**
```toml
[[groups]]
name = "developers"  # ← Only this one
```

**Result:**
- ACL checks if "developers" is in alice's groups → **YES**
- Applies "developers" rules
- Ignores: engineering, hr, team_marketing, ... (not in ACL)

**Connection test:**
```bash
curl -x socks5://alice:password@127.0.0.1:1080 http://api.dev.company.com
```

**Logs:**
```
INFO PAM authentication successful user=alice
INFO User authenticated with groups from LDAP user=alice group_count=50
DEBUG Matched LDAP group to ACL group (case-insensitive) ldap_group=developers acl_group=developers
INFO ACL allowed connection user=alice rule="Developers access to internal dev servers"
```

---

### Scenario 2: User with No Matching Groups

**LDAP groups for user "bob":**
```
["bob", "hr", "finance", "team_sales"]
```

**ACL config defines:**
```toml
[[groups]]
name = "developers"
[[groups]]
name = "admins"
```

**Result:**
- ACL checks: "hr" in ACL? → NO
- ACL checks: "finance" in ACL? → NO
- ACL checks: "team_sales" in ACL? → NO
- No groups match → Apply `default_policy = "block"`

**Connection test:**
```bash
curl -x socks5://bob:password@127.0.0.1:1080 http://api.dev.company.com
```

**Logs:**
```
INFO PAM authentication successful user=bob
DEBUG User groups from LDAP: ["bob", "hr", "finance", "team_sales"]
WARN No ACL rules matched for user groups, applying default policy
INFO ACL blocked connection user=bob rule="Default policy (no matching groups)"
```

---

### Scenario 3: Case-Insensitive Matching

**LDAP groups:** `["Developers"]` (capital D)
**ACL config:** `name = "developers"` (lowercase)

**Result:** ✅ **MATCH** (case-insensitive)

---

### Scenario 4: User in Multiple Defined Groups

**LDAP groups:** `["charlie", "developers", "admins"]`
**ACL config:** Both "developers" and "admins" defined

**Result:**
- Both groups' rules are collected
- Rules sorted by priority (BLOCK first, then higher priority)
- First matching rule wins

---

## System Requirements

### 1. NSS/SSSD Configuration

Your system must have LDAP configured via NSS/SSSD:

```bash
# Test if LDAP users are visible
getent passwd alice

# Test if LDAP groups are visible
getent group developers

# Test group membership
id alice
# Output: uid=1001(alice) gid=1001(alice) groups=1001(alice),2001(developers),2002(engineering)
```

### 2. PAM Configuration

Requires `/etc/pam.d/sockd` (or your custom service):

```
#%PAM-1.0
auth required /usr/local/lib64/security/pam_mysql.so config_file=/opt/nsnras/config/pam-mysql.conf
account sufficient /lib64/security/pam_sss.so
account required /lib64/security/pam_unix_acct.so
```

### 3. /etc/nsswitch.conf

Ensure SSSD is enabled for group lookups:

```
passwd:     files sss
group:      files sss
shadow:     files sss
```

---

## Deployment Steps

### 1. Install Dependencies

```bash
# Development headers (if building from source)
sudo apt-get install libpam0g-dev  # Debian/Ubuntu
sudo dnf install pam-devel gcc nodejs rust cargo  # RHEL/CentOS

# SSSD (if not already installed)
sudo apt-get install sssd sssd-tools
```

### 2. Configure SSSD for LDAP

```bash
# Example /etc/sssd/sssd.conf
[sssd]
config_file_version = 2
services = nss, pam
domains = LDAP

[domain/LDAP]
id_provider = ldap
auth_provider = ldap
ldap_uri = ldap://ldap.company.com
ldap_search_base = dc=company,dc=com

# Restart SSSD
sudo systemctl restart sssd
```

### 3. Configure PAM Service

```bash
# Copy your PAM service file
sudo cp /path/to/sockd /etc/pam.d/sockd
sudo chmod 644 /etc/pam.d/sockd
```

### 4. Configure RustSocks

```bash
# Edit configs
vim config/rustsocks.toml  # Set socks_method = "pam.username", username_service = "sockd"
vim config/acl.toml        # Define your LDAP groups (ONLY the ones you need)
```

### 5. Build and Run

```bash
cargo build --release

# Run (may need sudo for PAM)
sudo ./target/release/rustsocks --config config/rustsocks.toml
```

---

## Testing

### 1. Test LDAP Group Resolution

```bash
# Run unit tests (no LDAP required)
cargo test --all-features --lib groups

# Expected: 3 tests passed (including mock tests)
```

### 2. Test ACL with Mock LDAP Groups

```bash
# Integration tests use mock groups
cargo test --all-features --test ldap_groups

# Expected: 7 tests passed
# - test_ldap_groups_only_defined_groups_are_checked
# - test_ldap_groups_no_matching_groups_uses_default_policy
# - test_ldap_groups_case_insensitive_matching
# - test_ldap_groups_multiple_matching_groups
# - test_ldap_groups_with_per_user_override
# - test_ldap_groups_empty_groups_list
# - test_ldap_groups_mixed_case_variations
```

### 3. Test Real LDAP Integration

```bash
# Requires real LDAP user "alice" in group "developers"
cargo test --all-features --lib groups::tests::test_get_user_groups_current_user -- --ignored
```

### 4. Test End-to-End

```bash
# Start server
sudo ./target/release/rustsocks --config config/rustsocks.toml

# In another terminal, test with real LDAP user
curl -x socks5://alice:password@127.0.0.1:1080 -v http://api.dev.company.com

# Check logs for:
# - PAM authentication successful
# - User authenticated with groups from LDAP
# - Matched LDAP group to ACL group
# - ACL allowed/blocked connection
```

---

## Troubleshooting

### Problem: Groups not found via getgrouplist()

**Symptoms:**
```
WARN Failed to retrieve user groups from system, using empty list
```

**Solution:**
```bash
# Check SSSD status
sudo systemctl status sssd

# Check if LDAP groups are visible
getent group developers

# Check user's groups
id username

# Restart SSSD
sudo systemctl restart sssd

# Check SSSD logs
sudo tail -f /var/log/sssd/sssd_LDAP.log
```

### Problem: PAM authentication fails

**Symptoms:**
```
WARN PAM authentication failed user=alice error=AuthFailed
```

**Solution:**
```bash
# Test PAM manually
pamtester sockd alice authenticate

# Check PAM service file
ls -la /etc/pam.d/sockd

# Check PAM logs
sudo tail -f /var/log/auth.log  # Debian/Ubuntu
sudo tail -f /var/log/secure    # RHEL/CentOS
```

### Problem: ACL doesn't match groups

**Symptoms:**
```
DEBUG User groups from LDAP: ["developers", "engineering"]
WARN No ACL rules matched for user groups
```

**Solution:**
```bash
# Check ACL config
cat config/acl.toml | grep -A 5 "\[\[groups\]\]"

# Ensure group names match (case-insensitive)
# LDAP: "Developers" = ACL: "developers" ✅
# LDAP: "developers" ≠ ACL: "devs" ❌

# Reload ACL (if hot reload enabled)
curl -X POST http://127.0.0.1:9090/api/admin/reload-acl
```

---

## Performance Considerations

### getgrouplist() Overhead

- **Called once per authentication** (cached during session)
- Typical latency: **1-5ms** for local NSS cache
- Up to **50-100ms** if LDAP lookup required
- **Recommendation**: Ensure SSSD caching is enabled

### ACL Evaluation Overhead

- **Filtering 1000 LDAP groups:** <1ms (only checks defined groups)
- **Evaluating 10 ACL rules:** <1ms
- **Total overhead:** ~5-10ms per connection (acceptable)

---

## Security Best Practices

1. **Use TLS for SOCKS5 traffic** - Password transmitted in clear-text
2. **Restrict PAM service** - Only allow trusted modules
3. **Monitor failed authentications** - Watch `/var/log/auth.log`
4. **Use BLOCK as default policy** - Deny by default, allow explicitly
5. **Regular ACL audits** - Review group access periodically
6. **SSSD caching** - Reduce LDAP query load

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  SOCKS5 Client                                                  │
│  curl -x socks5://alice:password@proxy:1080 http://dest.com    │
└──────────────────────┬──────────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────────┐
│  RustSocks Server (handler.rs)                                   │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ 1. SOCKS5 Handshake                                         ││
│  │ 2. Authentication (PAM)                                     ││
│  │    └─> AuthManager::authenticate()                          ││
│  └────────────────────┬────────────────────────────────────────┘│
│                       │                                          │
│                       ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ groups.rs: get_user_groups("alice")                         ││
│  │    ├─> getgrouplist() syscall                               ││
│  │    └─> Returns: ["alice", "developers", "engineering", ...] ││
│  └────────────────────┬────────────────────────────────────────┘│
│                       │                                          │
│                       ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ ACL Engine: evaluate_with_groups()                          ││
│  │    ├─> Filter LDAP groups (only "developers" defined)       ││
│  │    ├─> Collect rules from "developers" group                ││
│  │    ├─> Sort rules (BLOCK first, then by priority)           ││
│  │    └─> Match destination/port/protocol → ALLOW/BLOCK        ││
│  └────────────────────┬────────────────────────────────────────┘│
│                       │                                          │
│                       ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ 3. Connect to destination (if ALLOWED)                       ││
│  │ 4. Proxy data                                                ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                       ▲
                       │
┌──────────────────────┴──────────────────────────────────────────┐
│  External Systems                                                │
│  ┌───────────────┐  ┌─────────────┐  ┌──────────────────────┐  │
│  │ /etc/pam.d/   │  │ NSS/SSSD    │  │ LDAP Server          │  │
│  │ sockd         │  │ (getgrouplist)│ │ ldap.company.com     │  │
│  │ (pam_mysql.so)│  │             │  │ - Users              │  │
│  │               │  │             │  │ - Groups             │  │
│  └───────────────┘  └─────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## FAQ

### Q: Do I need to define ALL LDAP groups in ACL config?

**A:** NO! Only define groups you want to grant/block access to. Other groups are ignored.

### Q: What if user has 1000+ LDAP groups?

**A:** No problem. ACL only checks groups defined in config. Performance impact is minimal (<1ms filtering).

### Q: Are group names case-sensitive?

**A:** NO. "developers" = "Developers" = "DEVELOPERS" (case-insensitive matching).

### Q: Can I use per-user overrides?

**A:** YES. Define `[[users]]` with custom rules. They override group rules (higher priority).

### Q: Does this work without LDAP?

**A:** YES. Works with any NSS-compatible system (LDAP, NIS, local files, etc.). Just needs `getgrouplist()`.

### Q: What happens if LDAP is down?

**A:** `get_user_groups()` fails → empty groups list → default_policy applies (usually BLOCK).

---

## Summary

✅ **Zero manual synchronization** - Groups fetched automatically from LDAP
✅ **Efficient** - Only checks groups defined in ACL config
✅ **Case-insensitive** - Works regardless of case in LDAP
✅ **Flexible** - Supports per-user overrides
✅ **Secure** - Default BLOCK policy for unmatched groups
✅ **Production-ready** - Comprehensive error handling and logging

**Ready to deploy!** 🚀
