/// System and kernel settings checks for optimal performance
use std::fs;
use tracing::{warn, info};

/// Recommended kernel parameter values for high-performance proxy server
struct KernelRecommendation {
    name: &'static str,
    path: &'static str,
    min_value: u64,
    recommended: u64,
    description: &'static str,
}

const KERNEL_PARAMS: &[KernelRecommendation] = &[
    KernelRecommendation {
        name: "fs.file-max",
        path: "/proc/sys/fs/file-max",
        min_value: 65536,
        recommended: 262144,
        description: "Maximum number of file handles (affects max connections)",
    },
    KernelRecommendation {
        name: "net.core.somaxconn",
        path: "/proc/sys/net/core/somaxconn",
        min_value: 1024,
        recommended: 4096,
        description: "Maximum backlog for accept() queue",
    },
    KernelRecommendation {
        name: "net.core.netdev_max_backlog",
        path: "/proc/sys/net/core/netdev_max_backlog",
        min_value: 5000,
        recommended: 16384,
        description: "Maximum network device backlog",
    },
    KernelRecommendation {
        name: "net.ipv4.tcp_max_syn_backlog",
        path: "/proc/sys/net/ipv4/tcp_max_syn_backlog",
        min_value: 2048,
        recommended: 8192,
        description: "Maximum SYN backlog for TCP connections",
    },
];

/// Check if a kernel parameter meets minimum requirements
fn check_kernel_param(param: &KernelRecommendation) -> Option<(u64, bool)> {
    match fs::read_to_string(param.path) {
        Ok(content) => {
            if let Ok(value) = content.trim().parse::<u64>() {
                let meets_min = value >= param.min_value;
                Some((value, meets_min))
            } else {
                warn!(
                    "Failed to parse kernel parameter {}: '{}'",
                    param.name,
                    content.trim()
                );
                None
            }
        }
        Err(e) => {
            // Don't warn on non-Linux systems or when file doesn't exist
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "Failed to read kernel parameter {} from {}: {}",
                    param.name, param.path, e
                );
            }
            None
        }
    }
}

/// Check TCP port range configuration
fn check_port_range() {
    let path = "/proc/sys/net/ipv4/ip_local_port_range";
    match fs::read_to_string(path) {
        Ok(content) => {
            let parts: Vec<&str> = content.split_whitespace().collect();
            if parts.len() == 2 {
                if let (Ok(min), Ok(max)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    let range = max - min;
                    if range < 16384 {
                        warn!(
                            "⚠️  net.ipv4.ip_local_port_range is {}-{} ({} ports available)",
                            min, max, range
                        );
                        warn!(
                            "    Recommendation: Increase port range for high connection loads"
                        );
                        warn!("    Suggested: 'sysctl -w net.ipv4.ip_local_port_range=\"10000 65535\"'");
                    }
                }
            }
        }
        Err(_) => {
            // Silently ignore if file doesn't exist (non-Linux)
        }
    }
}

/// Check TCP FIN timeout setting
fn check_tcp_fin_timeout() {
    let path = "/proc/sys/net/ipv4/tcp_fin_timeout";
    match fs::read_to_string(path) {
        Ok(content) => {
            if let Ok(value) = content.trim().parse::<u32>() {
                if value > 30 {
                    warn!(
                        "⚠️  net.ipv4.tcp_fin_timeout is {} seconds (high)",
                        value
                    );
                    warn!(
                        "    Recommendation: Lower to 15-30 seconds to free TIME_WAIT sockets faster"
                    );
                    warn!("    Suggested: 'sysctl -w net.ipv4.tcp_fin_timeout=30'");
                }
            }
        }
        Err(_) => {
            // Silently ignore if file doesn't exist (non-Linux)
        }
    }
}

/// Check TCP TIME_WAIT reuse setting
fn check_tcp_tw_reuse() {
    let path = "/proc/sys/net/ipv4/tcp_tw_reuse";
    match fs::read_to_string(path) {
        Ok(content) => {
            if let Ok(value) = content.trim().parse::<u32>() {
                if value != 2 {
                    warn!(
                        "⚠️  net.ipv4.tcp_tw_reuse is {} (suboptimal)",
                        value
                    );
                    warn!(
                        "    Recommendation: Enable to reuse TIME_WAIT sockets for new connections"
                    );
                    warn!("    Suggested: 'sysctl -w net.ipv4.tcp_tw_reuse=2'");
                }
            }
        }
        Err(_) => {
            // Silently ignore if file doesn't exist (non-Linux)
        }
    }
}

/// Check file descriptor limit (ulimit -n)
fn check_fd_limit() {
    // Try to read from /proc/self/limits
    match fs::read_to_string("/proc/self/limits") {
        Ok(content) => {
            for line in content.lines() {
                if line.starts_with("Max open files") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        // parts[3] is soft limit, parts[4] is hard limit
                        if let Ok(soft_limit) = parts[3].parse::<u64>() {
                            if soft_limit < 65536 {
                                warn!(
                                    "⚠️  File descriptor limit (ulimit -n) is {} (low)",
                                    soft_limit
                                );
                                warn!(
                                    "    Recommendation: Increase to at least 65536 for high loads"
                                );
                                warn!("    Suggested: 'ulimit -n 65536' (or set in /etc/security/limits.conf)");
                            }
                        }
                    }
                    break;
                }
            }
        }
        Err(_) => {
            // Silently ignore if file doesn't exist (non-Linux)
        }
    }
}

/// Check all kernel settings and log warnings for suboptimal configurations
pub fn check_system_settings() {
    info!("Checking system settings for optimal performance...");

    let mut warnings_found = false;

    // Check kernel parameters
    for param in KERNEL_PARAMS {
        if let Some((value, meets_min)) = check_kernel_param(param) {
            if !meets_min {
                warnings_found = true;
                warn!(
                    "⚠️  {} is {} (below minimum {})",
                    param.name, value, param.min_value
                );
                warn!("    {}", param.description);
                warn!(
                    "    Recommendation: 'sysctl -w {}={}'",
                    param.name, param.recommended
                );
            } else if value < param.recommended {
                warn!(
                    "ℹ️  {} is {} (workable, but {} recommended)",
                    param.name, value, param.recommended
                );
            }
        }
    }

    // Check additional settings
    check_port_range();
    check_tcp_fin_timeout();
    check_tcp_tw_reuse();
    check_fd_limit();

    if !warnings_found {
        info!("✓ System settings appear optimal for high-performance operation");
    } else {
        warn!("⚠️  Some system settings are suboptimal. For production deployments,");
        warn!("    consider applying the recommended sysctl changes.");
        warn!("    To make changes permanent, add them to /etc/sysctl.conf");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_system_settings_does_not_panic() {
        // Should not panic on any system (Linux, macOS, Windows)
        check_system_settings();
    }

    #[test]
    fn test_check_kernel_param_nonexistent() {
        let fake_param = KernelRecommendation {
            name: "fake.param",
            path: "/proc/sys/fake/nonexistent",
            min_value: 100,
            recommended: 1000,
            description: "Fake parameter for testing",
        };

        let result = check_kernel_param(&fake_param);
        assert!(result.is_none());
    }
}
