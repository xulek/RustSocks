-- Add thresholds, cooldown, and additional notification toggles
ALTER TABLE smtp_config
    ADD COLUMN notify_cooldown_seconds INTEGER NOT NULL DEFAULT 3600;

ALTER TABLE smtp_config
    ADD COLUMN notify_cpu_threshold INTEGER NOT NULL DEFAULT 85;

ALTER TABLE smtp_config
    ADD COLUMN notify_ram_threshold INTEGER NOT NULL DEFAULT 85;

ALTER TABLE smtp_config
    ADD COLUMN notify_disk_threshold INTEGER NOT NULL DEFAULT 90;

ALTER TABLE smtp_config
    ADD COLUMN notify_connection_percent_threshold INTEGER NOT NULL DEFAULT 85;

ALTER TABLE smtp_config
    ADD COLUMN notify_resource_pressure INTEGER NOT NULL DEFAULT 0;

ALTER TABLE smtp_config
    ADD COLUMN notify_connection_pressure INTEGER NOT NULL DEFAULT 0;
