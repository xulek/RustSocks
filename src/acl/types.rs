use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// ACL Action - Allow or Block
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Block,
}

/// Protocol filter
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
    #[serde(alias = "*")] // Allow "*" as alias for "both"
    Both,
}

impl Protocol {
    pub fn matches(&self, other: &Protocol) -> bool {
        match (self, other) {
            (Protocol::Both, _) => true,
            (_, Protocol::Both) => true,
            (a, b) => a == b,
        }
    }
}

/// Destination matcher - IP, CIDR, Domain, or Wildcard domain
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DestinationMatcher {
    /// Single IP address (exact match)
    Ip(IpAddr),
    /// CIDR range (e.g., "10.0.0.0/8")
    #[serde(with = "ipnet_serde")]
    Cidr(IpNet),
    /// Exact domain name or wildcard pattern (e.g., "example.com" or "*.example.com")
    Domain(String),
}

// Custom serialization for IpNet
mod ipnet_serde {
    use ipnet::IpNet;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &IpNet, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<IpNet, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl DestinationMatcher {
    /// Create from string - auto-detect type
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, String> {
        // Check if it's a wildcard domain
        if s.contains('*') {
            return Ok(DestinationMatcher::Domain(s.to_string()));
        }

        // Try to parse as IP
        if let Ok(ip) = s.parse::<IpAddr>() {
            return Ok(DestinationMatcher::Ip(ip));
        }

        // Try to parse as CIDR
        if let Ok(cidr) = s.parse::<IpNet>() {
            return Ok(DestinationMatcher::Cidr(cidr));
        }

        // Otherwise, treat as domain
        Ok(DestinationMatcher::Domain(s.to_string()))
    }
}

/// Port matcher - Single, Range, Multiple, or Any
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortMatcher {
    /// Any port (*)
    Any,
    /// Single port
    Single(u16),
    /// Port range (e.g., "8000-9000")
    Range { start: u16, end: u16 },
    /// Multiple specific ports (e.g., [80, 443, 8080])
    Multiple(Vec<u16>),
}

impl PortMatcher {
    /// Create from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, String> {
        if s == "*" {
            return Ok(PortMatcher::Any);
        }

        // Check for range (e.g., "8000-9000")
        if s.contains('-') {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid port range: {}", s));
            }
            let start = parts[0]
                .parse::<u16>()
                .map_err(|_| format!("Invalid start port: {}", parts[0]))?;
            let end = parts[1]
                .parse::<u16>()
                .map_err(|_| format!("Invalid end port: {}", parts[1]))?;
            return Ok(PortMatcher::Range { start, end });
        }

        // Check for multiple ports (e.g., "80,443,8080")
        if s.contains(',') {
            let ports: Result<Vec<u16>, _> =
                s.split(',').map(|p| p.trim().parse::<u16>()).collect();
            return ports
                .map(PortMatcher::Multiple)
                .map_err(|e| format!("Invalid port list: {}", e));
        }

        // Single port
        s.parse::<u16>()
            .map(PortMatcher::Single)
            .map_err(|e| format!("Invalid port: {}", e))
    }
}

/// ACL Rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRule {
    /// Action to take (allow/block)
    pub action: Action,

    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Destination matchers (IP, CIDR, domain, wildcard)
    #[serde(default)]
    pub destinations: Vec<String>,

    /// Port matchers (single, range, multiple, any)
    #[serde(default)]
    pub ports: Vec<String>,

    /// Protocol filter (tcp, udp, both)
    #[serde(default = "default_protocols")]
    pub protocols: Vec<Protocol>,

    /// Priority (higher = evaluated first)
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_protocols() -> Vec<Protocol> {
    vec![Protocol::Both]
}

fn default_priority() -> u32 {
    // BLOCK rules get higher priority by default
    100
}

/// Dynamic policy execution mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    #[default]
    Enforce,
    Monitor,
}

/// Optional schedule for a dynamic access policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySchedule {
    #[serde(default)]
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub utc_offset_minutes: i32,
}

