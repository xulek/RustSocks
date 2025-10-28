/// CRUD operations for ACL rules with attribute-based identification
///
/// This module provides functions to add, update, and delete ACL rules
/// using destination + port as unique identifiers instead of indices.

use super::types::{AclConfig, AclRule, GroupAcl, UserAcl};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Identifier for uniquely identifying an ACL rule
///
/// Rules are identified by their destinations and optionally ports.
/// This avoids the fragility of index-based identification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleIdentifier {
    /// Destination patterns (must match exactly)
    pub destinations: Vec<String>,
    /// Port patterns (optional for more precise matching)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<String>>,
}

impl RuleIdentifier {
    /// Check if a rule matches this identifier
    pub fn matches(&self, rule: &AclRule) -> bool {
        // Exact match on destinations
        if self.destinations != rule.destinations {
            return false;
        }

        // If ports specified, they must match exactly
        if let Some(ref ports) = self.ports {
            if ports != &rule.ports {
                return false;
            }
        }

        true
    }

    /// Create identifier from an existing rule
    pub fn from_rule(rule: &AclRule) -> Self {
        Self {
            destinations: rule.destinations.clone(),
            ports: Some(rule.ports.clone()),
        }
    }
}

/// Find a rule in a group by identifier
///
/// Returns the index and reference to the matching rule, or None if not found.
pub fn find_rule_in_group<'a>(
    config: &'a AclConfig,
    group_name: &str,
    identifier: &RuleIdentifier,
) -> Option<(usize, &'a AclRule)> {
    let group = config.groups.iter().find(|g| g.name == group_name)?;

    group
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
}

/// Find a rule in user's rules by identifier
pub fn find_rule_in_user<'a>(
    config: &'a AclConfig,
    username: &str,
    identifier: &RuleIdentifier,
) -> Option<(usize, &'a AclRule)> {
    let user = config.users.iter().find(|u| u.username == username)?;

    user.rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
}

/// Add a new rule to a group
///
/// Creates the group if it doesn't exist.
pub fn add_group_rule(
    config: &mut AclConfig,
    group_name: &str,
    rule: AclRule,
) -> Result<(), String> {
    // Check if rule with same identifier already exists
    let identifier = RuleIdentifier::from_rule(&rule);
    if find_rule_in_group(config, group_name, &identifier).is_some() {
        return Err(format!(
            "Rule with destinations {:?} and ports {:?} already exists in group '{}'",
            rule.destinations, rule.ports, group_name
        ));
    }

    // Find or create group
    if let Some(group) = config.groups.iter_mut().find(|g| g.name == group_name) {
        group.rules.push(rule.clone());
        info!(
            group = group_name,
            destinations = ?rule.destinations,
            "Added rule to existing group"
        );
    } else {
        // Create new group
        config.groups.push(GroupAcl {
            name: group_name.to_string(),
            rules: vec![rule.clone()],
        });
        info!(
            group = group_name,
            "Created new group and added rule"
        );
    }

    Ok(())
}

/// Update an existing rule in a group
///
/// Identifies the rule by the provided identifier and replaces it with new_rule.
pub fn update_group_rule(
    config: &mut AclConfig,
    group_name: &str,
    identifier: &RuleIdentifier,
    new_rule: AclRule,
) -> Result<AclRule, String> {
    let group = config
        .groups
        .iter_mut()
        .find(|g| g.name == group_name)
        .ok_or_else(|| format!("Group '{}' not found", group_name))?;

    let (index, _) = group
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
        .ok_or_else(|| {
            format!(
                "No rule matching destinations {:?} and ports {:?} found in group '{}'",
                identifier.destinations, identifier.ports, group_name
            )
        })?;

    let old_rule = group.rules[index].clone();
    group.rules[index] = new_rule;

    debug!(
        group = group_name,
        old_destinations = ?old_rule.destinations,
        new_destinations = ?group.rules[index].destinations,
        "Updated rule in group"
    );

    Ok(old_rule)
}

/// Delete a rule from a group
///
/// Identifies the rule by the provided identifier.
pub fn delete_group_rule(
    config: &mut AclConfig,
    group_name: &str,
    identifier: &RuleIdentifier,
) -> Result<AclRule, String> {
    let group = config
        .groups
        .iter_mut()
        .find(|g| g.name == group_name)
        .ok_or_else(|| format!("Group '{}' not found", group_name))?;

    let index = group
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
        .map(|(i, _)| i)
        .ok_or_else(|| {
            format!(
                "No rule matching destinations {:?} and ports {:?} found in group '{}'",
                identifier.destinations, identifier.ports, group_name
            )
        })?;

    let deleted_rule = group.rules.remove(index);

    info!(
        group = group_name,
        destinations = ?deleted_rule.destinations,
        "Deleted rule from group"
    );

    Ok(deleted_rule)
}

