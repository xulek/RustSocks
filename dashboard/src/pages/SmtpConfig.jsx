import React, { useEffect, useState } from 'react'
import { Mail, Save, Send, AlertCircle, CheckCircle } from 'lucide-react'
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
    has_password: false
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
      setConfig({
        ...data,
        password: ''
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
      const payload = {
        enabled: config.enabled,
        mode: config.mode,
        host: config.host,
        port: config.port,
        from_address: config.from_address,
        from_name: config.from_name || null,
        username: config.username || null,
        password: config.password || null
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
