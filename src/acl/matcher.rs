use super::types::{AclRule, Action, PortMatcher, Protocol};
use crate::protocol::Address;
use regex::Regex;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Parsed matchers (compiled from strings for efficiency)
#[derive(Debug, Clone)]
pub struct CompiledDestinationMatcher {
    matcher: DestinationMatcherType,
}

#[derive(Debug, Clone)]
enum DestinationMatcherType {
    MatchAll, // "*" - matches everything (IPs, domains, all)
    Ip(IpAddr),
    Cidr(ipnet::IpNet),
    Domain(String),
    WildcardDomain(WildcardPattern),
}

#[derive(Debug, Clone)]
struct WildcardPattern {
    regex: Regex,
}

impl CompiledDestinationMatcher {
    /// Compile from string
    pub fn compile(s: &str) -> Result<Self, String> {
        let matcher_type = if s == "*" {
            // Special case: "*" matches everything (all IPs, domains, etc.)
            DestinationMatcherType::MatchAll
        } else if s.contains('*') {
            // Wildcard domain pattern - convert to regex
            let pattern = wildcard_to_regex(s)?;
            DestinationMatcherType::WildcardDomain(WildcardPattern {
                regex: Regex::new(&pattern)
                    .map_err(|e| format!("Invalid wildcard pattern: {}", e))?,
            })
        } else if let Ok(ip) = s.parse::<IpAddr>() {
            DestinationMatcherType::Ip(ip)
        } else if let Ok(cidr) = s.parse::<ipnet::IpNet>() {
            DestinationMatcherType::Cidr(cidr)
        } else {
            // Plain domain
            DestinationMatcherType::Domain(s.to_lowercase())
        };

        Ok(Self {
            matcher: matcher_type,
        })
    }

    /// Check if address matches this matcher
    pub fn matches(&self, addr: &Address) -> bool {
        match &self.matcher {
            DestinationMatcherType::MatchAll => true, // "*" matches everything
            DestinationMatcherType::Ip(ip) => Self::match_ip(ip, addr),
            DestinationMatcherType::Cidr(cidr) => Self::match_cidr(cidr, addr),
            DestinationMatcherType::Domain(domain) => Self::match_domain(domain, addr),
            DestinationMatcherType::WildcardDomain(pattern) => Self::match_wildcard(pattern, addr),
        }
    }

