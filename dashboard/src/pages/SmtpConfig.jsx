import React, { useEffect, useState } from 'react'
import {
  Mail,
  Save,
  Send,
  AlertCircle,
  CheckCircle,
  BellRing,
  ShieldAlert,
  Settings2,
  Activity,
  Cpu,
  HardDrive,
  Gauge
} from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

const DEFAULT_MODES = [
  { value: 'plain_noauth', label: 'Plain (no auth)', default_port: 1025, requires_auth: false },
  { value: 'plain_auth', label: 'Plain with auth', default_port: 1025, requires_auth: true },
  { value: 'starttls_noauth', label: 'STARTTLS (no auth)', default_port: 587, requires_auth: false },
  { value: 'starttls_auth', label: 'STARTTLS with auth', default_port: 587, requires_auth: true },
  { value: 'starttls_required', label: 'STARTTLS required', default_port: 587, requires_auth: true },
  { value: 'smtps_noauth', label: 'SMTPS/SSL (no auth)', default_port: 465, requires_auth: false },
  { value: 'smtps_auth', label: 'SMTPS/SSL with auth', default_port: 465, requires_auth: true }
]

function SmtpConfig() {
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [error, setError] = useState(null)
  const [saveStatus, setSaveStatus] = useState(null)
  const [testStatus, setTestStatus] = useState(null)
  const [showTestModal, setShowTestModal] = useState(false)
  const [testRecipient, setTestRecipient] = useState('')
  const [modes, setModes] = useState(DEFAULT_MODES)

  const [config, setConfig] = useState({
    enabled: false,
    mode: 'starttls_auth',
    host: '',
    port: 587,
    from_address: '',
    from_name: '',
    username: '',
    password: '',
    has_password: false,
    notify_recipients: '',
    notify_critical: false,
    notify_security: false,
    notify_config_changes: false,
    notify_service_status: false,
    notify_resource_pressure: false,
    notify_connection_pressure: false,
    notify_cooldown_minutes: 60,
    notify_cpu_threshold: 85,
    notify_ram_threshold: 85,
    notify_disk_threshold: 90,
    notify_connection_percent_threshold: 85
  })

  useEffect(() => {
    fetchModesAndConfig()
  }, [])

  const fetchModesAndConfig = async () => {
    try {
      const [modesResponse, configResponse] = await Promise.all([
        fetch(getApiUrl('/api/smtp/modes')),
        fetch(getApiUrl('/api/smtp/config'))
      ])

      if (modesResponse.ok) {
        const modesData = await modesResponse.json()
        if (Array.isArray(modesData.modes)) {
          setModes(modesData.modes)
        }
      }

      if (!configResponse.ok) {
        const data = await configResponse.json()
        throw new Error(data.error || 'Failed to fetch SMTP configuration')
      }

      const data = await configResponse.json()
      const recipients = Array.isArray(data.notify_recipients) ? data.notify_recipients.join(', ') : ''
      const cooldownSeconds =
        typeof data.notify_cooldown_seconds === 'number' ? data.notify_cooldown_seconds : 3600
      const cooldownMinutes = Math.max(0, Math.round(cooldownSeconds / 60))
      setConfig({
        ...data,
        password: '',
        notify_recipients: recipients,
        notify_critical: data.notify_critical ?? false,
        notify_security: data.notify_security ?? false,
        notify_config_changes: data.notify_config_changes ?? false,
        notify_service_status: data.notify_service_status ?? false,
        notify_resource_pressure: data.notify_resource_pressure ?? false,
        notify_connection_pressure: data.notify_connection_pressure ?? false,
        notify_cooldown_minutes: cooldownMinutes,
        notify_cpu_threshold: data.notify_cpu_threshold ?? 85,
        notify_ram_threshold: data.notify_ram_threshold ?? 85,
        notify_disk_threshold: data.notify_disk_threshold ?? 90,
        notify_connection_percent_threshold: data.notify_connection_percent_threshold ?? 85
      })
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const handleModeChange = (mode) => {
    const modeConfig = modes.find((m) => m.value === mode)
    setConfig((prev) => ({
      ...prev,
      mode,
      port: modeConfig?.default_port || prev.port
    }))
  }

  const handleSave = async () => {
    setSaving(true)
    setSaveStatus(null)
    try {
      const notifyRecipients = config.notify_recipients
        .split(/[\n,]/)
        .map((value) => value.trim())
        .filter((value) => value.length > 0)
      const cooldownMinutes = Number(config.notify_cooldown_minutes) || 0
      const payload = {
        enabled: config.enabled,
        mode: config.mode,
        host: config.host,
        port: config.port,
        from_address: config.from_address,
        from_name: config.from_name || null,
        username: config.username || null,
        password: config.password || null,
        notify_recipients: notifyRecipients,
        notify_critical: config.notify_critical,
        notify_security: config.notify_security,
        notify_config_changes: config.notify_config_changes,
        notify_service_status: config.notify_service_status,
        notify_resource_pressure: config.notify_resource_pressure,
        notify_connection_pressure: config.notify_connection_pressure,
        notify_cooldown_seconds: Math.max(0, Math.round(cooldownMinutes * 60)),
        notify_cpu_threshold: Number(config.notify_cpu_threshold) || 0,
        notify_ram_threshold: Number(config.notify_ram_threshold) || 0,
        notify_disk_threshold: Number(config.notify_disk_threshold) || 0,
        notify_connection_percent_threshold: Number(config.notify_connection_percent_threshold) || 0
      }

      const response = await fetch(getApiUrl('/api/smtp/config'), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      })

      const data = await response.json()
      if (!response.ok) {
        throw new Error(data.error || 'Failed to save configuration')
      }

      setSaveStatus({ success: true, message: 'Configuration saved successfully' })
      setConfig((prev) => ({
        ...prev,
        password: '',
        has_password: !!config.password || prev.has_password
      }))
    } catch (err) {
      setSaveStatus({ success: false, message: err.message })
    } finally {
      setSaving(false)
    }
  }

  const handleTest = async () => {
    if (!testRecipient) return

    setTesting(true)
    setTestStatus(null)
    try {
      const response = await fetch(getApiUrl('/api/smtp/test'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ recipient: testRecipient })
      })

      const data = await response.json()
      setTestStatus({
        success: data.success,
        message: data.message,
        error: data.error
      })
    } catch (err) {
      setTestStatus({ success: false, message: 'Request failed', error: err.message })
    } finally {
      setTesting(false)
    }
  }

  const currentMode = modes.find((m) => m.value === config.mode)
  const requiresAuth = currentMode?.requires_auth || false
  const notifyRecipientsList = config.notify_recipients
    .split(/[\n,]/)
    .map((value) => value.trim())
    .filter((value) => value.length > 0)
  const notificationsEnabled =
    config.notify_critical ||
    config.notify_security ||
    config.notify_config_changes ||
    config.notify_service_status ||
    config.notify_resource_pressure ||
    config.notify_connection_pressure
  const recipientsMissing = notificationsEnabled && notifyRecipientsList.length === 0
  const isConfigReady = (() => {
    if (!config.enabled) return false
    if (!config.host?.trim()) return false
    if (!config.from_address?.trim()) return false
    if (!config.port || Number.isNaN(Number(config.port))) return false
    if (requiresAuth) {
      if (!config.username?.trim()) return false
      if (!config.password?.trim() && !config.has_password) return false
    }
    return true
  })()

  if (loading) {
    return <div className="loading">Loading SMTP configuration...</div>
  }

  return (
    <div>
      <div className="page-header">
        <h2>SMTP Configuration</h2>
        <p>Configure email notifications for alerts and reports</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
        <div className="card-header">
          <h3>
            <Mail size={20} style={{ marginRight: '8px', verticalAlign: 'middle' }} />
            Email Server Settings
          </h3>
        </div>
        <div style={{ padding: '20px' }}>
          <div className="form-section">
            <div className="form-group" style={{ marginBottom: '20px' }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: '8px', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={config.enabled}
                  onChange={(e) => setConfig((prev) => ({ ...prev, enabled: e.target.checked }))}
                  style={{ width: '18px', height: '18px' }}
                />
                <span style={{ fontWeight: 'bold' }}>Enable SMTP</span>
              </label>
            </div>

            <div className="form-grid" style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: '16px' }}>
              <div className="form-group">
                <label>Connection Mode</label>
                <select value={config.mode} onChange={(e) => handleModeChange(e.target.value)}>
                  {modes.map((mode) => (
                    <option key={mode.value} value={mode.value}>
                      {mode.label} (port {mode.default_port})
                    </option>
                  ))}
                </select>
              </div>

              <div className="form-group">
                <label>Port</label>
                <input
                  type="number"
                  min="1"
                  max="65535"
                  value={config.port}
                  onChange={(e) => setConfig((prev) => ({ ...prev, port: Number(e.target.value) || 0 }))}
                />
              </div>

              <div className="form-group" style={{ gridColumn: 'span 2' }}>
                <label>SMTP Host</label>
                <input
                  type="text"
                  placeholder="smtp.example.com"
                  value={config.host}
                  onChange={(e) => setConfig((prev) => ({ ...prev, host: e.target.value }))}
                />
              </div>

              <div className="form-group">
                <label>From Address</label>
                <input
                  type="email"
                  placeholder="alerts@example.com"
                  value={config.from_address}
                  onChange={(e) => setConfig((prev) => ({ ...prev, from_address: e.target.value }))}
                />
              </div>

              <div className="form-group">
                <label>From Name (optional)</label>
                <input
                  type="text"
                  placeholder="RustSocks Alerts"
                  value={config.from_name || ''}
                  onChange={(e) => setConfig((prev) => ({ ...prev, from_name: e.target.value }))}
                />
              </div>

              {requiresAuth && (
                <>
                  <div className="form-group">
                    <label>Username</label>
                    <input
                      type="text"
                      placeholder="SMTP username"
                      value={config.username || ''}
                      onChange={(e) => setConfig((prev) => ({ ...prev, username: e.target.value }))}
                    />
                  </div>

                  <div className="form-group">
                    <label>
                      Password
                      {config.has_password && (
                        <span style={{ color: 'var(--text-secondary)', fontWeight: 'normal', marginLeft: '8px' }}>
                          (leave empty to keep existing)
                        </span>
                      )}
                    </label>
                    <input
                      type="password"
                      placeholder={config.has_password ? '••••••••' : 'SMTP password'}
                      value={config.password}
                      onChange={(e) => setConfig((prev) => ({ ...prev, password: e.target.value }))}
                    />
                  </div>
                </>
              )}
            </div>
          </div>

          <div className="form-section notification-section">
            <div className="notification-header">
              <div className="notification-title">
                <BellRing size={20} />
                <div>
                  <h4>Notification Settings</h4>
                  <div className="form-section-description">
                    Control delivery targets, alert types, and throttling for SMTP notifications.
                  </div>
                </div>
              </div>
              <span className={`notification-status ${notificationsEnabled ? 'on' : 'off'}`}>
                {notificationsEnabled ? 'Alerts enabled' : 'Alerts disabled'}
              </span>
            </div>

            <div className="notification-grid">
              <div className="notification-card">
                <div className="notification-card-header">
                  <Settings2 size={18} />
                  <div>
                    <h5>Recipients</h5>
                    <p>Who should receive alert emails.</p>
                  </div>
                </div>
                <div className="form-group">
                  <label>Notification Recipients</label>
                  <textarea
                    rows="3"
                    placeholder="admin@example.com, oncall@example.com"
                    value={config.notify_recipients}
                    onChange={(e) => setConfig((prev) => ({ ...prev, notify_recipients: e.target.value }))}
                  />
                  <div className="notification-meta">
                    <span className="subtle-text">Separate addresses with commas or new lines.</span>
                    <span className="notification-pill">
                      {notifyRecipientsList.length} recipient{notifyRecipientsList.length === 1 ? '' : 's'}
                    </span>
                  </div>
                  {recipientsMissing && (
                    <span className="subtle-text notification-warning">
                      Add at least one recipient to enable notifications.
                    </span>
                  )}
                </div>
              </div>

              <div className="notification-card">
                <div className="notification-card-header">
                  <ShieldAlert size={18} />
                  <div>
                    <h5>Alert Types</h5>
                    <p>Select the events that should trigger email alerts.</p>
                  </div>
                </div>
                <div className="toggle-list">
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_critical}
                      onChange={(e) =>
                        setConfig((prev) => ({ ...prev, notify_critical: e.target.checked }))
                      }
                    />
                    <span>
                      <strong>Critical failures</strong>
                      <span className="subtle-text">Blocked starts and config errors.</span>
                    </span>
                  </label>

                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_security}
                      onChange={(e) =>
                        setConfig((prev) => ({ ...prev, notify_security: e.target.checked }))
                      }
                    />
                    <span>
                      <strong>Security incidents</strong>
                      <span className="subtle-text">Failed dashboard logins.</span>
                    </span>
                  </label>

                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_config_changes}
                      onChange={(e) =>
                        setConfig((prev) => ({ ...prev, notify_config_changes: e.target.checked }))
                      }
                    />
                    <span>
                      <strong>Config changes</strong>
                      <span className="subtle-text">High-impact updates via dashboard.</span>
                    </span>
                  </label>

                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_service_status}
                      onChange={(e) =>
                        setConfig((prev) => ({ ...prev, notify_service_status: e.target.checked }))
                      }
                    />
                    <span>
                      <strong>Service status</strong>
                      <span className="subtle-text">Start/restart notifications.</span>
                    </span>
                  </label>

                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_resource_pressure}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_resource_pressure: e.target.checked
                        }))
                      }
                    />
                    <span>
                      <strong>Resource pressure</strong>
                      <span className="subtle-text">CPU/RAM/Disk above thresholds.</span>
                    </span>
                  </label>

                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={config.notify_connection_pressure}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_connection_pressure: e.target.checked
                        }))
                      }
                    />
                    <span>
                      <strong>Connection pressure</strong>
                      <span className="subtle-text">Approaching connection limits.</span>
                    </span>
                  </label>
                </div>
              </div>

              <div className="notification-card">
                <div className="notification-card-header">
                  <Activity size={18} />
                  <div>
                    <h5>Thresholds & Cooldown</h5>
                    <p>Throttle alerts and set resource trigger levels.</p>
                  </div>
                </div>
                <div className="threshold-grid">
                  <div className="form-group">
                    <label>Cooldown (minutes)</label>
                    <input
                      type="number"
                      min="0"
                      value={config.notify_cooldown_minutes}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_cooldown_minutes: Number(e.target.value) || 0
                        }))
                      }
                    />
                    <span className="subtle-text">Minimum time between emails per category.</span>
                  </div>

                  <div className="form-group">
                    <label>
                      <Cpu size={14} style={{ marginRight: '6px' }} />
                      CPU Alert (%)
                    </label>
                    <input
                      type="number"
                      min="1"
                      max="100"
                      value={config.notify_cpu_threshold}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_cpu_threshold: Number(e.target.value) || 0
                        }))
                      }
                    />
                  </div>

                  <div className="form-group">
                    <label>
                      <Gauge size={14} style={{ marginRight: '6px' }} />
                      RAM Alert (%)
                    </label>
                    <input
                      type="number"
                      min="1"
                      max="100"
                      value={config.notify_ram_threshold}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_ram_threshold: Number(e.target.value) || 0
                        }))
                      }
                    />
                  </div>

                  <div className="form-group">
                    <label>
                      <HardDrive size={14} style={{ marginRight: '6px' }} />
                      Disk Alert (%)
                    </label>
                    <input
                      type="number"
                      min="1"
                      max="100"
                      value={config.notify_disk_threshold}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_disk_threshold: Number(e.target.value) || 0
                        }))
                      }
                    />
                  </div>

                  <div className="form-group" style={{ gridColumn: 'span 2' }}>
                    <label>Connection Limit Alert (%)</label>
                    <input
                      type="number"
                      min="1"
                      max="100"
                      value={config.notify_connection_percent_threshold}
                      onChange={(e) =>
                        setConfig((prev) => ({
                          ...prev,
                          notify_connection_percent_threshold: Number(e.target.value) || 0
                        }))
                      }
                    />
                    <span className="subtle-text">
                      Uses the lower of server max connections and the Linux FD soft limit.
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', gap: '12px', marginTop: '24px' }}>
            <button
              className="btn btn-primary"
              onClick={handleSave}
              disabled={saving}
              style={{ display: 'flex', alignItems: 'center', gap: '8px' }}
            >
              <Save size={16} />
              {saving ? 'Saving...' : 'Save Configuration'}
            </button>

            <button
              className="btn"
              onClick={() => setShowTestModal(true)}
              disabled={!isConfigReady}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: '8px',
                backgroundColor: 'transparent',
                border: '1px solid var(--border)',
                color: 'var(--text-primary)'
              }}
            >
              <Send size={16} />
              Test Connection
            </button>
          </div>

          {saveStatus && (
            <div
              className={`status-message ${saveStatus.success ? 'success' : 'error'}`}
              style={{ marginTop: '16px', display: 'flex', alignItems: 'center', gap: '8px' }}
            >
              {saveStatus.success ? <CheckCircle size={16} /> : <AlertCircle size={16} />}
              {saveStatus.message}
            </div>
          )}
        </div>
      </div>

      {showTestModal && (
        <div
          className="modal-overlay"
          onClick={(e) => e.target === e.currentTarget && setShowTestModal(false)}
        >
          <div className="modal" style={{ width: '100%', maxWidth: '520px' }}>
            <div className="modal-header">
              <h3>Send Test Email</h3>
              <button className="modal-close" onClick={() => setShowTestModal(false)}>
                &times;
              </button>
            </div>
            <div className="modal-content">
              <div className="form-group">
                <label>Recipient Email</label>
                <input
                  type="email"
                  placeholder="test@example.com"
                  value={testRecipient}
                  onChange={(e) => setTestRecipient(e.target.value)}
                />
              </div>

              {testStatus && (
                <div
                  className={`status-message ${testStatus.success ? 'success' : 'error'}`}
                  style={{ marginTop: '16px' }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    {testStatus.success ? <CheckCircle size={16} /> : <AlertCircle size={16} />}
                    {testStatus.message}
                  </div>
                  {testStatus.error && (
                    <div style={{ marginTop: '8px', fontSize: '12px', color: 'var(--text-secondary)' }}>
                      {testStatus.error}
                    </div>
                  )}
                </div>
              )}
            </div>
            <div className="modal-footer">
              <button
                className="btn btn-secondary"
                onClick={() => setShowTestModal(false)}
              >
                Cancel
              </button>
              <button
                className="btn btn-primary"
                onClick={handleTest}
                disabled={testing || !testRecipient || !isConfigReady}
                style={{ display: 'flex', alignItems: 'center', gap: '8px' }}
              >
                <Send size={16} />
                {testing ? 'Sending...' : 'Send Test'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export default SmtpConfig