/// Delete an entire group
pub fn delete_group(config: &mut AclConfig, group_name: &str) -> Result<GroupAcl, String> {
    let index = config
        .groups
        .iter()
        .position(|g| g.name == group_name)
        .ok_or_else(|| format!("Group '{}' not found", group_name))?;

    let deleted_group = config.groups.remove(index);

    info!(
        group = group_name,
        rule_count = deleted_group.rules.len(),
        "Deleted group"
    );

    Ok(deleted_group)
}

/// Add a new rule to a user
///
/// Creates the user entry if it doesn't exist.
pub fn add_user_rule(
    config: &mut AclConfig,
    username: &str,
    rule: AclRule,
) -> Result<(), String> {
    // Check if rule with same identifier already exists
    let identifier = RuleIdentifier::from_rule(&rule);
    if find_rule_in_user(config, username, &identifier).is_some() {
        return Err(format!(
            "Rule with destinations {:?} and ports {:?} already exists for user '{}'",
            rule.destinations, rule.ports, username
        ));
    }

    // Find or create user
    if let Some(user) = config.users.iter_mut().find(|u| u.username == username) {
        user.rules.push(rule.clone());
        info!(
            user = username,
            destinations = ?rule.destinations,
            "Added rule to existing user"
        );
    } else {
        // Create new user
        config.users.push(UserAcl {
            username: username.to_string(),
            groups: vec![],
            rules: vec![rule.clone()],
        });
        info!(user = username, "Created new user and added rule");
    }

    Ok(())
}

/// Update an existing rule for a user
pub fn update_user_rule(
    config: &mut AclConfig,
    username: &str,
    identifier: &RuleIdentifier,
    new_rule: AclRule,
) -> Result<AclRule, String> {
    let user = config
        .users
        .iter_mut()
        .find(|u| u.username == username)
        .ok_or_else(|| format!("User '{}' not found", username))?;

    let (index, _) = user
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
        .ok_or_else(|| {
            format!(
                "No rule matching destinations {:?} and ports {:?} found for user '{}'",
                identifier.destinations, identifier.ports, username
            )
        })?;

    let old_rule = user.rules[index].clone();
    user.rules[index] = new_rule;

    debug!(
        user = username,
        old_destinations = ?old_rule.destinations,
        new_destinations = ?user.rules[index].destinations,
        "Updated rule for user"
    );

    Ok(old_rule)
}

/// Delete a rule from a user
pub fn delete_user_rule(
    config: &mut AclConfig,
    username: &str,
    identifier: &RuleIdentifier,
) -> Result<AclRule, String> {
    let user = config
        .users
        .iter_mut()
        .find(|u| u.username == username)
        .ok_or_else(|| format!("User '{}' not found", username))?;

    let index = user
        .rules
        .iter()
        .enumerate()
        .find(|(_, rule)| identifier.matches(rule))
        .map(|(i, _)| i)
        .ok_or_else(|| {
            format!(
                "No rule matching destinations {:?} and ports {:?} found for user '{}'",
                identifier.destinations, identifier.ports, username
            )
        })?;

    let deleted_rule = user.rules.remove(index);

    info!(
        user = username,
        destinations = ?deleted_rule.destinations,
        "Deleted rule from user"
    );

    Ok(deleted_rule)
}

/// Search for rules matching criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSearchCriteria {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSearchResult {
    pub rule_type: String, // "group" or "user"
    pub owner: String,     // group name or username
    pub rule: AclRule,
}

/// Search for rules matching criteria across all groups and users
pub fn search_rules(
    config: &AclConfig,
    criteria: &RuleSearchCriteria,
) -> Vec<RuleSearchResult> {
    let mut results = Vec::new();

    // Search in groups
    for group in &config.groups {
        for rule in &group.rules {
            if matches_criteria(rule, criteria) {
                results.push(RuleSearchResult {
                    rule_type: "group".to_string(),
                    owner: group.name.clone(),
                    rule: rule.clone(),
                });
            }
        }
    }

    // Search in users
    for user in &config.users {
        for rule in &user.rules {
            if matches_criteria(rule, criteria) {
                results.push(RuleSearchResult {
                    rule_type: "user".to_string(),
                    owner: user.username.clone(),
                    rule: rule.clone(),
                });
            }
        }
    }

    results
}

