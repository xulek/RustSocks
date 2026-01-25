-- Add notification toggles for SMTP email alerts
ALTER TABLE smtp_config
    ADD COLUMN notify_recipients TEXT NOT NULL DEFAULT '';

ALTER TABLE smtp_config
    ADD COLUMN notify_critical INTEGER NOT NULL DEFAULT 0;

ALTER TABLE smtp_config
    ADD COLUMN notify_security INTEGER NOT NULL DEFAULT 0;

ALTER TABLE smtp_config
    ADD COLUMN notify_config_changes INTEGER NOT NULL DEFAULT 0;

ALTER TABLE smtp_config
    ADD COLUMN notify_service_status INTEGER NOT NULL DEFAULT 0;
