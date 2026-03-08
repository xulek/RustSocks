use super::{SmtpClient, SmtpConfig};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(feature = "database")]
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    Critical,
    Security,
    ConfigChange,
    ServiceStatus,
    ResourcePressure,
    ConnectionPressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSkipReason {
    SmtpDisabled,
    CategoryDisabled,
    NoRecipients,
    CooldownActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationDecision {
    Sent { recipients: usize },
    Skipped(NotificationSkipReason),
}

#[derive(Default)]
struct CooldownState {
    last_sent: HashMap<NotificationKind, u64>,
}

impl CooldownState {
    fn can_send(
        &self,
        kind: NotificationKind,
        cooldown_seconds: u64,
        now: u64,
    ) -> Result<(), NotificationSkipReason> {
        if cooldown_seconds == 0 {
            return Ok(());
        }

        if let Some(last) = self.last_sent.get(&kind) {
            if now < last.saturating_add(cooldown_seconds) {
                return Err(NotificationSkipReason::CooldownActive);
            }
        }

        Ok(())
    }

    fn mark_sent(&mut self, kind: NotificationKind, now: u64) {
        self.last_sent.insert(kind, now);
    }
}

fn cooldown_state() -> &'static Mutex<CooldownState> {
    static STATE: OnceLock<Mutex<CooldownState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CooldownState::default()))
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(target_os = "linux")]
fn read_fd_limit() -> Option<u64> {
    use std::fs;

    let content = fs::read_to_string("/proc/self/limits").ok()?;
    for line in content.lines() {
        if line.starts_with("Max open files") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                return parts[3].parse::<u64>().ok();
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_fd_limit() -> Option<u64> {
    None
}

pub fn notification_gate(
    config: &SmtpConfig,
    kind: NotificationKind,
) -> Result<usize, NotificationSkipReason> {
    if !config.enabled {
        return Err(NotificationSkipReason::SmtpDisabled);
    }

    let category_enabled = match kind {
        NotificationKind::Critical => config.notify_critical,
        NotificationKind::Security => config.notify_security,
        NotificationKind::ConfigChange => config.notify_config_changes,
        NotificationKind::ServiceStatus => config.notify_service_status,
        NotificationKind::ResourcePressure => config.notify_resource_pressure,
        NotificationKind::ConnectionPressure => config.notify_connection_pressure,
    };

    if !category_enabled {
        return Err(NotificationSkipReason::CategoryDisabled);
    }

    let recipients = config.notify_recipients.len();
    if recipients == 0 {
        return Err(NotificationSkipReason::NoRecipients);
    }

    Ok(recipients)
}

fn check_cooldown(
    kind: NotificationKind,
    cooldown_seconds: u64,
) -> Result<(), NotificationSkipReason> {
    let now = current_timestamp();
    let state = cooldown_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.can_send(kind, cooldown_seconds, now)
}

fn mark_notification_sent(kind: NotificationKind) {
    let now = current_timestamp();
    let mut state = cooldown_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.mark_sent(kind, now);
}

pub async fn send_notification(
    config: &SmtpConfig,
    kind: NotificationKind,
    subject: &str,
    body: &str,
) -> Result<NotificationDecision, String> {
    let recipients = match notification_gate(config, kind) {
        Ok(count) => count,
        Err(reason) => return Ok(NotificationDecision::Skipped(reason)),
    };

    if let Err(reason) = check_cooldown(kind, config.notify_cooldown_seconds) {
        return Ok(NotificationDecision::Skipped(reason));
    }

    let client = SmtpClient::new(config.clone());
    for recipient in &config.notify_recipients {
        client.send(recipient, subject, body).await?;
    }

    mark_notification_sent(kind);
    Ok(NotificationDecision::Sent { recipients })
}

#[cfg(feature = "database")]
pub async fn notify_with_pool(
    pool: &sqlx::Pool<sqlx::Any>,
    api_token: Option<String>,
    kind: NotificationKind,
    subject: String,
    body: String,
) -> Result<NotificationDecision, String> {
    use super::repository::SmtpRepository;

    let repo = SmtpRepository::new(pool.clone(), api_token);
    let config = repo.get_config().await?;
    send_notification(&config, kind, &subject, &body).await
}

#[cfg(feature = "database")]
pub async fn resource_monitor_loop(
    pool: sqlx::Pool<sqlx::Any>,
    api_token: Option<String>,
    session_manager: std::sync::Arc<crate::session::SessionManager>,
    max_connections: usize,
    interval_seconds: u64,
) {
    use super::repository::SmtpRepository;
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
    use tokio::time::{sleep, Duration};

    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );

    loop {
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let system_cpu_percent = sys.global_cpu_usage();
        let total_ram = sys.total_memory();
        let used_ram = sys.used_memory();
        let ram_percent = if total_ram > 0 {
            (used_ram as f32 / total_ram as f32) * 100.0
        } else {
            0.0
        };

        let disks = Disks::new_with_refreshed_list();
        let mut max_disk_usage: Option<(String, f32)> = None;
        for disk in disks.iter() {
            let total = disk.total_space();
            if total == 0 {
                continue;
            }
            let used = total.saturating_sub(disk.available_space());
            let percent = (used as f32 / total as f32) * 100.0;
            let mount = disk.mount_point().to_string_lossy().to_string();
            if max_disk_usage
                .as_ref()
                .map(|(_, value)| percent > *value)
                .unwrap_or(true)
            {
                max_disk_usage = Some((mount, percent));
            }
        }

        let active_sessions = session_manager.active_session_count();
        let fd_limit = read_fd_limit();
        let effective_limit = fd_limit
            .map(|limit| max_connections.min(limit as usize))
            .unwrap_or(max_connections);
        let connection_percent = if effective_limit > 0 {
            (active_sessions as f32 / effective_limit as f32) * 100.0
        } else {
            0.0
        };

        let repo = SmtpRepository::new(pool.clone(), api_token.clone());
        let config = match repo.get_config().await {
            Ok(cfg) => cfg,
            Err(err) => {
                warn!("Failed to fetch SMTP config for resource alerts: {}", err);
                continue;
            }
        };

        if config.notify_resource_pressure {
            let mut issues = Vec::new();
            if system_cpu_percent >= config.notify_cpu_threshold as f32 {
                issues.push(format!(
                    "CPU usage is {:.1}% (threshold {}%)",
                    system_cpu_percent, config.notify_cpu_threshold
                ));
            }
            if ram_percent >= config.notify_ram_threshold as f32 {
                issues.push(format!(
                    "RAM usage is {:.1}% (threshold {}%)",
                    ram_percent, config.notify_ram_threshold
                ));
            }
            if let Some((mount, percent)) = max_disk_usage.as_ref() {
                if *percent >= config.notify_disk_threshold as f32 {
                    issues.push(format!(
                        "Disk usage on {} is {:.1}% (threshold {}%)",
                        mount, percent, config.notify_disk_threshold
                    ));
                }
            }

            if !issues.is_empty() {
                let subject = "RustSocks resource alert".to_string();
                let body = format!(
                    "Resource usage crossed configured thresholds:\n\n{}\n",
                    issues.join("\n")
                );
                match send_notification(
                    &config,
                    NotificationKind::ResourcePressure,
                    &subject,
                    &body,
                )
                .await
                {
                    Ok(NotificationDecision::Sent { recipients }) => {
                        info!(
                            "Resource alert notification sent to {} recipient(s)",
                            recipients
                        );
                    }
                    Ok(NotificationDecision::Skipped(_)) => {}
                    Err(err) => {
                        warn!("Failed to send resource alert notification: {}", err);
                    }
                }
            }
        }

        if config.notify_connection_pressure
            && effective_limit > 0
            && connection_percent >= config.notify_connection_percent_threshold as f32
        {
            let subject = "RustSocks connection pressure alert".to_string();
            let limit_label = match fd_limit {
                Some(limit) => format!(
                    "min(server.max_connections={}, fd_limit={})",
                    max_connections, limit
                ),
                None => format!("server.max_connections={}", max_connections),
            };
            let body = format!(
                "Active connections are approaching the configured limit.\n\nActive sessions: {}\nConfigured limit: {}\nUtilization: {:.1}% (threshold {}%)\n",
                active_sessions,
                limit_label,
                connection_percent,
                config.notify_connection_percent_threshold
            );
            match send_notification(
                &config,
                NotificationKind::ConnectionPressure,
                &subject,
                &body,
            )
            .await
            {
                Ok(NotificationDecision::Sent { recipients }) => {
                    info!(
                        "Connection pressure notification sent to {} recipient(s)",
                        recipients
                    );
                }
                Ok(NotificationDecision::Skipped(_)) => {}
                Err(err) => {
                    warn!("Failed to send connection pressure notification: {}", err);
                }
            }
        }

        sleep(Duration::from_secs(interval_seconds.max(5))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smtp::SmtpMode;

    fn base_config() -> SmtpConfig {
        SmtpConfig {
            enabled: true,
            mode: SmtpMode::PlainNoauth,
            host: "smtp.test".to_string(),
            port: 25,
            from_address: "alerts@example.com".to_string(),
            from_name: None,
            username: None,
            password: None,
            has_password: false,
            notify_recipients: vec!["admin@example.com".to_string()],
            notify_critical: true,
            notify_security: true,
            notify_config_changes: true,
            notify_service_status: true,
            notify_resource_pressure: true,
            notify_connection_pressure: true,
            notify_cooldown_seconds: 60,
            notify_cpu_threshold: 85,
            notify_ram_threshold: 85,
            notify_disk_threshold: 90,
            notify_connection_percent_threshold: 85,
        }
    }

    #[test]
    fn notification_gate_requires_smtp_enabled() {
        let mut config = base_config();
        config.enabled = false;
        let result = notification_gate(&config, NotificationKind::Security);
        assert_eq!(result, Err(NotificationSkipReason::SmtpDisabled));
    }

    #[test]
    fn notification_gate_requires_category_enabled() {
        let mut config = base_config();
        config.notify_security = false;
        let result = notification_gate(&config, NotificationKind::Security);
        assert_eq!(result, Err(NotificationSkipReason::CategoryDisabled));
    }

    #[test]
    fn notification_gate_requires_recipients() {
        let mut config = base_config();
        config.notify_recipients.clear();
        let result = notification_gate(&config, NotificationKind::Critical);
        assert_eq!(result, Err(NotificationSkipReason::NoRecipients));
    }

    #[test]
    fn notification_gate_allows_enabled() {
        let config = base_config();
        let result = notification_gate(&config, NotificationKind::ConfigChange);
        assert_eq!(result, Ok(1));
    }

    #[test]
    fn cooldown_blocks_repeated_notifications() {
        let mut state = CooldownState::default();
        let now = 1_000;
        assert_eq!(state.can_send(NotificationKind::Critical, 60, now), Ok(()));
        state.mark_sent(NotificationKind::Critical, now);
        assert_eq!(
            state.can_send(NotificationKind::Critical, 60, now + 30),
            Err(NotificationSkipReason::CooldownActive)
        );
        assert_eq!(state.can_send(NotificationKind::Critical, 60, now + 61), Ok(()));
    }

    #[test]
    fn cooldown_is_not_armed_until_send_succeeds() {
        let mut state = CooldownState::default();
        let now = 2_000;
        assert_eq!(
            state.can_send(NotificationKind::Security, 60, now),
            Ok(())
        );
        assert_eq!(
            state.can_send(NotificationKind::Security, 60, now + 30),
            Ok(())
        );
        state.mark_sent(NotificationKind::Security, now + 30);
        assert_eq!(
            state.can_send(NotificationKind::Security, 60, now + 45),
            Err(NotificationSkipReason::CooldownActive)
        );
    }
}