/// Check if a rule matches search criteria
fn matches_criteria(rule: &AclRule, criteria: &RuleSearchCriteria) -> bool {
    // Check destination if specified
    if let Some(ref dest) = criteria.destination {
        if !rule.destinations.iter().any(|d| d.contains(dest)) {
            return false;
        }
    }

    // Check port if specified
    if let Some(port) = criteria.port {
        let port_str = port.to_string();
        if !rule.ports.iter().any(|p| p == "*" || p == &port_str) {
            return false;
        }
    }

    // Check action if specified
    if let Some(ref action) = criteria.action {
        let rule_action = match rule.action {
            super::types::Action::Allow => "allow",
            super::types::Action::Block => "block",
        };
        if action.to_lowercase() != rule_action {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::types::{Action, Protocol};

    fn create_test_rule(dest: &str, port: &str) -> AclRule {
        AclRule {
            action: Action::Allow,
            description: format!("Test rule for {}", dest),
            destinations: vec![dest.to_string()],
            ports: vec![port.to_string()],
            protocols: vec![Protocol::Tcp],
            priority: 100,
        }
    }

    #[test]
    fn test_rule_identifier_matches() {
        let rule = create_test_rule("*.example.com", "443");

        let identifier1 = RuleIdentifier {
            destinations: vec!["*.example.com".to_string()],
            ports: Some(vec!["443".to_string()]),
        };
        assert!(identifier1.matches(&rule));

        let identifier2 = RuleIdentifier {
            destinations: vec!["*.example.com".to_string()],
            ports: None,
        };
        assert!(identifier2.matches(&rule));

        let identifier3 = RuleIdentifier {
            destinations: vec!["*.different.com".to_string()],
            ports: Some(vec!["443".to_string()]),
        };
        assert!(!identifier3.matches(&rule));
    }

    #[test]
    fn test_add_group_rule() {
        let mut config = AclConfig::default();
        let rule = create_test_rule("*.prod.com", "22");

        assert!(add_group_rule(&mut config, "developers", rule.clone()).is_ok());
        assert_eq!(config.groups.len(), 1);
        assert_eq!(config.groups[0].name, "developers");
        assert_eq!(config.groups[0].rules.len(), 1);

        // Try to add duplicate - should fail
        assert!(add_group_rule(&mut config, "developers", rule).is_err());
    }

    #[test]
    fn test_update_group_rule() {
        let mut config = AclConfig::default();
        let rule1 = create_test_rule("*.prod.com", "22");
        add_group_rule(&mut config, "developers", rule1).unwrap();

        let identifier = RuleIdentifier {
            destinations: vec!["*.prod.com".to_string()],
            ports: Some(vec!["22".to_string()]),
        };

        let mut rule2 = create_test_rule("*.prod.com", "22");
        rule2.action = Action::Block;
        rule2.priority = 500;

        let old_rule = update_group_rule(&mut config, "developers", &identifier, rule2).unwrap();
        assert_eq!(old_rule.action, Action::Allow);
        assert_eq!(config.groups[0].rules[0].action, Action::Block);
        assert_eq!(config.groups[0].rules[0].priority, 500);
    }

    #[test]
    fn test_delete_group_rule() {
        let mut config = AclConfig::default();
        let rule = create_test_rule("*.prod.com", "22");
        add_group_rule(&mut config, "developers", rule).unwrap();

        let identifier = RuleIdentifier {
            destinations: vec!["*.prod.com".to_string()],
            ports: Some(vec!["22".to_string()]),
        };

        let deleted = delete_group_rule(&mut config, "developers", &identifier).unwrap();
        assert_eq!(deleted.destinations[0], "*.prod.com");
        assert_eq!(config.groups[0].rules.len(), 0);
    }

    #[test]
    fn test_search_rules() {
        let mut config = AclConfig::default();

        let rule1 = create_test_rule("*.prod.com", "22");
        add_group_rule(&mut config, "developers", rule1).unwrap();

        let mut rule2 = create_test_rule("*.prod.com", "443");
        rule2.action = Action::Block;
        add_group_rule(&mut config, "admins", rule2).unwrap();

        // Search by destination
        let criteria = RuleSearchCriteria {
            destination: Some("prod.com".to_string()),
            port: None,
            action: None,
        };
        let results = search_rules(&config, &criteria);
        assert_eq!(results.len(), 2);

        // Search by action
        let criteria = RuleSearchCriteria {
            destination: None,
            port: None,
            action: Some("block".to_string()),
        };
        let results = search_rules(&config, &criteria);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].owner, "admins");
    }
}