    fn match_ip(ip: &IpAddr, addr: &Address) -> bool {
        match (ip, addr) {
            (IpAddr::V4(matcher_ip), Address::IPv4(octets)) => {
                let addr_ip = Ipv4Addr::from(*octets);
                matcher_ip == &addr_ip
            }
            (IpAddr::V6(matcher_ip), Address::IPv6(octets)) => {
                let addr_ip = Ipv6Addr::from(*octets);
                matcher_ip == &addr_ip
            }
            (_, Address::Domain(domain)) => {
                if let Ok(parsed) = domain.parse::<IpAddr>() {
                    matcher_ip_eq(ip, &parsed)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn match_cidr(cidr: &ipnet::IpNet, addr: &Address) -> bool {
        let ip = match addr {
            Address::IPv4(octets) => IpAddr::V4(Ipv4Addr::from(*octets)),
            Address::IPv6(octets) => IpAddr::V6(Ipv6Addr::from(*octets)),
            Address::Domain(domain) => match domain.parse::<IpAddr>() {
                Ok(parsed) => parsed,
                Err(_) => return false,
            },
        };

        cidr.contains(&ip)
    }

    fn match_domain(domain: &str, addr: &Address) -> bool {
        if let Address::Domain(addr_domain) = addr {
            domain.eq_ignore_ascii_case(addr_domain)
        } else {
            false
        }
    }

    fn match_wildcard(pattern: &WildcardPattern, addr: &Address) -> bool {
        if let Address::Domain(addr_domain) = addr {
            pattern.regex.is_match(&addr_domain.to_lowercase())
        } else {
            false
        }
    }
}

fn matcher_ip_eq(matcher: &IpAddr, candidate: &IpAddr) -> bool {
    match (matcher, candidate) {
        (IpAddr::V4(expected), IpAddr::V4(actual)) => expected == actual,
        (IpAddr::V6(expected), IpAddr::V6(actual)) => expected == actual,
        _ => false,
    }
}

/// Convert wildcard pattern to regex
/// Examples:
///   *.example.com -> ^[^.]+\.example\.com$
///   api.*.com -> ^api\.[^.]+\.com$
fn wildcard_to_regex(pattern: &str) -> Result<String, String> {
    let mut regex = String::from("^");

    for part in pattern.split('.') {
        if !regex.ends_with('^') {
            regex.push_str(r"\.");
        }

        if part == "*" {
            regex.push_str("[^.]+");
        } else {
            // Escape special regex characters
            for ch in part.chars() {
                if "[]{}()|^$+?.\\".contains(ch) {
                    regex.push('\\');
                }
                regex.push(ch);
            }
        }
    }

    regex.push('$');
    Ok(regex)
}

/// Compiled port matcher
#[derive(Debug, Clone)]
pub struct CompiledPortMatcher {
    matcher: PortMatcherType,
}

#[derive(Debug, Clone)]
enum PortMatcherType {
    Any,
    Single(u16),
    Range { start: u16, end: u16 },
    Multiple(Vec<u16>),
}

impl CompiledPortMatcher {
    /// Compile from string
    pub fn compile(s: &str) -> Result<Self, String> {
        let matcher_type = PortMatcher::from_str(s)?;

        let compiled = match matcher_type {
            PortMatcher::Any => PortMatcherType::Any,
            PortMatcher::Single(p) => PortMatcherType::Single(p),
            PortMatcher::Range { start, end } => PortMatcherType::Range { start, end },
            PortMatcher::Multiple(ports) => PortMatcherType::Multiple(ports),
        };

        Ok(Self { matcher: compiled })
    }

    /// Check if port matches
    pub fn matches(&self, port: u16) -> bool {
        match &self.matcher {
            PortMatcherType::Any => true,
            PortMatcherType::Single(p) => port == *p,
            PortMatcherType::Range { start, end } => port >= *start && port <= *end,
            PortMatcherType::Multiple(ports) => ports.contains(&port),
        }
    }
}

/// Compiled ACL rule with pre-compiled matchers
#[derive(Debug, Clone)]
pub struct CompiledAclRule {
    pub action: Action,
    pub description: String,
    pub destinations: Vec<CompiledDestinationMatcher>,
    pub ports: Vec<CompiledPortMatcher>,
    pub protocols: Vec<Protocol>,
    pub priority: u32,
}

impl CompiledAclRule {
    /// Compile an ACL rule for efficient matching
    pub fn compile(rule: &AclRule) -> Result<Self, String> {
        let destinations: Result<Vec<_>, _> = rule
            .destinations
            .iter()
            .map(|s| CompiledDestinationMatcher::compile(s))
            .collect();

        let ports: Result<Vec<_>, _> = rule
            .ports
            .iter()
            .map(|s| CompiledPortMatcher::compile(s))
            .collect();

        Ok(Self {
            action: rule.action.clone(),
            description: rule.description.clone(),
            destinations: destinations?,
            ports: ports?,
            protocols: rule.protocols.clone(),
            priority: rule.priority,
        })
    }

    /// Check if this rule matches the given connection parameters
    pub fn matches(&self, addr: &Address, port: u16, protocol: &Protocol) -> bool {
        // Check protocol
        if !self.protocols.iter().any(|p| p.matches(protocol)) {
            return false;
        }

        // Check destination
        // Empty list = match nothing
        // Use ["*"] to match all destinations
        let dest_match = self.destinations.iter().any(|d| d.matches(addr));

        // Check port
        // Empty list = match nothing
        // Use ["*"] to match all ports
        let port_match = self.ports.iter().any(|p| p.matches(port));

        dest_match && port_match
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_matching() {
        let matcher = CompiledDestinationMatcher::compile("192.168.1.1").unwrap();

        assert!(matcher.matches(&Address::IPv4([192, 168, 1, 1])));
        assert!(!matcher.matches(&Address::IPv4([192, 168, 1, 2])));
        assert!(!matcher.matches(&Address::Domain("example.com".to_string())));
    }

    #[test]
    fn test_cidr_matching() {
        let matcher = CompiledDestinationMatcher::compile("10.0.0.0/8").unwrap();

        assert!(matcher.matches(&Address::IPv4([10, 0, 0, 1])));
        assert!(matcher.matches(&Address::IPv4([10, 255, 255, 255])));
        assert!(!matcher.matches(&Address::IPv4([11, 0, 0, 1])));

        // More specific CIDR
        let matcher2 = CompiledDestinationMatcher::compile("192.168.1.0/24").unwrap();
        assert!(matcher2.matches(&Address::IPv4([192, 168, 1, 100])));
        assert!(!matcher2.matches(&Address::IPv4([192, 168, 2, 100])));
    }

    #[test]
    fn test_domain_matching() {
        let matcher = CompiledDestinationMatcher::compile("example.com").unwrap();

        assert!(matcher.matches(&Address::Domain("example.com".to_string())));
        assert!(matcher.matches(&Address::Domain("EXAMPLE.COM".to_string())));
        assert!(!matcher.matches(&Address::Domain("test.example.com".to_string())));
        assert!(!matcher.matches(&Address::IPv4([192, 168, 1, 1])));
    }

    #[test]
    fn test_wildcard_domain_matching() {
        let matcher = CompiledDestinationMatcher::compile("*.example.com").unwrap();

        assert!(matcher.matches(&Address::Domain("api.example.com".to_string())));
        assert!(matcher.matches(&Address::Domain("www.example.com".to_string())));
        assert!(!matcher.matches(&Address::Domain("example.com".to_string())));
        assert!(!matcher.matches(&Address::Domain("api.test.example.com".to_string())));

        // Test api.*.com pattern
        let matcher2 = CompiledDestinationMatcher::compile("api.*.com").unwrap();
        assert!(matcher2.matches(&Address::Domain("api.example.com".to_string())));
        assert!(matcher2.matches(&Address::Domain("api.test.com".to_string())));
        assert!(!matcher2.matches(&Address::Domain("api.example.org".to_string())));
    }

    #[test]
    fn test_port_matching() {
        // Any
        let any = CompiledPortMatcher::compile("*").unwrap();
        assert!(any.matches(80));
        assert!(any.matches(65535));

        // Single
        let single = CompiledPortMatcher::compile("443").unwrap();
        assert!(single.matches(443));
        assert!(!single.matches(80));

        // Range
        let range = CompiledPortMatcher::compile("8000-9000").unwrap();
        assert!(range.matches(8000));
        assert!(range.matches(8500));
        assert!(range.matches(9000));
        assert!(!range.matches(7999));
        assert!(!range.matches(9001));

        // Multiple
        let multiple = CompiledPortMatcher::compile("80,443,8080").unwrap();
        assert!(multiple.matches(80));
        assert!(multiple.matches(443));
        assert!(multiple.matches(8080));
        assert!(!multiple.matches(8081));
    }

    #[test]
    fn test_rule_matching() {
        let rule = AclRule {
            action: Action::Allow,
            description: "Allow HTTPS".to_string(),
            destinations: vec!["10.0.0.0/8".to_string()],
            ports: vec!["443".to_string()],
            protocols: vec![Protocol::Tcp],
            priority: 100,
        };

        let compiled = CompiledAclRule::compile(&rule).unwrap();

        // Should match: TCP to 10.x.x.x:443
        assert!(compiled.matches(&Address::IPv4([10, 0, 0, 1]), 443, &Protocol::Tcp));

        // Should not match: wrong port
        assert!(!compiled.matches(&Address::IPv4([10, 0, 0, 1]), 80, &Protocol::Tcp));

        // Should not match: wrong IP range
        assert!(!compiled.matches(&Address::IPv4([11, 0, 0, 1]), 443, &Protocol::Tcp));

        // Should not match: wrong protocol
        assert!(!compiled.matches(&Address::IPv4([10, 0, 0, 1]), 443, &Protocol::Udp));
    }

    #[test]
    fn test_wildcard_regex_conversion() {
        let regex = wildcard_to_regex("*.example.com").unwrap();
        let re = Regex::new(&regex).unwrap();

        assert!(re.is_match("api.example.com"));
        assert!(re.is_match("www.example.com"));
        assert!(!re.is_match("example.com"));
        assert!(!re.is_match("api.test.example.com"));

        let regex2 = wildcard_to_regex("api.*.com").unwrap();
        let re2 = Regex::new(&regex2).unwrap();

        assert!(re2.is_match("api.example.com"));
        assert!(re2.is_match("api.test.com"));
        assert!(!re2.is_match("api.example.org"));
    }

    #[test]
    fn test_domain_string_as_ip_matches_cidr_and_ip() {
        let cidr_matcher = CompiledDestinationMatcher::compile("192.168.0.0/16").unwrap();
        assert!(cidr_matcher.matches(&Address::Domain("192.168.55.220".to_string())));
        assert!(!cidr_matcher.matches(&Address::Domain("10.0.0.5".to_string())));

        let ip_matcher = CompiledDestinationMatcher::compile("10.0.0.1").unwrap();
        assert!(ip_matcher.matches(&Address::Domain("10.0.0.1".to_string())));
        assert!(!ip_matcher.matches(&Address::Domain("10.0.0.2".to_string())));
    }
}
