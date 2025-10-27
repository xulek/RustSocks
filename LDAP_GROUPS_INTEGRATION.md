# LDAP Groups Integration - Dynamiczne pobieranie grup

## Problem

Obecna implementacja wymaga **ręcznego mapowania** użytkowników do grup w `acl.toml`:

```toml
[[users]]
username = "alice"
groups = ["developers"]
```

To oznacza, że musisz synchronizować grupy LDAP z ACL config ręcznie.

## Rozwiązanie: Dynamiczne pobieranie grup z systemu

### Opcja A: Użycie `getgrouplist()` (Unix)

Pobierz grupy użytkownika bezpośrednio z systemu (LDAP via NSS/SSSD):

```rust
// src/auth/groups.rs

use nix::unistd::{User, Group};
use std::ffi::CString;
use tracing::{debug, warn};

/// Get user's groups from system (via NSS/SSSD/LDAP)
#[cfg(unix)]
pub fn get_user_groups(username: &str) -> Result<Vec<String>, std::io::Error> {
    // Lookup user
    let user = User::from_name(username)
        .map_err(|e| std::io::Error::other(format!("User lookup failed: {}", e)))?
        .ok_or_else(|| std::io::Error::other(format!("User not found: {}", username)))?;

    // Get groups for user
    let gid = user.gid;
    let uid = user.uid;

    // Use getgrouplist to get all groups (primary + supplementary)
    let mut groups = Vec::new();
    let username_c = CString::new(username).unwrap();

    // First call to get number of groups
    let mut ngroups = 0;
    unsafe {
        let ret = libc::getgrouplist(
            username_c.as_ptr(),
            gid.as_raw(),
            std::ptr::null_mut(),
            &mut ngroups,
        );

        if ret == -1 && ngroups > 0 {
            // Second call with allocated buffer
            let mut gids = vec![0u32; ngroups as usize];
            let ret = libc::getgrouplist(
                username_c.as_ptr(),
                gid.as_raw(),
                gids.as_mut_ptr() as *mut libc::gid_t,
                &mut ngroups,
            );

            if ret != -1 {
                // Convert GIDs to group names
                for gid in gids.iter().take(ngroups as usize) {
                    if let Ok(Some(group)) = Group::from_gid(nix::unistd::Gid::from_raw(*gid)) {
                        groups.push(group.name);
                    }
                }
            }
        }
    }

    debug!(
        username = username,
        groups = ?groups,
        "Resolved user groups from system"
    );

    Ok(groups)
}

#[cfg(not(unix))]
pub fn get_user_groups(username: &str) -> Result<Vec<String>, std::io::Error> {
    Err(std::io::Error::other("Group lookup not supported on this platform"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires real user on system
    fn test_get_user_groups() {
        // Test with current user
        let username = std::env::var("USER").unwrap();
        let groups = get_user_groups(&username).unwrap();

        println!("User {} is in groups: {:?}", username, groups);
        assert!(!groups.is_empty());
    }
}
```

### Integracja z AuthManager

```rust
// src/auth/mod.rs

impl AuthManager {
    pub async fn authenticate(
        &self,
        stream: &mut TcpStream,
        method: AuthMethod,
        client_ip: IpAddr,
    ) -> Result<Option<(String, Vec<String>)>> {  // Zwraca (username, groups)
        match (&self.socks_backend, method) {
            // ... existing code ...

            (AuthBackend::PamUsername(pam), AuthMethod::UserPass) => {
                debug!("Performing PAM username authentication");
                let (username, password) = parse_userpass_auth(stream).await?;

                match pam
                    .authenticate_username(client_ip, &username, &password)
                    .await
                {
                    Ok(()) => {
                        send_auth_response(stream, true).await?;
                        info!(user = %username, "PAM authentication successful");

                        // Pobierz grupy z systemu (LDAP via NSS/SSSD)
                        let groups = crate::auth::groups::get_user_groups(&username)
                            .unwrap_or_else(|e| {
                                warn!(
                                    user = %username,
                                    error = %e,
                                    "Failed to resolve user groups, using empty list"
                                );
                                Vec::new()
                            });

                        info!(
                            user = %username,
                            groups = ?groups,
                            "Resolved user groups from system"
                        );

                        Ok(Some((username, groups)))
                    }
                    Err(e) => {
                        send_auth_response(stream, false).await?;
                        warn!(user = %username, error = ?e, "PAM authentication failed");
                        Err(map_pam_runtime_error(e))
                    }
                }
            }

            // ... rest of code ...
        }
    }
}
```

### Modyfikacja ACL Engine do obsługi dynamicznych grup

