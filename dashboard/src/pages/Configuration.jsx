import React, { useState, useEffect } from 'react'
import {
  Save,
  Shield,
  SlidersHorizontal,
  Server,
  Database,
  Network,
  Activity,
  BarChart2
} from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function Configuration() {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [globalSettings, setGlobalSettings] = useState(null)
  const [defaultPolicy, setDefaultPolicy] = useState('block')
  const [saving, setSaving] = useState(false)
  const [saveSuccess, setSaveSuccess] = useState(null)
  const [configFile, setConfigFile] = useState(null)
  const [runtimeConfig, setRuntimeConfig] = useState(null)
  const [runtimeLoading, setRuntimeLoading] = useState(true)
  const [runtimeError, setRuntimeError] = useState(null)
  const [runtimeDirty, setRuntimeDirty] = useState(false)
  const [runtimeSaving, setRuntimeSaving] = useState(false)
  const [runtimeStatus, setRuntimeStatus] = useState(null)
  const [restartPending, setRestartPending] = useState(false)
  const [restartAfterSave, setRestartAfterSave] = useState(true)

  useEffect(() => {
    fetchGlobalSettings()
    fetchRuntimeConfig()
  }, [])

  const fetchGlobalSettings = async () => {
    try {
      const response = await fetch(getApiUrl('/api/acl/global'))
      if (!response.ok) throw new Error('Failed to fetch global settings')
      const data = await response.json()
      setGlobalSettings(data)
      setDefaultPolicy(data.default_policy)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const handleSave = async () => {
    setSaving(true)
    setSaveSuccess(null)
    try {
      const response = await fetch(getApiUrl('/api/acl/global'), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ default_policy: defaultPolicy })
      })

      const data = await response.json()
      if (!response.ok) {
        throw new Error(data.message || 'Failed to update settings')
      }

      setSaveSuccess({
        success: true,
        message: `Global policy changed from "${data.old_policy}" to "${data.new_policy}"`
      })
      await fetchGlobalSettings()
    } catch (err) {
      setSaveSuccess({
        success: false,
        message: err.message
      })
    } finally {
      setSaving(false)
    }
  }

  const fetchRuntimeConfig = async () => {
    setRuntimeLoading(true)
    setRuntimeError(null)
    try {
      const response = await fetch(getApiUrl('/api/admin/runtime-config'))
      const data = await response.json()
      if (!response.ok) {
        throw new Error(data.message || 'Failed to load runtime configuration')
      }
      setConfigFile({ path: data.path, editable: data.editable })
      setRuntimeConfig({
        server: data.server,
        pool: data.pool,
        sessions: data.sessions,
        metrics: data.metrics,
        telemetry: data.telemetry
      })
      setRuntimeDirty(false)
      setRuntimeStatus(null)
      setRestartPending(false)
    } catch (err) {
      setRuntimeError(err.message)
    } finally {
      setRuntimeLoading(false)
    }
  }

  const updateField = (section, field, value) => {
    setRuntimeConfig((prev) => {
      if (!prev) return prev
      return {
        ...prev,
        [section]: {
          ...prev[section],
          [field]: value
        }
      }
    })
    setRuntimeDirty(true)
    setRuntimeStatus(null)
    setRestartPending(false)
  }

  const handleRuntimeSave = async () => {
    if (!runtimeConfig || (configFile && !configFile.editable)) return
    setRuntimeSaving(true)
    setRuntimeStatus(null)
    try {
      const payload = {
        server: runtimeConfig.server,
        pool: runtimeConfig.pool,
        sessions: runtimeConfig.sessions,
        metrics: runtimeConfig.metrics,
        telemetry: runtimeConfig.telemetry,
        restart: restartAfterSave
      }
      const response = await fetch(getApiUrl('/api/admin/runtime-config'), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      })
      const data = await response.json()
      if (!response.ok) {
        throw new Error(data.message || 'Failed to save configuration')
      }
      setRuntimeStatus({
        success: true,
        message: data.message,
        restarting: data.restarting
      })
      setRuntimeDirty(false)
      if (data.restarting) {
        setRestartPending(true)
      } else {
        await fetchRuntimeConfig()
      }
    } catch (err) {
      setRuntimeStatus({
        success: false,
        message: err.message
      })
    } finally {
      setRuntimeSaving(false)
    }
  }

  if (loading) return <div className="loading">Loading configuration...</div>

  const SectionCard = ({ icon: Icon, title, description, children }) => (
    <div className="form-section">
      <div className="form-section-header">
        <div>
          <h4>
            {Icon && <Icon size={18} />}
            {title}
          </h4>
          {description && <p className="form-section-description">{description}</p>}
        </div>
      </div>
      <div className="form-grid">{children}</div>
    </div>
  )

  return (
    <div>
      <div className="page-header">
        <h2>Configuration</h2>
        <p>Manage global ACL settings and server configuration</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
        <div className="card-header">
          <h3>
            <Shield size={20} style={{ marginRight: '8px', verticalAlign: 'middle' }} />
            Global ACL Settings
          </h3>
        </div>
        <div style={{ padding: '20px' }}>
          <div className="form-group">
            <label style={{ fontWeight: 'bold', marginBottom: '8px', display: 'block' }}>
              Default Policy
            </label>
            <p style={{ color: 'var(--text-secondary)', marginBottom: '16px', fontSize: '14px' }}>
              Determines the default action for connections that don’t match any ACL rule.
            </p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', marginBottom: '20px' }}>
              <label style={{ display: 'flex', alignItems: 'flex-start', gap: '8px', cursor: 'pointer', padding: '12px', border: '1px solid var(--border)', borderRadius: '8px', transition: 'border-color 0.2s', ':hover': { borderColor: 'var(--primary)' } }}>
                <input
                  type="radio"
                  name="defaultPolicy"
                  value="allow"
                  checked={defaultPolicy === 'allow'}
                  onChange={(e) => setDefaultPolicy(e.target.value)}
                  style={{ marginTop: '2px', width: '16px', height: '16px', flexShrink: 0 }}
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ marginBottom: '8px' }}>
                    <span className="badge badge-success">ALLOW</span>
                  </div>
                  <div style={{ color: 'var(--text-secondary)', fontSize: '14px', lineHeight: '1.5' }}>
                    Allow all connections (whitelist mode - default allow)
                  </div>
                </div>
              </label>
              <label style={{ display: 'flex', alignItems: 'flex-start', gap: '8px', cursor: 'pointer', padding: '12px', border: '1px solid var(--border)', borderRadius: '8px', transition: 'border-color 0.2s' }}>
                <input
                  type="radio"
                  name="defaultPolicy"
                  value="block"
                  checked={defaultPolicy === 'block'}
                  onChange={(e) => setDefaultPolicy(e.target.value)}
                  style={{ marginTop: '2px', width: '16px', height: '16px', flexShrink: 0 }}
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ marginBottom: '8px' }}>
                    <span className="badge badge-danger">BLOCK</span>
                  </div>
                  <div style={{ color: 'var(--text-secondary)', fontSize: '14px', lineHeight: '1.5' }}>
                    Block all connections (blacklist mode - default block)
                  </div>
                </div>
              </label>
            </div>
          </div>

          <button
            className="btn btn-primary"
            onClick={handleSave}
            disabled={saving || (globalSettings && globalSettings.default_policy === defaultPolicy)}
            style={{ display: 'flex', alignItems: 'center', gap: '8px' }}
          >
            <Save size={16} />
            {saving ? 'Saving...' : 'Save Changes'}
          </button>

          {saveSuccess && (
            <div
              className={`status-message ${saveSuccess.success ? 'success' : 'error'}`}
              style={{ marginTop: '16px' }}
            >
              {saveSuccess.message}
            </div>
          )}

          <div className="guidelines-wrapper">
            <h4>Policy Guidelines</h4>
            <div className="guidelines-grid">
              <div className="guideline-card">
                <h5>
                  <span className="badge badge-success">ALLOW</span>
                  Default Policy
                </h5>
                <p>
                  Default allows traffic – only entries matching BLOCK rules are denied.
                </p>
                <ul className="guideline-list">
                  <li>When you only block a few hosts (blacklist).</li>
                  <li>Most traffic should be permitted.</li>
                  <li>Development or testing environments.</li>
                </ul>
              </div>
              <div className="guideline-card">
                <h5>
                  <span className="badge badge-danger">BLOCK</span>
                  Default Policy
                </h5>
                <p>
                  Default blocks traffic – only ALLOW entries are forwarded.
                </p>
                <ul className="guideline-list">
                  <li>Production environments using a whitelist.</li>
                  <li>Zero-trust with full control over new destinations.</li>
                  <li>When security outweighs convenience.</li>
                </ul>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: '24px' }}>
        <div className="card-header">
          <h3>
            <SlidersHorizontal size={20} style={{ marginRight: '8px', verticalAlign: 'middle' }} />
            Runtime Configuration
          </h3>
        </div>
        <div style={{ padding: '20px', display: 'flex', flexDirection: 'column', gap: '16px' }}>
          {runtimeLoading ? (
            <div className="loading">Loading settings...</div>
          ) : runtimeError ? (
            <div className="error">{runtimeError}</div>
          ) : configFile && !configFile.editable ? (
            <div className="error">
              Server started without a configuration file. Editing is disabled.
            </div>
          ) : runtimeConfig ? (
            <>
              {configFile?.path && (
                <div className="subtle-text">
                  Active file: <code>{configFile.path}</code>
                </div>
              )}

              <SectionCard
                icon={Server}
                title="Server"
                description="SOCKS5 listen address along with dashboard components."
              >
                <div className="form-group">
                  <label>Bind address</label>
                  <input
                    type="text"
                    value={runtimeConfig.server.bind_address}
                    onChange={(e) => updateField('server', 'bind_address', e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label>Port</label>
                  <input
                    type="number"
                    min="1"
                    max="65535"
                    value={runtimeConfig.server.bind_port}
                    onChange={(e) => updateField('server', 'bind_port', Number(e.target.value) || 0)}
                  />
                </div>
                <div className="form-group">
                  <label>Dashboard</label>
                  <select
                    value={runtimeConfig.server.dashboard_enabled ? 'true' : 'false'}
                    onChange={(e) => updateField('server', 'dashboard_enabled', e.target.value === 'true')}
                  >
                    <option value="true">Enabled</option>
                    <option value="false">Disabled</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Swagger UI</label>
                  <select
                    value={runtimeConfig.server.swagger_enabled ? 'true' : 'false'}
                    onChange={(e) => updateField('server', 'swagger_enabled', e.target.value === 'true')}
                  >
                    <option value="true">Enabled</option>
                    <option value="false">Disabled</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Stats API</label>
                  <select
                    value={runtimeConfig.server.stats_api_enabled ? 'true' : 'false'}
                    onChange={(e) =>
                      updateField('server', 'stats_api_enabled', e.target.value === 'true')
                    }
                  >
                    <option value="true">Enabled</option>
                    <option value="false">Disabled</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Stats API address</label>
                  <input
                    type="text"
                    value={runtimeConfig.server.stats_api_bind_address}
                    disabled={!runtimeConfig.server.stats_api_enabled}
                    onChange={(e) =>
                      updateField('server', 'stats_api_bind_address', e.target.value)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Stats API port</label>
                  <input
                    type="number"
                    min="1"
                    max="65535"
                    value={runtimeConfig.server.stats_api_port}
                    disabled={!runtimeConfig.server.stats_api_enabled}
                    onChange={(e) =>
                      updateField('server', 'stats_api_port', Number(e.target.value) || 0)
                    }
                  />
                </div>
              </SectionCard>

              <SectionCard
                icon={Database}
                title="Sessions & API"
                description="Session storage engine and dashboard/statistics settings."
              >
                <div className="form-group">
                  <label>Storage</label>
                  <select
                    value={runtimeConfig.sessions.storage}
                    onChange={(e) => updateField('sessions', 'storage', e.target.value)}
                  >
                    <option value="memory">memory</option>
                    <option value="sqlite">sqlite</option>
                  </select>
                </div>
                {runtimeConfig.sessions.storage === 'sqlite' && (
                  <div className="form-group">
                    <label>SQLite URL</label>
                    <input
                      type="text"
                      value={runtimeConfig.sessions.database_url || ''}
                      onChange={(e) => updateField('sessions', 'database_url', e.target.value || null)}
                    />
                  </div>
                )}
                <div className="form-group">
                  <label>Retention (days)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.sessions.retention_days}
                    onChange={(e) => updateField('sessions', 'retention_days', Number(e.target.value) || 0)}
                  />
                </div>
                <div className="form-group">
                  <label>Cleanup interval (h)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.sessions.cleanup_interval_hours}
                    onChange={(e) =>
                      updateField('sessions', 'cleanup_interval_hours', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Traffic update interval</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.sessions.traffic_update_packet_interval}
                    onChange={(e) =>
                      updateField('sessions', 'traffic_update_packet_interval', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Stats window (h)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.sessions.stats_window_hours}
                    onChange={(e) =>
                      updateField('sessions', 'stats_window_hours', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group" style={{ gridColumn: 'span 2' }}>
                  <label>Base path</label>
                  <input
                    type="text"
                    value={runtimeConfig.sessions.base_path}
                    onChange={(e) => updateField('sessions', 'base_path', e.target.value)}
                  />
                </div>
              </SectionCard>

              <SectionCard
                icon={Network}
                title="Connection Pool"
                description="How many connections we buffer and how quickly they expire."
              >
                <div className="form-group">
                  <label>Enabled</label>
                  <select
                    value={runtimeConfig.pool.enabled ? 'true' : 'false'}
                    onChange={(e) => updateField('pool', 'enabled', e.target.value === 'true')}
                  >
                    <option value="true">Yes</option>
                    <option value="false">No</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Max idle per destination</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.pool.max_idle_per_dest}
                    onChange={(e) =>
                      updateField('pool', 'max_idle_per_dest', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Max total idle</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.pool.max_total_idle}
                    onChange={(e) =>
                      updateField('pool', 'max_total_idle', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Idle timeout (s)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.pool.idle_timeout_secs}
                    onChange={(e) =>
                      updateField('pool', 'idle_timeout_secs', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Connect timeout (ms)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.pool.connect_timeout_ms}
                    onChange={(e) =>
                      updateField('pool', 'connect_timeout_ms', Number(e.target.value) || 0)
                    }
                  />
                </div>
              </SectionCard>

              <SectionCard
                icon={BarChart2}
                title="Metrics & Telemetry"
                description="Metrics history and operational alerts."
              >
                <div className="form-group">
                  <label>Metrics enabled</label>
                  <select
                    value={runtimeConfig.metrics.enabled ? 'true' : 'false'}
                    onChange={(e) => updateField('metrics', 'enabled', e.target.value === 'true')}
                  >
                    <option value="true">Yes</option>
                    <option value="false">No</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Storage</label>
                  <select
                    value={runtimeConfig.metrics.storage}
                    onChange={(e) => updateField('metrics', 'storage', e.target.value)}
                  >
                    <option value="memory">memory</option>
                    <option value="sqlite">sqlite</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Retention (h)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.metrics.retention_hours}
                    onChange={(e) =>
                      updateField('metrics', 'retention_hours', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Cleanup interval (h)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.metrics.cleanup_interval_hours}
                    onChange={(e) =>
                      updateField('metrics', 'cleanup_interval_hours', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Collection interval (s)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.metrics.collection_interval_secs}
                    onChange={(e) =>
                      updateField('metrics', 'collection_interval_secs', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Telemetry enabled</label>
                  <select
                    value={runtimeConfig.telemetry.enabled ? 'true' : 'false'}
                    onChange={(e) => updateField('telemetry', 'enabled', e.target.value === 'true')}
                  >
                    <option value="true">Yes</option>
                    <option value="false">No</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Telemetry max events</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.telemetry.max_events}
                    onChange={(e) =>
                      updateField('telemetry', 'max_events', Number(e.target.value) || 0)
                    }
                  />
                </div>
                <div className="form-group">
                  <label>Telemetry retention (h)</label>
                  <input
                    type="number"
                    min="1"
                    value={runtimeConfig.telemetry.retention_hours}
                    onChange={(e) =>
                      updateField('telemetry', 'retention_hours', Number(e.target.value) || 0)
                    }
                  />
                </div>
              </SectionCard>

              <label className="restart-toggle">
                <input
                  type="checkbox"
                  checked={restartAfterSave}
                  onChange={(e) => setRestartAfterSave(e.target.checked)}
                />
                Restart after save
              </label>

              <div style={{ display: 'flex', gap: '12px' }}>
                <button
                  className="btn btn-primary"
                  onClick={handleRuntimeSave}
                  disabled={
                    runtimeSaving ||
                    !runtimeDirty ||
                    restartPending ||
                    (configFile && !configFile.editable)
                  }
                  style={{ display: 'flex', alignItems: 'center', gap: '8px' }}
                >
                  <Save size={16} />
                  {runtimeSaving ? 'Saving...' : 'Save Changes'}
                </button>
                <button
                  className="btn"
                  onClick={fetchRuntimeConfig}
                  disabled={runtimeSaving}
                  style={{
                    backgroundColor: 'transparent',
                    border: '1px solid var(--border)',
                    color: 'var(--text-secondary)'
                  }}
                >
                  Reset
                </button>
              </div>

              {runtimeStatus && (
                <div
                  className={`status-message ${runtimeStatus.success ? 'success' : 'error'}`}
                  style={{ marginTop: '8px' }}
                >
                  {runtimeStatus.message}
                </div>
              )}
              {restartPending && (
                <div className="subtle-text">
                  Waiting for server restart... The page will refresh once reconnected.
                </div>
              )}
            </>
          ) : null}
        </div>
      </div>

    </div>
  )
}

export default Configuration