/// Dynamic conditions attached to an access policy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyConditions {
    #[serde(default)]
    pub source_ips: Vec<String>,
    #[serde(default)]
    pub auth_methods: Vec<String>,
    #[serde(default)]
    pub schedule: Option<PolicySchedule>,
    #[serde(default)]
    pub not_before: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub max_active_connections: Option<u32>,
    #[serde(default)]
    pub max_connections_per_minute: Option<u32>,
    #[serde(default)]
    pub daily_transfer_limit_bytes: Option<u64>,
    #[serde(default)]
    pub monthly_transfer_limit_bytes: Option<u64>,
}

/// Dynamic access policy evaluated together with legacy ACL rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub id: String,
    #[serde(default = "default_policy_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: PolicyMode,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    pub action: Action,
    #[serde(default)]
    pub destinations: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default = "default_protocols")]
    pub protocols: Vec<Protocol>,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(default)]
    pub enforce_conditions: bool,
    #[serde(default)]
    pub conditions: PolicyConditions,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_policy_enabled() -> bool {
    true
}

/// Per-user ACL configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAcl {
    pub username: String,

    #[serde(default)]
    pub groups: Vec<String>,

    #[serde(default)]
    pub rules: Vec<AclRule>,
}

/// Per-group ACL configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupAcl {
    pub name: String,

    #[serde(default)]
    pub rules: Vec<AclRule>,
}

/// Global ACL configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalAclConfig {
    /// Default policy when no rules match
    #[serde(default = "default_policy")]
    pub default_policy: Action,
}

fn default_policy() -> Action {
    Action::Block
}

impl Default for GlobalAclConfig {
    fn default() -> Self {
        Self {
            default_policy: Action::Block,
        }
    }
}

/// Complete ACL configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AclConfig {
    #[serde(default)]
    pub global: GlobalAclConfig,

    #[serde(default)]
    pub users: Vec<UserAcl>,

    #[serde(default)]
    pub groups: Vec<GroupAcl>,

    #[serde(default)]
    pub policies: Vec<AccessPolicy>,
}

/// ACL Decision result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclDecision {
    Allow,
    Block,
}

impl From<&Action> for AclDecision {
    fn from(action: &Action) -> Self {
        match action {
            Action::Allow => AclDecision::Allow,
            Action::Block => AclDecision::Block,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_matcher_from_str() {
        // IP address
        let ip = DestinationMatcher::from_str("192.168.1.1").unwrap();
        assert!(matches!(ip, DestinationMatcher::Ip(_)));

        // CIDR
        let cidr = DestinationMatcher::from_str("10.0.0.0/8").unwrap();
        assert!(matches!(cidr, DestinationMatcher::Cidr(_)));

        // Domain
        let domain = DestinationMatcher::from_str("example.com").unwrap();
        assert!(matches!(domain, DestinationMatcher::Domain(_)));

        // Wildcard domain
        let wildcard = DestinationMatcher::from_str("*.example.com").unwrap();
        assert!(matches!(wildcard, DestinationMatcher::Domain(_)));
    }

    #[test]
    fn test_port_matcher_from_str() {
        // Any
        let any = PortMatcher::from_str("*").unwrap();
        assert!(matches!(any, PortMatcher::Any));

        // Single
        let single = PortMatcher::from_str("443").unwrap();
        assert!(matches!(single, PortMatcher::Single(443)));

        // Range
        let range = PortMatcher::from_str("8000-9000").unwrap();
        if let PortMatcher::Range { start, end } = range {
            assert_eq!(start, 8000);
            assert_eq!(end, 9000);
        } else {
            panic!("Expected Range");
        }

        // Multiple
        let multiple = PortMatcher::from_str("80,443,8080").unwrap();
        if let PortMatcher::Multiple(ports) = multiple {
            assert_eq!(ports, vec![80, 443, 8080]);
        } else {
            panic!("Expected Multiple");
        }
    }

    #[test]
    fn test_protocol_matching() {
        assert!(Protocol::Both.matches(&Protocol::Tcp));
        assert!(Protocol::Both.matches(&Protocol::Udp));
        assert!(Protocol::Tcp.matches(&Protocol::Both));
        assert!(Protocol::Tcp.matches(&Protocol::Tcp));
        assert!(!Protocol::Tcp.matches(&Protocol::Udp));
    }
}
