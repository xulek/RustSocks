# Active Directory Integration Guide

**RustSocks** supports full integration with **Microsoft Active Directory** for authentication and group-based access control. This guide provides step-by-step instructions for configuring RustSocks to work with your Windows AD environment.

## Table of Contents

1. [Overview](#overview)
2. [How It Works](#how-it-works)
3. [Prerequisites](#prerequisites)
4. [System Preparation](#system-preparation)
5. [Joining AD Domain](#joining-ad-domain)
6. [SSSD Configuration](#sssd-configuration)
7. [Kerberos Configuration](#kerberos-configuration)
8. [NSS Configuration](#nss-configuration)
9. [PAM Configuration](#pam-configuration)
10. [RustSocks Configuration](#rustsocks-configuration)
11. [Testing & Verification](#testing--verification)
12. [ACL Rules with AD Groups](#acl-rules-with-ad-groups)
13. [Troubleshooting](#troubleshooting)
14. [Production Deployment](#production-deployment)
15. [Security Best Practices](#security-best-practices)
16. [FAQ](#faq)
17. [Appendix](#appendix)

---

## Overview

RustSocks integrates with Active Directory through the **System Security Services Daemon (SSSD)** and **PAM (Pluggable Authentication Modules)**. This architecture provides:

- **Native AD Support**: No custom LDAP client needed
- **Kerberos Authentication**: Secure, encrypted authentication
- **Group-Based ACL**: Control access using AD security groups
- **Automatic Group Resolution**: Groups retrieved from AD automatically
- **High Performance**: SSSD caching reduces AD queries
- **Battle-Tested**: Uses standard Linux authentication stack

### Supported AD Versions

- Windows Server 2012 R2 and later
- Windows Server 2016
- Windows Server 2019
- Windows Server 2022
- Azure Active Directory Domain Services (Azure AD DS)

### Supported Linux Distributions

- Red Hat Enterprise Linux 7, 8, 9
- CentOS 7, 8 Stream
- Rocky Linux 8, 9
- AlmaLinux 8, 9
- Ubuntu 18.04, 20.04, 22.04, 24.04
- Debian 10, 11, 12
- SUSE Linux Enterprise Server 12, 15

---

## How It Works

### Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│ Windows Active Directory Server                             │
│  - User accounts (alice@COMPANY.COM)                        │
│  - Security groups (CN=Developers, CN=Admins)              │
│  - Group Policy Objects                                     │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            │ LDAP + Kerberos
                            │ TCP 389, 636 (LDAPS), 88 (Kerberos)
                            │
┌───────────────────────────▼─────────────────────────────────┐
│ Linux Server (RustSocks Proxy)                              │
│                                                              │
│  ┌────────────────────────────────────────────────────┐    │
│  │ RustSocks SOCKS5 Proxy                             │    │
│  │  - Accepts client connections                      │    │
│  │  - Calls PAM for authentication                    │    │
│  │  - Retrieves user's AD groups                      │    │
│  │  - Evaluates ACL rules based on groups            │    │
│  │  - Allows/blocks connections                       │    │
│  └──────────────────┬──────────────────────────────────┘    │
│                     │                                        │
│                     ▼                                        │
│  ┌────────────────────────────────────────────────────┐    │
│  │ PAM (Pluggable Authentication Modules)             │    │
│  │  - pam_sss module for AD authentication            │    │
│  └──────────────────┬──────────────────────────────────┘    │
│                     │                                        │
│                     ▼                                        │
│  ┌────────────────────────────────────────────────────┐    │
│  │ NSS (Name Service Switch)                          │    │
│  │  - Resolves usernames to UIDs                      │    │
│  │  - Resolves groups to GIDs                         │    │
│  │  - getgrouplist() fetches all user groups          │    │
│  └──────────────────┬──────────────────────────────────┘    │
│                     │                                        │
│                     ▼                                        │
│  ┌────────────────────────────────────────────────────┐    │
│  │ SSSD (System Security Services Daemon)             │    │
│  │  - Communicates with AD via LDAP                   │    │
│  │  - Authenticates via Kerberos                      │    │
│  │  - Caches users, groups, credentials               │    │
│  │  - Handles AD domain discovery                     │    │
│  └──────────────────┬──────────────────────────────────┘    │
│                     │                                        │
└─────────────────────┼────────────────────────────────────────┘
                      │
                      │ Kerberos + LDAP
                      │
                      ▼
         Active Directory (see above)
```

### Authentication Flow

1. **Client connects** to RustSocks SOCKS5 proxy
2. **SOCKS5 handshake** negotiates authentication method (username/password)
3. **PAM authentication** - RustSocks calls PAM with credentials
4. **SSSD authenticates** - PAM delegates to SSSD (pam_sss module)
5. **Kerberos authentication** - SSSD authenticates against AD using Kerberos
6. **Group retrieval** - RustSocks calls `getgrouplist()` to fetch all user's AD groups
7. **NSS resolution** - NSS (via SSSD) queries AD for group memberships
8. **ACL evaluation** - RustSocks evaluates ACL rules based on user's groups
9. **Connection decision** - Allow or block based on matched rule
10. **Proxy connection** - If allowed, establish connection to destination

### Key Components

- **RustSocks**: SOCKS5 proxy with ACL engine
- **PAM (pam_sss)**: Authentication interface
- **NSS (nss_sss)**: Name resolution and group lookup
- **SSSD**: AD communication and caching layer
- **Kerberos**: Encrypted authentication protocol
- **Active Directory**: User and group directory

---

## Prerequisites

### Network Requirements

#### Ports (Linux → AD)

| Port | Protocol | Service | Required |
|------|----------|---------|----------|
| 88 | TCP/UDP | Kerberos | **Yes** |
| 389 | TCP | LDAP | **Yes** |
| 636 | TCP | LDAPS (TLS) | Recommended |
| 464 | TCP/UDP | Kerberos Password Change | Optional |
| 3268 | TCP | Global Catalog | Multi-domain |
| 3269 | TCP | Global Catalog (TLS) | Multi-domain |

#### DNS Requirements

- **Forward DNS**: Linux server can resolve AD domain name
  - Example: `ad.company.com` → AD server IP
- **Reverse DNS**: Recommended but not required
- **SRV records**: AD publishes SRV records for service discovery
  - `_ldap._tcp.ad.company.com`
  - `_kerberos._tcp.ad.company.com`

**Test DNS resolution**:
```bash
# Resolve AD domain
nslookup ad.company.com

# Query SRV records
dig _ldap._tcp.ad.company.com SRV
dig _kerberos._tcp.ad.company.com SRV

# Query domain controllers
dig ad.company.com ANY
```

### Time Synchronization

**Critical**: Kerberos requires time synchronization within **5 minutes** (default).

```bash
# Install NTP or chrony
## Red Hat / CentOS
sudo yum install chrony
sudo systemctl enable chronyd
sudo systemctl start chronyd

## Ubuntu / Debian
sudo apt install chrony
sudo systemctl enable chrony
sudo systemctl start chrony

# Configure time server (point to AD DC or central NTP)
sudo vi /etc/chrony.conf
# Add: server ad.company.com iburst

# Restart and verify
sudo systemctl restart chronyd
chronyc tracking
chronyc sources
```

**Verify time difference**:
```bash
# Check local time
date

# Check AD server time (if accessible)
net time \\DC01

# Time diff should be <5 minutes
```

### AD Account Requirements

You need **one** of the following to join the domain:

1. **Domain Admin account** (easiest for testing)
2. **Account with delegation** to join computers to domain
3. **Pre-created computer account** in AD

**Recommended**: Create dedicated service account with minimal privileges:
- **Account name**: `svc_rustsocks@COMPANY.COM`
- **Permissions**:
  - Read all user attributes
  - Read all group attributes
  - Join computers to domain (if using automated join)

### Permissions in AD

For RustSocks to work, SSSD needs to:
- ✅ Read user accounts (default: all authenticated users can)
- ✅ Read group memberships (default: all authenticated users can)
- ✅ Authenticate users (Kerberos)

**No special AD permissions required** - default read access is sufficient.

---

## System Preparation

### Install Required Packages

#### Red Hat / CentOS / Rocky / AlmaLinux

```bash
# RHEL 8/9, Rocky 8/9, AlmaLinux 8/9
sudo dnf install -y \
    sssd \
    sssd-ad \
    sssd-tools \
    realmd \
    adcli \
    krb5-workstation \
    samba-common-tools \
    oddjob \
    oddjob-mkhomedir

# Enable oddjobd for automatic home directory creation
sudo systemctl enable oddjobd
sudo systemctl start oddjobd
```

#### Ubuntu / Debian

```bash
# Ubuntu 18.04+, Debian 10+
sudo apt update
sudo apt install -y \
    sssd \
    sssd-ad \
    sssd-tools \
    realmd \
    adcli \
    krb5-user \
    samba-common-bin \
    packagekit \
    libnss-sss \
    libpam-sss

# Note: You'll be prompted for Kerberos realm during krb5-user install
# Enter your AD domain in UPPERCASE: COMPANY.COM
```

#### SUSE Linux Enterprise

```bash
# SLES 12/15
sudo zypper install -y \
    sssd \
    sssd-ad \
    sssd-tools \
    realmd \
    adcli \
    krb5-client \
    samba-client
```

### Verify Installation

```bash
# Check SSSD version
sssd --version

# Check realm command
realm --version

# Check adcli
adcli --version

# Verify PAM and NSS libraries
ls -l /usr/lib64/security/pam_sss.so
ls -l /usr/lib64/libnss_sss.so.2
```

### Configure Firewall (if enabled)

```bash
# Allow RustSocks SOCKS5 port (default: 1080)
sudo firewall-cmd --permanent --add-port=1080/tcp
sudo firewall-cmd --reload

# OR for older systems
sudo iptables -A INPUT -p tcp --dport 1080 -j ACCEPT
sudo service iptables save
```

### SELinux Considerations (RHEL/CentOS)

If SELinux is enforcing, you may need to adjust policies:

```bash
# Check SELinux status
getenforce

# Option 1: Allow SSSD to connect to LDAP/Kerberos (recommended)
sudo setsebool -P authlogin_nsswitch_use_ldap on

# Option 2: Create custom policy (advanced)
# See troubleshooting section

# Option 3: Permissive mode for testing (NOT for production)
sudo setenforce 0
```

---

## Joining AD Domain

There are **two methods** to join the domain:

1. **Automated with `realm`** (recommended for beginners)
2. **Manual with `adcli`** (more control)

### Method 1: Automated Join with `realm`

This is the simplest method. `realm` auto-configures SSSD, Kerberos, and NSS.

#### Step 1: Discover AD Domain

```bash
# Discover domain
realm discover ad.company.com

# Example output:
# ad.company.com
#   type: kerberos
#   realm-name: AD.COMPANY.COM
#   domain-name: ad.company.com
#   configured: no
#   server-software: active-directory
#   client-software: sssd
#   required-package: sssd-tools
#   required-package: sssd
#   required-package: adcli
#   required-package: samba-common-tools
```

**If discovery fails**, check:
- DNS is pointing to AD DNS server
- Firewall allows UDP/TCP 389
- SRV records exist (`dig _ldap._tcp.ad.company.com SRV`)

#### Step 2: Join Domain

```bash
# Join with domain admin credentials
sudo realm join --user=administrator ad.company.com

# Enter password when prompted

# Example output:
#  * Resolving: _ldap._tcp.ad.company.com
#  * Performing LDAP DSE lookup on: 10.0.0.10
#  * Successfully enrolled machine in realm
```

**Options**:
- `--user=USERNAME`: AD user with join privileges (default: administrator)
- `--computer-ou="OU=Servers,DC=company,DC=com"`: Place computer in specific OU
- `--os-name="Rocky Linux"`: Set OS name in AD
- `--os-version="9.0"`: Set OS version in AD

#### Step 3: Verify Join

```bash
# Check realm status
realm list

# Example output:
# ad.company.com
#   type: kerberos
#   realm-name: AD.COMPANY.COM
#   domain-name: ad.company.com
#   configured: kerberos-member
#   server-software: active-directory
#   client-software: sssd
#   ...

# Verify computer account in AD
adcli show-computer

# Test Kerberos ticket
kinit administrator@AD.COMPANY.COM
klist
```

### Method 2: Manual Join with `adcli`

This method gives you more control but requires manual configuration.

#### Step 1: Create Kerberos Configuration

See [Kerberos Configuration](#kerberos-configuration) section below.

#### Step 2: Join Domain

```bash
# Join domain
sudo adcli join ad.company.com \
    --domain-ou="OU=Servers,DC=company,DC=com" \
    --login-user=administrator \
    --login-ccache=/tmp/krb5cc_0 \
    --show-details

# Options:
#   --domain-ou: Organizational Unit for computer account
#   --login-user: User with join privileges
#   --login-ccache: Kerberos credential cache
#   --show-details: Verbose output
```

#### Step 3: Configure SSSD

See [SSSD Configuration](#sssd-configuration) section below.

#### Step 4: Configure NSS

See [NSS Configuration](#nss-configuration) section below.

---

## SSSD Configuration

### Automatic Configuration (realm join)

If you used `realm join`, SSSD is already configured. The config file is at `/etc/sssd/sssd.conf`.

**Review and customize** the auto-generated config:

```bash
sudo cat /etc/sssd/sssd.conf
```

### Manual Configuration

If you joined manually with `adcli`, create SSSD configuration:

#### Basic SSSD Configuration

Create `/etc/sssd/sssd.conf`:

```ini
[sssd]
services = nss, pam
config_file_version = 2
domains = ad.company.com

[domain/ad.company.com]
# Active Directory provider
id_provider = ad
auth_provider = ad
access_provider = ad
chpass_provider = ad

# AD server settings
ad_server = dc01.ad.company.com, dc02.ad.company.com
ad_backup_server = dc03.ad.company.com
ad_domain = ad.company.com
ad_hostname = rustsocks.ad.company.com

# Kerberos settings
krb5_realm = AD.COMPANY.COM
krb5_server = dc01.ad.company.com, dc02.ad.company.com
krb5_backup_server = dc03.ad.company.com

# LDAP settings
ldap_uri = ldap://dc01.ad.company.com, ldap://dc02.ad.company.com
ldap_backup_uri = ldap://dc03.ad.company.com
ldap_id_mapping = True
ldap_schema = ad
ldap_referrals = False

# Access control (optional - allow all by default)
# access_provider = ad
# ad_access_filter = (memberOf=CN=VPN-Users,OU=Groups,DC=company,DC=com)

# Caching
cache_credentials = True
krb5_store_password_if_offline = True

# Performance tuning
ldap_idmap_range_size = 200000
enumerate = False

# Debug (remove in production)
# debug_level = 6
```

**Key settings explained**:

- **`id_provider = ad`**: Use AD for user/group lookups
- **`auth_provider = ad`**: Use AD for authentication (Kerberos)
- **`access_provider = ad`**: Use AD for access control
- **`ad_server`**: List of domain controllers (comma-separated)
- **`ad_domain`**: AD domain name (FQDN, lowercase)
- **`krb5_realm`**: Kerberos realm (UPPERCASE)
- **`ldap_id_mapping`**: Auto-map AD SIDs to UIDs/GIDs (recommended)
- **`cache_credentials`**: Cache credentials for offline auth
- **`enumerate = False`**: Don't enumerate all users (performance)

#### Advanced SSSD Configuration

For production environments, add these settings:

```ini
[domain/ad.company.com]
# ... (basic settings above) ...

# Use only LDAPS (encrypted LDAP)
ldap_uri = ldaps://dc01.ad.company.com, ldaps://dc02.ad.company.com
ldap_backup_uri = ldaps://dc03.ad.company.com

# TLS settings
ldap_tls_reqcert = demand
ldap_tls_cacert = /etc/pki/tls/certs/ca-bundle.crt

# Access control - only allow specific group
access_provider = ad
ad_access_filter = (memberOf:1.2.840.113556.1.4.1941:=CN=SOCKS-Users,OU=Groups,DC=company,DC=com)
# Note: 1.2.840.113556.1.4.1941 is the LDAP_MATCHING_RULE_IN_CHAIN OID (nested groups)

# UID/GID mapping
ldap_id_mapping = True
ldap_idmap_range_min = 200000
ldap_idmap_range_max = 2000200000
ldap_idmap_range_size = 200000

# Attribute overrides (match RustSocks expectations)
ldap_user_principal = userPrincipalName
ldap_user_fullname = displayName
ldap_user_name = sAMAccountName

# Performance tuning
entry_cache_timeout = 300
entry_cache_user_timeout = 300
entry_cache_group_timeout = 300
ldap_enumeration_refresh_timeout = 300

# Automatic home directory creation
override_homedir = /home/%u
default_shell = /bin/bash
fallback_homedir = /home/%u

# Debug (remove in production)
# debug_level = 6
```

**Advanced settings explained**:

- **`ldap_tls_reqcert = demand`**: Require valid TLS certificate
- **`ad_access_filter`**: Only allow users in specific AD group
  - `memberOf`: Direct membership
  - `memberOf:1.2.840.113556.1.4.1941:=`: Recursive membership (nested groups)
- **`ldap_idmap_range_*`**: UID/GID mapping range (prevents conflicts)
- **`entry_cache_timeout`**: Cache entries for 5 minutes (reduces AD load)
- **`override_homedir`**: Set home directory pattern

#### Set Correct Permissions

```bash
# SSSD requires strict permissions
sudo chmod 600 /etc/sssd/sssd.conf
sudo chown root:root /etc/sssd/sssd.conf
```

#### Enable and Start SSSD

```bash
# Enable SSSD
sudo systemctl enable sssd

# Start SSSD
sudo systemctl start sssd

# Check status
sudo systemctl status sssd

# Check SSSD logs
sudo journalctl -u sssd -f
```

#### Verify SSSD Configuration

```bash
# Test configuration syntax
sudo sssctl config-check

# Check SSSD status
sudo sssctl domain-status ad.company.com

# Example output:
# Online status: Online
# Active servers:
#   AD Global Catalog: dc01.ad.company.com
#   AD Domain Controller: dc01.ad.company.com
```

---

## Kerberos Configuration

### Configuration File

Create or edit `/etc/krb5.conf`:

```ini
[libdefaults]
    default_realm = AD.COMPANY.COM
    dns_lookup_realm = true
    dns_lookup_kdc = true
    ticket_lifetime = 24h
    renew_lifetime = 7d
    forwardable = true
    rdns = false
    default_ccache_name = KEYRING:persistent:%{uid}

[realms]
    AD.COMPANY.COM = {
        kdc = dc01.ad.company.com
        kdc = dc02.ad.company.com
        admin_server = dc01.ad.company.com
        default_domain = ad.company.com
    }

[domain_realm]
    .ad.company.com = AD.COMPANY.COM
    ad.company.com = AD.COMPANY.COM
```

**Key settings**:

- **`default_realm`**: Kerberos realm (UPPERCASE)
- **`dns_lookup_kdc = true`**: Auto-discover KDCs via DNS SRV records
- **`ticket_lifetime`**: Kerberos ticket lifetime (default: 24h)
- **`forwardable = true`**: Allow ticket forwarding
- **`rdns = false`**: Disable reverse DNS (prevents issues with some AD setups)
- **`default_ccache_name = KEYRING`**: Use kernel keyring for ticket storage (more secure)

### Test Kerberos Authentication

```bash
# Request ticket for AD user
kinit administrator@AD.COMPANY.COM

# Enter password when prompted

# List tickets
klist

# Example output:
# Ticket cache: KEYRING:persistent:0:0
# Default principal: administrator@AD.COMPANY.COM
#
# Valid starting       Expires              Service principal
# 01/15/2025 10:00:00  01/15/2025 20:00:00  krbtgt/AD.COMPANY.COM@AD.COMPANY.COM
#     renew until 01/22/2025 10:00:00

# Destroy ticket
kdestroy
```

**If `kinit` fails**, check:
- Time synchronization (must be within 5 minutes)
- DNS resolution of AD domain
- Network connectivity to KDC (port 88)
- Firewall rules

---

## NSS Configuration

NSS (Name Service Switch) determines how the system resolves users and groups.

### Edit `/etc/nsswitch.conf`

```bash
sudo vi /etc/nsswitch.conf
```

**Modify these lines**:

```
passwd:     files sss
shadow:     files sss
group:      files sss
```

**Explanation**:
- `files`: Check local files first (`/etc/passwd`, `/etc/group`)
- `sss`: Then check SSSD (which queries AD)

**Full example** `/etc/nsswitch.conf`:

```
passwd:     files sss
shadow:     files sss
group:      files sss

hosts:      files dns
bootparams: nisplus [NOTFOUND=return] files

ethers:     files
netmasks:   files
networks:   files
protocols:  files
rpc:        files
services:   files sss

netgroup:   nisplus sss

publickey:  nisplus

automount:  files nisplus sss
aliases:    files nisplus
```

### Test NSS Resolution

```bash
# Lookup AD user (by username)
getent passwd alice@ad.company.com
# Output: alice@ad.company.com:*:200001:200001:Alice Smith:/home/alice:/bin/bash

# Lookup AD user (short form, if configured)
getent passwd alice
# Output: alice:*:200001:200001:Alice Smith:/home/alice:/bin/bash

# Lookup AD group
getent group developers@ad.company.com
# Output: developers@ad.company.com:*:200010:alice,bob,charlie

# Lookup user's group memberships
id alice
# Output: uid=200001(alice) gid=200001(alice) groups=200001(alice),200010(developers@ad.company.com),200020(employees@ad.company.com)
```

**If `getent` fails**, check:
- SSSD is running (`sudo systemctl status sssd`)
- `/etc/nsswitch.conf` includes `sss`
- SSSD logs (`sudo journalctl -u sssd -f`)

---

## PAM Configuration

PAM (Pluggable Authentication Modules) handles authentication.

### Create PAM Service for RustSocks

Create `/etc/pam.d/rustsocks`:

```
#%PAM-1.0
auth        required      pam_env.so
auth        sufficient    pam_sss.so forward_pass
auth        required      pam_deny.so

account     required      pam_unix.so
account     sufficient    pam_localuser.so
account     sufficient    pam_sss.so
account     required      pam_permit.so

password    sufficient    pam_sss.so use_authtok
password    required      pam_deny.so

session     optional      pam_keyinit.so revoke
session     required      pam_limits.so
session     optional      pam_mkhomedir.so skel=/etc/skel/ umask=0077
session     [success=1 default=ignore] pam_succeed_if.so service in crond quiet use_uid
session     required      pam_unix.so
session     optional      pam_sss.so
```

**Key modules**:

- **`pam_sss.so`**: SSSD PAM module (authenticates against AD)
- **`pam_mkhomedir.so`**: Auto-create home directory on first login
- **`pam_keyinit.so`**: Initialize kernel keyring (for Kerberos tickets)

### Set Permissions

```bash
sudo chmod 644 /etc/pam.d/rustsocks
sudo chown root:root /etc/pam.d/rustsocks
```

### Test PAM Authentication

```bash
# Install pamtester (if not already installed)
## RHEL/CentOS
sudo dnf install pamtester

## Ubuntu/Debian
sudo apt install pamtester

# Test authentication
sudo pamtester rustsocks alice authenticate

# Enter password when prompted

# Example output:
# pamtester: invoking pam_start(rustsocks, alice, ...)
# pamtester: performing operation - authenticate
# Password:
# pamtester: successfully authenticated
```

**If `pamtester` fails**, check:
- PAM config file exists (`/etc/pam.d/rustsocks`)
- SSSD is running and connected to AD
- User exists in AD (`getent passwd alice`)
- SELinux is not blocking PAM (`sudo ausearch -m avc -ts recent`)

---

## RustSocks Configuration

Now configure RustSocks to use PAM authentication and AD groups for ACL.

### Main Configuration: `rustsocks.toml`

```toml
[server]
bind_address = "0.0.0.0"
bind_port = 1080
max_connections = 10000

[auth]
# No pre-SOCKS authentication (authenticate during SOCKS handshake)
client_method = "none"

# Use PAM for SOCKS-level authentication
socks_method = "pam.username"

[auth.pam]
# PAM service name (must match /etc/pam.d/<service>)
username_service = "rustsocks"

# Verbose logging for troubleshooting
verbose = true

# Verify PAM service file exists at startup
verify_service = true

[acl]
# Enable ACL engine
enabled = true

# Path to ACL rules
config_file = "config/acl.toml"

# Enable hot-reload (ACL changes applied without restart)
watch = true

# Default user for unauthenticated connections
anonymous_user = "anonymous"

[sessions]
enabled = true
storage = "sqlite"  # Options: "memory", "sqlite", "mariadb"
database_url = "sqlite://data/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
stats_window_hours = 24
stats_api_enabled = true
stats_api_bind_address = "0.0.0.0"
stats_api_port = 9090

[server.pool]
enabled = true
max_idle_per_dest = 4
max_total_idle = 100
idle_timeout_secs = 90
connect_timeout_ms = 5000

[logging]
level = "info"
format = "pretty"
```

**Key settings**:

- **`socks_method = "pam.username"`**: Use PAM for authentication
- **`auth.pam.username_service = "rustsocks"`**: PAM service name (must match `/etc/pam.d/rustsocks`)
- **`acl.enabled = true`**: Enable ACL engine
- **`acl.watch = true`**: Hot-reload ACL rules without restart

### ACL Configuration: `acl.toml`

See [ACL Rules with AD Groups](#acl-rules-with-ad-groups) section below.

---

## Testing & Verification

### Step-by-Step Testing Checklist

#### 1. Verify Time Synchronization

```bash
# Check time
date

# Check time sync status
chronyc tracking
# Ensure "System time" is within a few milliseconds
```

#### 2. Verify DNS Resolution

```bash
# Resolve AD domain
nslookup ad.company.com
# Should return AD DC IP

# Query SRV records
dig _ldap._tcp.ad.company.com SRV
dig _kerberos._tcp.ad.company.com SRV
```

#### 3. Verify Domain Join

```bash
# Check realm
realm list
# Should show "configured: kerberos-member"

# Check computer account
sudo adcli show-computer
# Should show computer object details
```

#### 4. Verify Kerberos

```bash
# Request ticket
kinit alice@AD.COMPANY.COM

# List tickets
klist
# Should show valid ticket

# Destroy ticket
kdestroy
```

#### 5. Verify SSSD

```bash
# Check SSSD status
sudo systemctl status sssd
# Should be "active (running)"

# Check SSSD domain status
sudo sssctl domain-status ad.company.com
# Should show "Online status: Online"

# Check SSSD logs
sudo journalctl -u sssd -n 50 --no-pager
# Should show successful connections to AD
```

#### 6. Verify NSS Resolution

```bash
# Lookup AD user
getent passwd alice@ad.company.com
# Should return: alice@ad.company.com:*:200001:200001:Alice Smith:/home/alice:/bin/bash

# Lookup AD group
getent group developers@ad.company.com
# Should return: developers@ad.company.com:*:200010:alice,bob

# Check user's groups
id alice
# Should show all AD groups
```

#### 7. Verify PAM Authentication

```bash
# Test PAM
sudo pamtester rustsocks alice authenticate
# Enter password
# Should output: "pamtester: successfully authenticated"
```

#### 8. Verify RustSocks Configuration

```bash
# Check config syntax
./rustsocks --config config/rustsocks.toml --check

# Start RustSocks
./rustsocks --config config/rustsocks.toml --log-level debug

# Check startup logs for:
# - PAM service loaded
# - ACL config loaded
# - Listening on 0.0.0.0:1080
```

#### 9. Test SOCKS5 Connection (NoAuth)

First, test without authentication to verify basic connectivity:

```bash
# Temporarily change socks_method to "none"
# In rustsocks.toml: socks_method = "none"

# Restart RustSocks
./rustsocks --config config/rustsocks.toml

# Test with curl
curl -x socks5://127.0.0.1:1080 http://example.com

# Should output HTML from example.com
```

#### 10. Test SOCKS5 Connection (PAM Auth)

Now test with AD authentication:

```bash
# Set socks_method back to "pam.username"
# In rustsocks.toml: socks_method = "pam.username"

# Restart RustSocks
./rustsocks --config config/rustsocks.toml

# Test with AD user credentials
curl -x socks5://alice:PASSWORD@127.0.0.1:1080 http://example.com

# Should output HTML (if ACL allows)

# Check RustSocks logs for:
# - "PAM authentication successful for user: alice"
# - "Retrieved 5 groups for user alice"
# - "ACL decision: Allow"
```

#### 11. Test ACL Rules

Test that ACL correctly blocks/allows based on AD groups:

```bash
# Test as developer (should allow *.dev.company.com)
curl -x socks5://alice:PASSWORD@127.0.0.1:1080 http://app.dev.company.com

# Test as developer (should block social media)
curl -x socks5://alice:PASSWORD@127.0.0.1:1080 http://facebook.com
# Should fail with connection refused

# Check RustSocks logs for ACL decision
```

### Troubleshooting Tests

If any test fails, see [Troubleshooting](#troubleshooting) section below.

---

## ACL Rules with AD Groups

### Example Scenario

**Company Policy**:
- **Temporary employees** (group: `temps@ad.company.com`): Only work-related sites
- **Full employees** (group: `employees@ad.company.com`): Full internet access
- **Administrators** (group: `admins@ad.company.com`): Unrestricted access

### ACL Configuration: `config/acl.toml`

```toml
[global]
# Default policy: BLOCK everything not explicitly allowed
default_policy = "block"

##############################################################################
# GROUP: admins@ad.company.com
# - Unrestricted access to all destinations
##############################################################################
[[groups]]
name = "admins@ad.company.com"  # Full AD group name

  [[groups.rules]]
  action = "allow"
  description = "Admins: Allow all traffic"
  destinations = ["*"]  # All destinations
  ports = ["*"]  # All ports
  protocols = ["tcp", "udp", "both"]
  priority = 1000  # High priority

##############################################################################
# GROUP: employees@ad.company.com
# - Full internet access
# - Block social media during work hours (handled by separate rule)
##############################################################################
[[groups]]
name = "employees@ad.company.com"

  [[groups.rules]]
  action = "allow"
  description = "Employees: Allow all traffic"
  destinations = ["*"]
  ports = ["*"]
  protocols = ["tcp", "udp", "both"]
  priority = 100

  # Block social media (higher priority than allow-all)
  [[groups.rules]]
  action = "block"
  description = "Employees: Block social media"
  destinations = [
    "facebook.com",
    "*.facebook.com",
    "instagram.com",
    "*.instagram.com",
    "twitter.com",
    "*.twitter.com",
    "tiktok.com",
    "*.tiktok.com",
    "reddit.com",
    "*.reddit.com"
  ]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 200  # Higher priority than allow-all (evaluated first)

##############################################################################
# GROUP: temps@ad.company.com
# - Only work-related sites
# - Company domains + essential services
##############################################################################
[[groups]]
name = "temps@ad.company.com"

  [[groups.rules]]
  action = "allow"
  description = "Temps: Allow company websites"
  destinations = [
    "*.company.com",
    "*.company.local"
  ]
  ports = ["*"]
  protocols = ["tcp", "udp", "both"]
  priority = 50

  [[groups.rules]]
  action = "allow"
  description = "Temps: Allow essential services"
  destinations = [
    "*.microsoft.com",       # Microsoft services
    "*.office.com",          # Office 365
    "*.office365.com",       # Office 365
    "*.windows.net",         # Azure
    "*.google.com",          # Google Workspace
    "*.googleapis.com",      # Google APIs
    "*.gstatic.com",         # Google static content
    "*.github.com",          # GitHub
    "*.stackoverflow.com",   # Stack Overflow
    "*.stackexchange.com"    # Stack Exchange
  ]
  ports = ["80", "443", "8080"]
  protocols = ["tcp"]
  priority = 50

  [[groups.rules]]
  action = "allow"
  description = "Temps: Allow internal network"
  destinations = [
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16"
  ]
  ports = ["*"]
  protocols = ["tcp", "udp", "both"]
  priority = 50

##############################################################################
# GROUP: developers@ad.company.com
# - Access to dev environments
# - Code repositories and development tools
##############################################################################
[[groups]]
name = "developers@ad.company.com"

  [[groups.rules]]
  action = "allow"
  description = "Developers: Allow dev/staging/test environments"
  destinations = [
    "*.dev.company.com",
    "*.staging.company.com",
    "*.test.company.com",
    "*.qa.company.com"
  ]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 80

  [[groups.rules]]
  action = "allow"
  description = "Developers: Allow code repositories"
  destinations = [
    "*.github.com",
    "*.gitlab.com",
    "*.bitbucket.org",
    "git.company.com"
  ]
  ports = ["22", "80", "443", "9418"]  # SSH, HTTP, HTTPS, Git protocol
  protocols = ["tcp"]
  priority = 80

  [[groups.rules]]
  action = "allow"
  description = "Developers: Allow package registries"
  destinations = [
    "*.npmjs.org",           # npm
    "*.yarnpkg.com",         # Yarn
    "*.pypi.org",            # Python
    "*.nuget.org",           # NuGet
    "*.maven.org",           # Maven
    "*.docker.com",          # Docker Hub
    "*.docker.io",           # Docker Hub
    "*.gcr.io",              # Google Container Registry
    "quay.io"                # Quay
  ]
  ports = ["80", "443"]
  protocols = ["tcp"]
  priority = 80

##############################################################################
# PER-USER OVERRIDES
# - User rules have higher priority than group rules
##############################################################################
[[users]]
username = "alice@ad.company.com"
groups = ["developers@ad.company.com", "employees@ad.company.com"]

  # Alice needs access to production for on-call duties
  [[users.rules]]
  action = "allow"
  description = "Alice: Allow production access (on-call)"
  destinations = ["*.prod.company.com"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 500  # Higher than group rules

[[users]]
username = "bob@ad.company.com"
groups = ["temps@ad.company.com"]

  # Block Bob from accessing specific subnet (disciplinary action)
  [[users.rules]]
  action = "block"
  description = "Bob: Block access to HR subnet"
  destinations = ["10.10.50.0/24"]
  ports = ["*"]
  protocols = ["tcp", "udp", "both"]
  priority = 1000  # Very high priority (overrides group allow)
```

### ACL Evaluation Logic

RustSocks evaluates rules in this order:

1. **Collect applicable rules**:
   - All rules from user's AD groups (e.g., `temps@ad.company.com`)
   - All rules from user's per-user config (if defined)

2. **Sort rules by priority**:
   - Higher priority first (1000 before 100)
   - BLOCK rules before ALLOW rules (at same priority)

3. **Evaluate rules until first match**:
   - Check destination, port, protocol against rule
   - If match found, return action (ALLOW or BLOCK)
   - If no match, use `default_policy`

4. **Example**:
   - User: `alice@ad.company.com`
   - Groups: `developers@ad.company.com`, `employees@ad.company.com`
   - Destination: `app.dev.company.com:443`
   - Rules collected:
     - Developers allow `*.dev.company.com` (priority 80)
     - Employees allow `*` (priority 100)
   - First match: Developers rule → **ALLOW**

### Testing ACL Rules

```bash
# Test as temp employee (blocked from social media)
curl -x socks5://temp_user:PASSWORD@127.0.0.1:1080 http://facebook.com
# Expected: Connection refused

# Test as temp employee (allowed to company site)
curl -x socks5://temp_user:PASSWORD@127.0.0.1:1080 http://intranet.company.com
# Expected: Success

# Test as developer (allowed to dev environment)
curl -x socks5://alice:PASSWORD@127.0.0.1:1080 http://api.dev.company.com
# Expected: Success

# Test as admin (allowed everywhere)
curl -x socks5://admin:PASSWORD@127.0.0.1:1080 http://anything.com
# Expected: Success
```

### Case-Insensitive Group Matching

Group names are **case-insensitive**:

- `developers@ad.company.com` = `Developers@ad.company.com` = `DEVELOPERS@AD.COMPANY.COM`

This handles variations in AD group naming.

### Hot-Reload ACL Rules

With `acl.watch = true`, you can update ACL rules without restarting:

```bash
# Edit ACL config
vi config/acl.toml

# Save file

# RustSocks automatically reloads (within ~1 second)
# Check logs for: "ACL configuration reloaded successfully"
```

---

## Troubleshooting

### General Troubleshooting Steps

1. **Check service status**:
   ```bash
   sudo systemctl status sssd
   sudo systemctl status chronyd
   ```

2. **Check logs**:
   ```bash
   # SSSD logs
   sudo journalctl -u sssd -f
   sudo tail -f /var/log/sssd/sssd_ad.company.com.log

   # RustSocks logs
   ./rustsocks --config config/rustsocks.toml --log-level debug
   ```

3. **Verify connectivity**:
   ```bash
   # Test LDAP
   ldapsearch -x -H ldap://dc01.ad.company.com -b "dc=company,dc=com" "(sAMAccountName=alice)"

   # Test Kerberos
   kinit alice@AD.COMPANY.COM
   klist
   ```

4. **Check time sync**:
   ```bash
   chronyc tracking
   # "System time" must be within seconds of AD
   ```

### Common Issues

#### Issue 1: `kinit` fails with "Clock skew too great"

**Cause**: Time difference between Linux server and AD DC is >5 minutes.

**Solution**:
```bash
# Sync time immediately
sudo chronyd -q

# Or manually set time
sudo date -s "$(ssh dc01 date)"

# Configure NTP to AD DC
sudo vi /etc/chrony.conf
# Add: server dc01.ad.company.com iburst prefer

sudo systemctl restart chronyd
```

#### Issue 2: `realm discover` fails

**Cause**: DNS cannot resolve AD domain or SRV records.

**Solution**:
```bash
# Check DNS resolution
nslookup ad.company.com
dig _ldap._tcp.ad.company.com SRV

# If fails, set DNS server to AD DC
sudo vi /etc/resolv.conf
# Add: nameserver 10.0.0.10  # AD DC IP

# Or configure NetworkManager
sudo nmcli con mod eth0 ipv4.dns "10.0.0.10"
sudo nmcli con up eth0
```

#### Issue 3: `getent passwd alice` returns nothing

**Cause**: SSSD not communicating with AD, or NSS not configured.

**Solution**:
```bash
# Check SSSD status
sudo sssctl domain-status ad.company.com

# If offline, restart SSSD
sudo systemctl restart sssd

# Check NSS config
grep sss /etc/nsswitch.conf
# Should have: passwd: files sss

# Clear SSSD cache and restart
sudo sss_cache -E
sudo systemctl restart sssd

# Test again
getent passwd alice@ad.company.com
```

#### Issue 4: `pamtester` fails with "Authentication failure"

**Cause**: PAM not configured, SSSD not running, or wrong credentials.

**Solution**:
```bash
# Verify PAM file exists
cat /etc/pam.d/rustsocks

# Check SSSD is running
sudo systemctl status sssd

# Test authentication manually
sudo pamtester -v rustsocks alice authenticate
# Enter password
# Check output for errors

# Check PAM logs
sudo journalctl -t pamtester -f

# Verify user exists
getent passwd alice
id alice
```

#### Issue 5: RustSocks fails with "PAM service not found"

**Cause**: `/etc/pam.d/rustsocks` doesn't exist.

**Solution**:
```bash
# Create PAM service file (see PAM Configuration section)
sudo vi /etc/pam.d/rustsocks

# Set permissions
sudo chmod 644 /etc/pam.d/rustsocks

# Restart RustSocks
./rustsocks --config config/rustsocks.toml
```

#### Issue 6: ACL blocks connections even though user is in allowed group

**Cause**: Groups not retrieved, or group name mismatch.

**Solution**:
```bash
# Check user's groups
id alice
# Should show AD groups

# Enable debug logging in RustSocks
# Set: log_level = "debug" in rustsocks.toml

# Restart and check logs for:
# - "Retrieved N groups for user alice"
# - "User groups: [developers@ad.company.com, employees@ad.company.com]"
# - "ACL decision: Allow/Block"
# - "Matched rule: <rule description>"

# Verify group names match exactly (case-insensitive)
# In acl.toml: name = "developers@ad.company.com"
# User's group: developers@ad.company.com
```

#### Issue 7: SSSD fails with "Could not resolve AD domain"

**Cause**: DNS not configured, or AD DNS not responding.

**Solution**:
```bash
# Test DNS
nslookup ad.company.com
dig _ldap._tcp.ad.company.com SRV

# Set DNS to AD DC
sudo vi /etc/resolv.conf
nameserver 10.0.0.10  # AD DC IP
nameserver 8.8.8.8    # Fallback

# Test again
realm discover ad.company.com
```

#### Issue 8: SELinux blocks SSSD

**Cause**: SELinux policy prevents SSSD from connecting to LDAP/Kerberos.

**Solution**:
```bash
# Check SELinux denials
sudo ausearch -m avc -ts recent | grep sssd

# If denials found, allow SSSD to use LDAP
sudo setsebool -P authlogin_nsswitch_use_ldap on

# Or create custom policy (advanced)
sudo ausearch -m avc -ts recent | audit2allow -M mysssd
sudo semodule -i mysssd.pp

# Or temporarily disable SELinux for testing (NOT for production)
sudo setenforce 0
```

#### Issue 9: Home directory not created on first login

**Cause**: `pam_mkhomedir` not enabled.

**Solution**:
```bash
# Check PAM config
grep mkhomedir /etc/pam.d/rustsocks

# Should have:
# session optional pam_mkhomedir.so skel=/etc/skel/ umask=0077

# If missing, add to /etc/pam.d/rustsocks

# For RHEL/CentOS, also enable oddjobd
sudo systemctl enable oddjobd
sudo systemctl start oddjobd
```

#### Issue 10: Groups not visible with `id alice`

**Cause**: SSSD not fetching groups, or user not in expected groups.

**Solution**:
```bash
# Check groups in AD
ldapsearch -x -H ldap://dc01.ad.company.com \
  -b "dc=company,dc=com" \
  "(sAMAccountName=alice)" memberOf

# Force SSSD to refresh
sudo sss_cache -E
sudo sss_cache -u alice
sudo systemctl restart sssd

# Check groups again
id alice

# If still missing, check SSSD config
grep enumerate /etc/sssd/sssd.conf
# Should be: enumerate = False (default)

# Check SSSD logs
sudo tail -f /var/log/sssd/sssd_ad.company.com.log
```

### Debug Mode

Enable maximum logging for troubleshooting:

#### SSSD Debug Mode

```bash
# Edit /etc/sssd/sssd.conf
sudo vi /etc/sssd/sssd.conf

# Add to [domain/ad.company.com] section:
debug_level = 9

# Restart SSSD
sudo systemctl restart sssd

# Watch logs
sudo tail -f /var/log/sssd/sssd_ad.company.com.log
```

#### Kerberos Debug Mode

```bash
# Set environment variable
export KRB5_TRACE=/dev/stderr

# Run kinit
kinit alice@AD.COMPANY.COM
# Will output detailed Kerberos protocol trace
```

#### RustSocks Debug Mode

```bash
# Run with debug logging
./rustsocks --config config/rustsocks.toml --log-level trace

# Or set in config
[logging]
level = "trace"
```

---

## Production Deployment

### High Availability

For production, configure multiple AD domain controllers:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
ad_server = dc01.ad.company.com, dc02.ad.company.com, dc03.ad.company.com
ad_backup_server = dc04.ad.company.com
krb5_server = dc01.ad.company.com, dc02.ad.company.com
ldap_uri = ldap://dc01.ad.company.com, ldap://dc02.ad.company.com
```

SSSD will automatically failover if primary DC is unavailable.

### Monitoring

Monitor these metrics in production:

1. **SSSD Health**:
   ```bash
   # Check online status
   sudo sssctl domain-status ad.company.com

   # Monitor with systemd
   sudo systemctl status sssd
   ```

2. **Authentication Latency**:
   - PAM authentication time (should be <100ms)
   - Group lookup time (should be <50ms)
   - Use RustSocks session metrics

3. **SSSD Cache Hit Rate**:
   ```bash
   # Monitor cache hits vs misses in SSSD logs
   sudo journalctl -u sssd | grep "cache hit"
   ```

4. **RustSocks Metrics**:
   - Session success/failure rate
   - ACL allow/block decisions
   - Authentication failures
   - Use Prometheus metrics endpoint (`:9090/metrics`)

### Backup Strategies

1. **SSSD Cache Backup**:
   ```bash
   # Backup SSSD cache (allows offline auth)
   sudo tar czf sssd-cache-backup.tar.gz /var/lib/sss/db/
   ```

2. **Configuration Backup**:
   ```bash
   # Backup configs
   sudo tar czf ad-integration-backup.tar.gz \
     /etc/sssd/sssd.conf \
     /etc/krb5.conf \
     /etc/pam.d/rustsocks \
     /etc/nsswitch.conf \
     /etc/rustsocks/
   ```

3. **Restore Procedure**:
   ```bash
   # Restore configs
   sudo tar xzf ad-integration-backup.tar.gz -C /

   # Restart services
   sudo systemctl restart sssd
   sudo systemctl restart rustsocks
   ```

### Update Procedures

When updating RustSocks or system packages:

1. **Test in non-production first**
2. **Backup configurations** (see above)
3. **Update packages**:
   ```bash
   sudo dnf update sssd
   sudo systemctl restart sssd
   ```
4. **Verify integration**:
   ```bash
   sudo sssctl domain-status ad.company.com
   getent passwd alice
   sudo pamtester rustsocks alice authenticate
   ```
5. **Update RustSocks**:
   ```bash
   # Stop service
   sudo systemctl stop rustsocks

   # Replace binary
   sudo cp rustsocks /usr/local/bin/

   # Start service
   sudo systemctl start rustsocks
   ```

### Log Rotation

Configure log rotation for SSSD and RustSocks:

```bash
# /etc/logrotate.d/sssd
/var/log/sssd/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    postrotate
        /bin/kill -HUP `cat /var/run/sssd.pid 2>/dev/null` 2>/dev/null || true
    endscript
}

# /etc/logrotate.d/rustsocks
/var/log/rustsocks/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    postrotate
        /bin/systemctl reload rustsocks || true
    endscript
}
```

---

## Security Best Practices

### 1. Use LDAPS (LDAP over TLS)

Encrypt all LDAP traffic:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
ldap_uri = ldaps://dc01.ad.company.com:636
ldap_tls_reqcert = demand
ldap_tls_cacert = /etc/pki/tls/certs/ca-bundle.crt
```

### 2. Restrict Access with AD Groups

Only allow specific AD groups to use the proxy:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
access_provider = ad
ad_access_filter = (memberOf:1.2.840.113556.1.4.1941:=CN=SOCKS-Users,OU=Groups,DC=company,DC=com)
```

This uses LDAP_MATCHING_RULE_IN_CHAIN (OID 1.2.840.113556.1.4.1941) for recursive group membership.

### 3. Limit Kerberos Ticket Lifetime

```ini
# /etc/krb5.conf
[libdefaults]
ticket_lifetime = 8h
renew_lifetime = 1d
```

### 4. Enable SELinux

Keep SELinux in enforcing mode:

```bash
# Check status
getenforce

# Ensure enforcing
sudo setenforce 1

# Enable booleans for SSSD
sudo setsebool -P authlogin_nsswitch_use_ldap on
```

### 5. Use TLS for RustSocks

Wrap SOCKS5 in TLS to encrypt credentials:

```toml
# rustsocks.toml
[server.tls]
enabled = true
certificate_path = "/etc/rustsocks/server.crt"
private_key_path = "/etc/rustsocks/server.key"
min_protocol_version = "TLS13"
```

### 6. Principle of Least Privilege

- **AD service account**: Read-only access (no admin rights)
- **RustSocks user**: Non-root user (drop privileges after binding socket)
- **ACL default policy**: BLOCK (whitelist approach)

### 7. Audit Logging

Enable comprehensive logging:

```toml
# rustsocks.toml
[logging]
level = "info"
format = "json"

[sessions]
enabled = true
# Use sqlite or MariaDB for persistence
storage = "sqlite"  # Options: "memory", "sqlite", "mariadb"
# All sessions logged to database
```

### 8. Firewall Rules

Restrict access to RustSocks:

```bash
# Only allow from specific subnet
sudo firewall-cmd --permanent --add-rich-rule='rule family="ipv4" source address="10.0.0.0/8" port port="1080" protocol="tcp" accept'
sudo firewall-cmd --reload
```

### 9. Regular Updates

Keep systems updated:

```bash
# RHEL/CentOS
sudo dnf update -y sssd krb5-workstation

# Ubuntu/Debian
sudo apt update && sudo apt upgrade -y sssd krb5-user
```

### 10. Password Complexity

Enforce strong passwords in AD Group Policy:

- Minimum length: 12 characters
- Complexity requirements: Uppercase, lowercase, digit, special
- Password expiration: 90 days
- Account lockout: 5 failed attempts

---

## FAQ

### Q1: Does RustSocks support Azure Active Directory (Azure AD)?

**A**: Yes, but only **Azure AD Domain Services (Azure AD DS)**, not Azure AD cloud-only tenants. Azure AD DS provides LDAP and Kerberos support, while cloud-only Azure AD does not.

**Setup**: Follow this guide but point to Azure AD DS domain (e.g., `aadds.company.com`).

### Q2: Can I use username without domain suffix (e.g., `alice` instead of `alice@ad.company.com`)?

**A**: Yes, configure SSSD to use short names:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
use_fully_qualified_names = False
```

Now users can authenticate with just `alice`.

**Note**: This only works if usernames are unique across all domains.

### Q3: Does RustSocks support multi-domain/multi-forest AD?

**A**: Yes. Configure SSSD with trust relationships:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
ad_server = dc01.ad.company.com
ad_domain = ad.company.com
subdomains_provider = ad
```

SSSD will automatically discover trusted domains.

### Q4: Can I use machine account instead of service account?

**A**: Yes. When you join the domain with `realm join`, a machine account is created automatically. SSSD uses this machine account for LDAP binds.

**No need for separate service account** - machine account is sufficient.

### Q5: How do I handle nested groups in AD?

**A**: SSSD automatically expands nested groups. Use LDAP_MATCHING_RULE_IN_CHAIN in `ad_access_filter`:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
ad_access_filter = (memberOf:1.2.840.113556.1.4.1941:=CN=SOCKS-Users,OU=Groups,DC=company,DC=com)
```

This OID makes LDAP recursively check group memberships.

### Q6: Does RustSocks cache AD group memberships?

**A**: SSSD caches groups (default: 5 minutes). RustSocks fetches fresh groups on every authentication via `getgrouplist()`, which queries SSSD's cache.

**Result**: Groups refreshed every 5 minutes (configurable via `entry_cache_group_timeout` in sssd.conf).

### Q7: What happens if AD is unreachable?

**A**: SSSD has offline mode:

1. **Cached credentials**: If `cache_credentials = True`, users can authenticate with cached passwords
2. **Cached groups**: SSSD returns cached group memberships
3. **Grace period**: SSSD retries AD connection in background

**Result**: RustSocks continues working with stale data until SSSD reconnects.

**Recommendation**: Configure multiple AD DCs for high availability.

### Q8: Can I use RustSocks without PAM (direct LDAP authentication)?

**A**: Not currently. RustSocks relies on PAM for authentication. PAM is standard on all Unix/Linux systems and provides better security than direct LDAP binds.

**Workaround**: Use `pam_ldap` module instead of `pam_sss` if you don't want SSSD.

### Q9: How do I troubleshoot "Permission denied" from SSSD?

**A**: Check these:

1. **SELinux**: `sudo ausearch -m avc -ts recent | grep sssd`
2. **Permissions**: `ls -l /var/lib/sss/db/` (should be owned by sssd:sssd)
3. **Firewall**: `sudo firewall-cmd --list-all` (should allow LDAP/Kerberos ports)
4. **Logs**: `sudo tail -f /var/log/sssd/sssd_ad.company.com.log`

### Q10: Does RustSocks support LDAP over TLS (LDAPS)?

**A**: Yes. Configure SSSD to use LDAPS:

```ini
# /etc/sssd/sssd.conf
[domain/ad.company.com]
ldap_uri = ldaps://dc01.ad.company.com:636
ldap_tls_reqcert = demand
```

RustSocks doesn't handle LDAP directly - it queries via SSSD, so SSSD's TLS settings apply.

---

## Appendix

### Port Reference

| Port | Protocol | Service | Direction | Required |
|------|----------|---------|-----------|----------|
| 88 | TCP/UDP | Kerberos | Linux → AD | **Yes** |
| 389 | TCP | LDAP | Linux → AD | **Yes** |
| 636 | TCP | LDAPS (TLS) | Linux → AD | Recommended |
| 464 | TCP/UDP | Kerberos Password Change | Linux → AD | Optional |
| 3268 | TCP | Global Catalog | Linux → AD | Multi-domain |
| 3269 | TCP | Global Catalog (TLS) | Linux → AD | Multi-domain |
| 445 | TCP | SMB/CIFS | Linux → AD | Optional |
| 123 | UDP | NTP | Linux → AD | Recommended |
| 1080 | TCP | SOCKS5 | Client → RustSocks | **Yes** |
| 9090 | TCP | RustSocks API | Admin → RustSocks | Optional |

### Command Reference

#### Domain Join
```bash
realm discover ad.company.com              # Discover AD domain
realm join --user=admin ad.company.com     # Join domain (automated)
adcli join ad.company.com                  # Join domain (manual)
realm list                                  # List joined domains
realm leave ad.company.com                 # Leave domain
```

#### SSSD Management
```bash
sudo systemctl start sssd                   # Start SSSD
sudo systemctl stop sssd                    # Stop SSSD
sudo systemctl restart sssd                 # Restart SSSD
sudo systemctl status sssd                  # Check status
sudo sssctl domain-status ad.company.com   # Check domain status
sudo sssctl config-check                    # Validate config
sudo sss_cache -E                           # Clear all cache
sudo sss_cache -u alice                     # Clear user cache
sudo sss_cache -g developers                # Clear group cache
```

#### Kerberos
```bash
kinit alice@AD.COMPANY.COM                  # Request ticket
klist                                        # List tickets
kdestroy                                     # Destroy tickets
kvno host/rustsocks.ad.company.com          # Test service principal
```

#### NSS Lookups
```bash
getent passwd alice                         # Lookup user
getent passwd alice@ad.company.com          # Lookup user (FQDN)
getent group developers                     # Lookup group
id alice                                     # Show user's groups
```

#### PAM Testing
```bash
sudo pamtester rustsocks alice authenticate # Test PAM auth
sudo pamtester -v rustsocks alice authenticate # Verbose output
```

#### LDAP Queries
```bash
# Search for user
ldapsearch -x -H ldap://dc01.ad.company.com -b "dc=company,dc=com" "(sAMAccountName=alice)"

# Search for group
ldapsearch -x -H ldap://dc01.ad.company.com -b "dc=company,dc=com" "(cn=developers)"

# List user's groups
ldapsearch -x -H ldap://dc01.ad.company.com -b "dc=company,dc=com" "(sAMAccountName=alice)" memberOf
```

#### Debugging
```bash
# SSSD logs
sudo journalctl -u sssd -f
sudo tail -f /var/log/sssd/sssd_ad.company.com.log

# Enable SSSD debug mode
sudo vi /etc/sssd/sssd.conf  # Add: debug_level = 9
sudo systemctl restart sssd

# Kerberos trace
export KRB5_TRACE=/dev/stderr
kinit alice@AD.COMPANY.COM

# RustSocks debug
./rustsocks --config config/rustsocks.toml --log-level trace
```

### Configuration File Templates

See `config/examples/` directory for:
- `sssd-ad.conf` - SSSD configuration for AD
- `krb5-ad.conf` - Kerberos configuration for AD
- `acl-ad-example.toml` - ACL rules with AD groups
- `rustsocks-ad.toml` - RustSocks configuration for AD

### Further Reading

- **SSSD Documentation**: https://sssd.io/docs/
- **Red Hat Identity Management**: https://access.redhat.com/documentation/en-us/red_hat_enterprise_linux/8/html/configuring_authentication_and_authorization_in_rhel/
- **Microsoft Active Directory**: https://learn.microsoft.com/en-us/windows-server/identity/ad-ds/
- **Kerberos Protocol**: https://web.mit.edu/kerberos/krb5-latest/doc/
- **PAM Documentation**: http://www.linux-pam.org/Linux-PAM-html/
- **RustSocks Documentation**: https://github.com/xulek/rustsocks

---

## Summary

You now have a **complete Active Directory integration** for RustSocks. The key points:

1. ✅ **Zero code changes needed** - RustSocks already supports AD via PAM/SSSD
2. ✅ **Standard Unix authentication** - Uses battle-tested PAM/SSSD stack
3. ✅ **Group-based ACL** - Control access using AD security groups
4. ✅ **Hot-reload support** - Update ACL rules without restart
5. ✅ **High performance** - SSSD caching minimizes AD queries
6. ✅ **Production-ready** - Supports HA, monitoring, audit logging

**Next steps**:
1. Join your Linux server to AD domain (`realm join`)
2. Configure PAM service (`/etc/pam.d/rustsocks`)
3. Define ACL rules with AD groups (`config/acl.toml`)
4. Test authentication (`pamtester rustsocks alice authenticate`)
5. Start RustSocks and test SOCKS5 connections
6. Monitor and enjoy!

For issues, see [Troubleshooting](#troubleshooting) or open a GitHub issue.
