/// LDAP Group Resolution via NSS/SSSD
///
/// This module provides functionality to retrieve user groups from the system,
/// which includes LDAP groups when NSS/SSSD is configured.
///
/// Flow:
/// 1. User authenticates via PAM (e.g., pam_mysql.so)
/// 2. This module retrieves ALL user's groups from system via getgrouplist()
/// 3. ACL engine filters only groups defined in ACL config
/// 4. Irrelevant groups are ignored (no need to define thousands of LDAP groups)
///
use std::ffi::CString;
use tracing::{debug, warn};

/// Get all groups for a user from the system (LDAP via NSS/SSSD)
///
/// Returns a vector of group names that the user belongs to, including:
/// - Primary group
/// - Supplementary groups
/// - LDAP groups (if NSS/SSSD is configured)
///
/// # Example
/// ```no_run
/// use rustsocks::auth::get_user_groups;
///
/// let groups = get_user_groups("alice").unwrap();
/// // Returns: ["alice", "developers", "engineering", "team_foo", ...]
/// ```
#[cfg(unix)]
pub fn get_user_groups(username: &str) -> Result<Vec<String>, std::io::Error> {
    use nix::unistd::{Gid, Group, User};

    // Lookup user to get UID and primary GID
    let user = User::from_name(username)
        .map_err(|e| std::io::Error::other(format!("User lookup failed: {}", e)))?
        .ok_or_else(|| std::io::Error::other(format!("User not found in system: {}", username)))?;

    let gid = user.gid;
    let username_c = CString::new(username)
        .map_err(|e| std::io::Error::other(format!("Invalid username: {}", e)))?;

    // First call to getgrouplist to determine number of groups
    let mut ngroups: libc::c_int = 0;
    unsafe {
        libc::getgrouplist(
            username_c.as_ptr(),
            gid.as_raw(),
            std::ptr::null_mut(),
            &mut ngroups,
        );
    }

    if ngroups <= 0 {
        warn!(
            username = username,
            "getgrouplist returned no groups, using primary group only"
        );
        // Return at least the primary group
        if let Ok(Some(group)) = Group::from_gid(gid) {
            return Ok(vec![group.name]);
        }
        return Ok(Vec::new());
    }

    // Second call with allocated buffer
    let mut gids = vec![0u32; ngroups as usize];
    let result = unsafe {
        libc::getgrouplist(
            username_c.as_ptr(),
            gid.as_raw(),
            gids.as_mut_ptr() as *mut libc::gid_t,
            &mut ngroups,
        )
    };

    if result == -1 {
        return Err(std::io::Error::other(
            "getgrouplist failed to retrieve groups",
        ));
    }

    // Convert GIDs to group names
    let mut groups = Vec::new();
    for gid in gids.iter().take(ngroups as usize) {
        if let Ok(Some(group)) = Group::from_gid(Gid::from_raw(*gid)) {
            groups.push(group.name);
        } else {
            debug!(gid = gid, "Could not resolve GID to group name, skipping");
        }
    }

    debug!(
        username = username,
        group_count = groups.len(),
        groups = ?groups,
        "Resolved user groups from system (LDAP via NSS/SSSD)"
    );

    Ok(groups)
}

/// Non-Unix platforms do not support getgrouplist
#[cfg(not(unix))]
pub fn get_user_groups(_username: &str) -> Result<Vec<String>, std::io::Error> {
    Err(std::io::Error::other(
        "Group lookup is not supported on this platform. Requires Unix/Linux with NSS/SSSD.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires real user on system
    fn test_get_user_groups_current_user() {
        // Test with current user (should always exist)
        let username = std::env::var("USER").unwrap_or_else(|_| "root".to_string());

        let result = get_user_groups(&username);
        assert!(result.is_ok(), "Should retrieve groups for current user");

        let groups = result.unwrap();
        assert!(!groups.is_empty(), "User should have at least one group");

        println!(
            "User '{}' belongs to {} groups: {:?}",
            username,
            groups.len(),
            groups
        );
    }

    #[test]
    fn test_get_user_groups_nonexistent_user() {
        let result = get_user_groups("nonexistent_user_that_does_not_exist_12345");
        assert!(result.is_err(), "Should fail for nonexistent user");

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "Error should indicate user not found"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_get_user_groups_root() {
        // Root user should always exist on Unix systems
        let result = get_user_groups("root");

        // May fail if running in restricted environment, so just log
        match result {
            Ok(groups) => {
                println!("Root user groups: {:?}", groups);
                assert!(!groups.is_empty(), "Root should have at least one group");
            }
            Err(e) => {
                println!("Could not retrieve root groups (may be restricted): {}", e);
            }
        }
    }

    #[test]
    #[cfg(not(unix))]
    fn test_get_user_groups_not_supported_on_non_unix() {
        let result = get_user_groups("testuser");
        assert!(result.is_err(), "Should return error on non-Unix platforms");

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not supported"),
            "Error should indicate platform not supported"
        );
    }
}