```rust
// src/acl/engine.rs

impl AclEngine {
    /// Evaluate ACL with dynamic groups
    pub async fn evaluate_with_groups(
        &self,
        username: &str,
        user_groups: &[String],  // Grupy z LDAP
        dest: &Address,
        port: u16,
        protocol: &Protocol,
    ) -> (AclDecision, Option<String>) {
        let config = self.config.read().unwrap();

        // Zbierz reguły użytkownika
        let mut rules = Vec::new();

        // 1. Reguły użytkownika (per-user)
        if let Some(user_config) = config.users.get(username) {
            rules.extend(user_config.rules.iter().map(|r| (r, "user")));
        }

        // 2. Reguły grup (z LDAP)
        for group_name in user_groups {
            if let Some(group_config) = config.groups.get(group_name) {
                rules.extend(group_config.rules.iter().map(|r| (r, "group")));
            }
        }

        // Sortuj: BLOCK first, potem po priority
        rules.sort_by(|a, b| {
            match (&a.0.action, &b.0.action) {
                (Action::Block, Action::Allow) => std::cmp::Ordering::Less,
                (Action::Allow, Action::Block) => std::cmp::Ordering::Greater,
                _ => b.0.priority.cmp(&a.0.priority),
            }
        });

        // Ewaluuj reguły
        for (rule, source) in rules {
            if self.rule_matches(rule, dest, port, protocol) {
                let description = rule.description
                    .clone()
                    .unwrap_or_else(|| format!("{} rule from {}", rule.action, source));

                return (
                    if rule.action == Action::Allow {
                        AclDecision::Allow
                    } else {
                        AclDecision::Block
                    },
                    Some(description),
                );
            }
        }

        // Default policy
        (config.global.default_policy, None)
    }
}
```

### Modyfikacja handler.rs

```rust
// src/server/handler.rs (w funkcji handle_socks5)

// Step 2: Authentication
let (user, groups) = ctx
    .auth_manager
    .authenticate(&mut client_stream, server_method, client_addr.ip())
    .await?;

let acl_user = user
    .clone()
    .unwrap_or_else(|| ctx.anonymous_user.as_ref().clone());

let acl_groups = groups.unwrap_or_default();

info!(
    user = %acl_user,
    groups = ?acl_groups,
    "User authenticated with groups"
);

// ... later in ACL evaluation ...

if let Some(engine) = ctx.acl_engine.as_ref() {
    let protocol = match request.command {
        Command::UdpAssociate => Protocol::Udp,
        _ => Protocol::Tcp,
    };

    let (decision, matched_rule) = engine
        .evaluate_with_groups(
            &acl_user,
            &acl_groups,  // Grupy z LDAP!
            &request.address,
            request.port,
            &protocol
        )
        .await;

    // ... rest of ACL handling ...
}
```

### Uproszczona konfiguracja ACL

Teraz ACL config jest prostsze - **nie musisz** definiować users:

```toml
# config/acl.toml

[global]
default_policy = "block"

# Tylko definicje grup (odpowiadają grupom LDAP)
[[groups]]
name = "developers"  # Musi odpowiadać nazwie grupy w LDAP!

  [[groups.rules]]
  action = "allow"
  description = "Developers access to internal"
  destinations = ["*.dev.company.com", "10.0.0.0/8"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 100

[[groups]]
name = "admins"  # Grupa z LDAP

  [[groups.rules]]
  action = "allow"
  description = "Admins full access"
  destinations = ["*"]
  ports = ["*"]
  protocols = ["tcp", "udp"]
  priority = 100

# Opcjonalnie: per-user overrides
[[users]]
username = "alice"
# Grupy będą pobrane automatycznie z LDAP
# Możesz dodać dodatkowe reguły per-user:

  [[users.rules]]
  action = "allow"
  description = "Alice extra staging access"
  destinations = ["*.staging.company.com"]
  ports = ["*"]
  protocols = ["tcp"]
  priority = 50
```

### Zależności (Cargo.toml)

```toml
[target.'cfg(unix)'.dependencies]
pam = "0.7"
nix = { version = "0.27", features = ["user"] }  # Dla getgrouplist
libc = "0.2"
```

## Testowanie

```bash
# 1. Sprawdź grupy użytkownika w systemie
id alice
# uid=1001(alice) gid=1001(alice) groups=1001(alice),2001(developers),2002(engineering)

# 2. Sprawdź SSSD/LDAP integration
getent passwd alice
getent group developers

# 3. Test RustSocks
./target/release/rustsocks --config config/rustsocks.toml

# 4. Test SOCKS connection
curl -x socks5://alice:password@127.0.0.1:1080 http://api.dev.company.com

# Logi powinny pokazać:
# INFO User authenticated with groups groups=["alice", "developers", "engineering"]
# INFO ACL allowed connection rule="Developers access to internal"
```

## Zalety rozwiązania

✅ **Automatyczna synchronizacja** - grupy z LDAP są pobierane automatycznie
✅ **Uproszczona konfiguracja** - nie trzeba mapować users → groups ręcznie
✅ **Real-time updates** - zmiana grup w LDAP od razu działa (po re-auth)
✅ **Zgodność z systemem** - używa NSS/SSSD jak reszta systemu

## Wady

⚠️ **Wymaga NSS/SSSD** - system musi mieć skonfigurowany LDAP via NSS
⚠️ **Unix only** - `getgrouplist()` działa tylko na Unix/Linux
⚠️ **Dodatkowa złożoność** - więcej kodu, więcej punktów awarii

## Deployment checklist

- [ ] SSSD/NSS skonfigurowane dla LDAP
- [ ] `getent passwd <username>` działa
- [ ] `getent group <groupname>` działa
- [ ] `/etc/nsswitch.conf` ma `group: files sss`
- [ ] Testy z prawdziwymi użytkownikami LDAP
- [ ] Monitoring grup w logach RustSocks

