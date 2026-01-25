import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import {
  Activity,
  AlertTriangle,
  AlertCircle,
  CheckCircle,
  ClockIcon,
  FileWarning,
  Gauge,
  Settings,
  TrendingUp,
  XCircle
} from 'lucide-react'

import { getApiUrl } from '../lib/basePath'
import { formatBytes } from '../lib/format'

// ============================================================================
// Constants
// ============================================================================

const SEVERITY_CLASSES = {
  info: 'badge badge-success',
  warning: 'badge badge-warning',
  error: 'badge badge-danger'
}

const TABS = [
  { id: 'metrics', label: 'Metrics', icon: Gauge },
  { id: 'errors', label: 'Errors', icon: XCircle },
  { id: 'alerts', label: 'Alerts', icon: AlertTriangle },
  { id: 'logs', label: 'Logs', icon: FileWarning }
]

// ============================================================================
// Utility Functions
// ============================================================================

const clamp = (value, min, max) => Math.min(Math.max(value, min), max)

const formatTimestamp = (value) => {
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) {
    return value
  }
  return parsed.toLocaleString()
}

const formatPercent = (value) => {
  if (value === null || value === undefined) return '-'
  return `${value.toFixed(1)}%`
}

const formatLatency = (ms) => {
  if (ms === null || ms === undefined) return '-'
  if (ms < 1) return '<1ms'
  if (ms < 1000) return `${Math.round(ms)}ms`
  return `${(ms / 1000).toFixed(2)}s`
}

const truncateDetails = (details) => {
  if (!details) return '-'
  const serialized = JSON.stringify(details)
  if (serialized.length <= 120) return serialized
  return `${serialized.slice(0, 117)}…`
}

// ============================================================================
// Metrics Tab Component
// ============================================================================

function MetricsTab({ minutes }) {
  const [metrics, setMetrics] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  const fetchMetrics = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl(`/api/telemetry/metrics?minutes=${minutes}`))
      if (!response.ok) throw new Error('Failed to fetch metrics')
      const data = await response.json()
      setMetrics(data)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }, [minutes])

  useEffect(() => {
    fetchMetrics()
    const interval = setInterval(fetchMetrics, 15000)
    return () => clearInterval(interval)
  }, [fetchMetrics])

  if (loading) return <div className="loading">Loading metrics...</div>
  if (error) return <div className="error">Error: {error}</div>
  if (!metrics) return null

  const { connections, latency, throughput, pool, system } = metrics

  return (
    <div className="metrics-tab">
      {/* Connection Metrics */}
      <h4 style={{ marginBottom: 12, color: 'var(--text-secondary)' }}>Connections</h4>
      <div className="metrics-grid">
        <div className="metric-card">
          <div className="metric-label">Total Sessions</div>
          <div className="metric-value">{connections.total}</div>
        </div>
        <div className="metric-card highlight">
          <div className="metric-label">Active</div>
          <div className="metric-value">{connections.active}</div>
        </div>
        <div className={`metric-card ${connections.failed > 0 ? 'danger' : ''}`}>
          <div className="metric-label">Failed</div>
          <div className={`metric-value ${connections.failed > 0 ? 'danger' : ''}`}>
            {connections.failed}
          </div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Success Rate</div>
          <div className={`metric-value ${connections.success_rate < 95 ? 'warning' : 'success'}`}>
            {formatPercent(connections.success_rate)}
          </div>
        </div>
      </div>

      {/* Latency Metrics */}
      <h4 style={{ marginBottom: 12, marginTop: 24, color: 'var(--text-secondary)' }}>Connect Latency</h4>
      <div className="metrics-grid">
        <div className="metric-card">
          <div className="metric-label">Average</div>
          <div className="metric-value">{formatLatency(latency.avg_ms)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">P50</div>
          <div className="metric-value">{formatLatency(latency.p50_ms)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">P95</div>
          <div className="metric-value">{formatLatency(latency.p95_ms)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">P99</div>
          <div className="metric-value">{formatLatency(latency.p99_ms)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Max</div>
          <div className="metric-value">{formatLatency(latency.max_ms)}</div>
        </div>
      </div>

      {/* Throughput Metrics */}
      <h4 style={{ marginBottom: 12, marginTop: 24, color: 'var(--text-secondary)' }}>Throughput</h4>
      <div className="metrics-grid">
        <div className="metric-card">
          <div className="metric-label">Bytes Sent</div>
          <div className="metric-value">{formatBytes(throughput.bytes_sent)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Bytes Received</div>
          <div className="metric-value">{formatBytes(throughput.bytes_received)}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Throughput</div>
          <div className="metric-value">{formatBytes(throughput.bytes_per_second)}/s</div>
        </div>
      </div>

      {/* Pool Metrics */}
      <h4 style={{ marginBottom: 12, marginTop: 24, color: 'var(--text-secondary)' }}>Connection Pool</h4>
      <div className="metrics-grid">
        <div className="metric-card">
          <div className="metric-label">Pool Hits</div>
          <div className="metric-value">{pool.hits}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Pool Misses</div>
          <div className="metric-value">{pool.misses}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Hit Rate</div>
          <div className={`metric-value ${pool.hit_rate < 50 ? 'warning' : 'success'}`}>
            {formatPercent(pool.hit_rate)}
          </div>
        </div>
        <div className="metric-card">
          <div className="metric-label">Idle Connections</div>
          <div className="metric-value">{pool.idle_connections}</div>
        </div>
        <div className="metric-card">
          <div className="metric-label">In Use</div>
          <div className="metric-value">{pool.in_use_connections}</div>
        </div>
      </div>

      {/* System Metrics */}
      {system && (
        <>
          <h4 style={{ marginBottom: 12, marginTop: 24, color: 'var(--text-secondary)' }}>System Resources</h4>
          <div className="metrics-grid">
            <div className={`metric-card ${system.memory_usage_percent > 80 ? 'warning' : ''}`}>
              <div className="metric-label">Memory Usage</div>
              <div className={`metric-value ${system.memory_usage_percent > 80 ? 'warning' : ''}`}>
                {formatPercent(system.memory_usage_percent)}
              </div>
              <div className="metric-subtext">{formatBytes(system.memory_usage_bytes)}</div>
            </div>
            {system.cpu_usage_percent !== undefined && (
              <div className={`metric-card ${system.cpu_usage_percent > 80 ? 'warning' : ''}`}>
                <div className="metric-label">CPU Usage</div>
                <div className={`metric-value ${system.cpu_usage_percent > 80 ? 'warning' : ''}`}>
                  {formatPercent(system.cpu_usage_percent)}
                </div>
              </div>
            )}
            {system.open_file_descriptors !== undefined && (
              <div className="metric-card">
                <div className="metric-label">Open FDs</div>
                <div className="metric-value">{system.open_file_descriptors}</div>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}

// ============================================================================
// Errors Tab Component
// ============================================================================

function ErrorsTab({ minutes }) {
  const [errors, setErrors] = useState(null)
  const [loading, setLoading] = useState(true)
  const [fetchError, setFetchError] = useState(null)

  const fetchErrors = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl(`/api/telemetry/errors?minutes=${minutes}`))
      if (!response.ok) throw new Error('Failed to fetch errors')
      const data = await response.json()
      setErrors(data)
      setFetchError(null)
    } catch (err) {
      setFetchError(err.message)
    } finally {
      setLoading(false)
    }
  }, [minutes])

  useEffect(() => {
    fetchErrors()
    const interval = setInterval(fetchErrors, 15000)
    return () => clearInterval(interval)
  }, [fetchErrors])

  if (loading) return <div className="loading">Loading errors...</div>
  if (fetchError) return <div className="error">Error: {fetchError}</div>
  if (!errors) return null

  const { total_errors, error_rate, by_type, by_destination, recent_errors } = errors

  return (
    <div className="errors-tab">
      {/* Summary */}
      <div className="metrics-grid" style={{ marginBottom: 24 }}>
        <div className={`metric-card ${total_errors > 0 ? 'danger' : ''}`}>
          <div className="metric-label">Total Errors</div>
          <div className={`metric-value ${total_errors > 0 ? 'danger' : ''}`}>
            {total_errors}
          </div>
        </div>
        <div className={`metric-card ${error_rate > 5 ? 'warning' : ''}`}>
          <div className="metric-label">Error Rate</div>
          <div className={`metric-value ${error_rate > 5 ? 'warning' : ''}`}>
            {formatPercent(error_rate)}
          </div>
        </div>
      </div>

      {total_errors === 0 ? (
        <div className="empty-state">
          <CheckCircle size={48} className="empty-state-icon" style={{ color: 'var(--success)' }} />
          <h4>No errors</h4>
          <p>No errors or warnings recorded in the selected time period.</p>
        </div>
      ) : (
        <>
          {/* Breakdown */}
          <div className="error-breakdown">
            <div className="breakdown-section">
              <h4>By Type</h4>
              {by_type.map((item) => (
                <div className="breakdown-item" key={item.error_type}>
                  <span className="breakdown-item-label">{item.error_type}</span>
                  <span className="breakdown-item-value">
                    {item.count} ({formatPercent(item.percentage)})
                  </span>
                </div>
              ))}
            </div>

            <div className="breakdown-section">
              <h4>By Destination</h4>
              {by_destination.length > 0 ? (
                by_destination.map((item) => (
                  <div className="breakdown-item" key={item.destination}>
                    <span className="breakdown-item-label" title={item.last_error}>
                      {item.destination.length > 30
                        ? `${item.destination.substring(0, 27)}...`
                        : item.destination}
                    </span>
                    <span className="breakdown-item-value">{item.error_count}</span>
                  </div>
                ))
              ) : (
                <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
                  No destination information available
                </p>
              )}
            </div>
          </div>

          {/* Recent Errors Table */}
          <div className="card" style={{ marginTop: 24 }}>
            <div className="card-header">
              <h3>Recent Errors</h3>
            </div>
            <div className="table-container">
              <table>
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Type</th>
                    <th>Message</th>
                    <th>Destination</th>
                  </tr>
                </thead>
                <tbody>
                  {recent_errors.map((error, idx) => (
                    <tr key={`${error.timestamp}-${idx}`}>
                      <td>{formatTimestamp(error.timestamp)}</td>
                      <td>
                        <span className="badge badge-danger">{error.error_type}</span>
                      </td>
                      <td>{error.message}</td>
                      <td>{error.destination || '-'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </>
      )}
    </div>
  )
}

// ============================================================================
// Alerts Tab Component
// ============================================================================

function AlertsTab({ minutes }) {
  const [alerts, setAlerts] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [showConfig, setShowConfig] = useState(false)
  const [editedThresholds, setEditedThresholds] = useState({})
  const [hasChanges, setHasChanges] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saveMessage, setSaveMessage] = useState(null)

  const fetchAlerts = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl(`/api/telemetry/alerts?minutes=${minutes}`))
      if (!response.ok) throw new Error('Failed to fetch alerts')
      const data = await response.json()
      setAlerts(data)
      // Initialize edited thresholds from fetched data
      const initial = {}
      data.thresholds.forEach((t) => {
        initial[t.metric_name] = {
          warning_threshold: t.warning_threshold,
          error_threshold: t.error_threshold,
          enabled: t.enabled
        }
      })
      setEditedThresholds(initial)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }, [minutes])

  useEffect(() => {
    fetchAlerts()
    const interval = setInterval(fetchAlerts, 15000)
    return () => clearInterval(interval)
  }, [fetchAlerts])

  const handleThresholdChange = (metricName, field, value) => {
    setEditedThresholds((prev) => ({
      ...prev,
      [metricName]: {
        ...prev[metricName],
        [field]: value === '' ? null : parseFloat(value)
      }
    }))
    setHasChanges(true)
    setSaveMessage(null)
  }

  const handleEnabledChange = (metricName, enabled) => {
    setEditedThresholds((prev) => ({
      ...prev,
      [metricName]: {
        ...prev[metricName],
        enabled
      }
    }))
    setHasChanges(true)
    setSaveMessage(null)
  }

  const handleSave = async () => {
    setSaving(true)
    setSaveMessage(null)
    try {
      const thresholdsToUpdate = Object.entries(editedThresholds).map(([metric_name, values]) => ({
        metric_name,
        warning_threshold: values.warning_threshold,
        error_threshold: values.error_threshold,
        enabled: values.enabled
      }))

      const response = await fetch(getApiUrl('/api/telemetry/alerts/config'), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ thresholds: thresholdsToUpdate })
      })

      if (!response.ok) throw new Error('Failed to save thresholds')

      const result = await response.json()
      setSaveMessage({ type: 'success', text: result.message || 'Thresholds saved successfully' })
      setHasChanges(false)
      // Refresh alerts to reflect changes
      fetchAlerts()
    } catch (err) {
      setSaveMessage({ type: 'error', text: err.message })
    } finally {
      setSaving(false)
    }
  }

  const handleReset = () => {
    // Reset to original values from alerts
    const initial = {}
    alerts.thresholds.forEach((t) => {
      initial[t.metric_name] = {
        warning_threshold: t.warning_threshold,
        error_threshold: t.error_threshold,
        enabled: t.enabled
      }
    })
    setEditedThresholds(initial)
    setHasChanges(false)
    setSaveMessage(null)
  }

  if (loading) return <div className="loading">Loading alerts...</div>
  if (error) return <div className="error">Error: {error}</div>
  if (!alerts) return null

  const { active_alerts, thresholds } = alerts

  // Group thresholds by category
  const thresholdsByCategory = thresholds.reduce((acc, t) => {
    if (!acc[t.category]) acc[t.category] = []
    acc[t.category].push(t)
    return acc
  }, {})

  return (
    <div className="alerts-tab">
      {/* Active Alerts */}
      <div className="card-header" style={{ marginBottom: 16 }}>
        <h3>Active Alerts ({active_alerts.length})</h3>
        <button
          className="btn btn-secondary"
          onClick={() => setShowConfig(!showConfig)}
          style={{ display: 'flex', alignItems: 'center', gap: 8 }}
        >
          <Settings size={16} />
          {showConfig ? 'Hide' : 'Configure'} Thresholds
        </button>
      </div>

      {active_alerts.length === 0 ? (
        <div className="empty-state">
          <CheckCircle size={48} className="empty-state-icon" style={{ color: 'var(--success)' }} />
          <h4>All clear</h4>
          <p>No active alerts. All metrics are within acceptable thresholds.</p>
        </div>
      ) : (
        <div className="alert-list">
          {active_alerts.map((alert) => (
            <div className={`alert-item ${alert.severity}`} key={alert.id}>
              <div className="alert-icon">
                {alert.severity === 'error' ? (
                  <AlertCircle size={24} color="var(--danger)" />
                ) : (
                  <AlertTriangle size={24} color="var(--warning)" />
                )}
              </div>
              <div className="alert-content">
                <div className="alert-title">{alert.display_name}</div>
                <div className="alert-message">{alert.message}</div>
                <div className="alert-time">
                  Triggered: {formatTimestamp(alert.triggered_at)}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Threshold Configuration */}
      {showConfig && (
        <div className="card" style={{ marginTop: 24 }}>
          <div className="card-header">
            <h3>Alert Thresholds</h3>
            <p style={{ color: 'var(--text-secondary)', fontSize: 13, marginTop: 4 }}>
              Configure warning and error thresholds for each metric
            </p>
          </div>

          {Object.entries(thresholdsByCategory).map(([category, items]) => (
            <div key={category} style={{ marginBottom: 24 }}>
              <h4 style={{
                marginBottom: 12,
                color: 'var(--text-secondary)',
                textTransform: 'capitalize',
                fontSize: 14
              }}>
                {category}
              </h4>
              <div className="threshold-grid">
                {items.map((t) => {
                  const edited = editedThresholds[t.metric_name] || {}
                  return (
                    <div className="threshold-item" key={t.metric_name}>
                      <div className="threshold-toggle">
                        <input
                          type="checkbox"
                          checked={edited.enabled !== false}
                          onChange={(e) => handleEnabledChange(t.metric_name, e.target.checked)}
                          title="Enable/disable this alert"
                        />
                      </div>
                      <div className="threshold-info" style={{ opacity: edited.enabled === false ? 0.5 : 1 }}>
                        <div className="threshold-name">{t.display_name}</div>
                        <div className="threshold-desc" title={t.description}>{t.description}</div>
                      </div>
                      <div className="threshold-values">
                        <div className="threshold-value">
                          <label>Warning</label>
                          <input
                            type="number"
                            className="warning-input"
                            value={edited.warning_threshold ?? ''}
                            onChange={(e) => handleThresholdChange(t.metric_name, 'warning_threshold', e.target.value)}
                            disabled={edited.enabled === false}
                            placeholder="-"
                          />
                          {t.unit && <span className="unit">{t.unit}</span>}
                        </div>
                        <div className="threshold-value">
                          <label>Error</label>
                          <input
                            type="number"
                            className="error-input"
                            value={edited.error_threshold ?? ''}
                            onChange={(e) => handleThresholdChange(t.metric_name, 'error_threshold', e.target.value)}
                            disabled={edited.enabled === false}
                            placeholder="-"
                          />
                          {t.unit && <span className="unit">{t.unit}</span>}
                        </div>
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>
          ))}

          {/* Save/Reset buttons */}
          <div className="threshold-actions">
            {saveMessage && (
              <div style={{
                flex: 1,
                fontSize: 14,
                color: saveMessage.type === 'success' ? 'var(--success)' : 'var(--danger)'
              }}>
                {saveMessage.text}
              </div>
            )}
            <button
              className="btn btn-secondary"
              onClick={handleReset}
              disabled={!hasChanges || saving}
            >
              Reset
            </button>
            <button
              className="btn btn-primary"
              onClick={handleSave}
              disabled={!hasChanges || saving}
            >
              {saving ? 'Saving...' : 'Save Changes'}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Logs Tab Component (Original Telemetry Events)
// ============================================================================

function LogsTab({ minutes, limit, severity, category, onSettingsChange }) {
  const [events, setEvents] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [lastRefreshed, setLastRefreshed] = useState(null)

  const fetchEvents = useCallback(
    async ({ showLoading = false } = {}) => {
      if (showLoading) setLoading(true)
      setError(null)

      const params = new URLSearchParams()
      params.set('minutes', clamp(minutes, 1, 1440).toString())
      params.set('limit', clamp(limit, 1, 500).toString())
      if (severity !== 'all') params.set('severity', severity)
      if (category.trim()) params.set('category', category.trim())

      try {
        const response = await fetch(getApiUrl(`/api/telemetry/events?${params}`))
        if (!response.ok) throw new Error('Failed to fetch telemetry')
        const data = await response.json()
        setEvents(data)
        setLastRefreshed(new Date().toISOString())
      } catch (err) {
        setError(err.message)
      } finally {
        if (showLoading) setLoading(false)
      }
    },
    [minutes, limit, severity, category]
  )

  useEffect(() => {
    fetchEvents({ showLoading: true })
    const refresher = setInterval(() => fetchEvents(), 15000)
    return () => clearInterval(refresher)
  }, [fetchEvents])

  const handleRefresh = () => fetchEvents({ showLoading: true })

  return (
    <div className="logs-tab">
      {/* Filters */}
      <div className="toolbar" style={{ marginBottom: 16 }}>
        <div className="toolbar-left">
          <ClockIcon size={18} />
          <div className="form-group">
            <label>Time window (minutes)</label>
            <input
              type="number"
              min="1"
              max="1440"
              value={minutes}
              onChange={(e) => onSettingsChange('minutes', clamp(Number(e.target.value) || 1, 1, 1440))}
            />
          </div>
          <div className="form-group">
            <label>Event limit</label>
            <input
              type="number"
              min="1"
              max="500"
              value={limit}
              onChange={(e) => onSettingsChange('limit', clamp(Number(e.target.value) || 1, 1, 500))}
            />
          </div>
          <div className="form-group">
            <label>Severity</label>
            <select value={severity} onChange={(e) => onSettingsChange('severity', e.target.value)}>
              <option value="all">All</option>
              <option value="info">Info</option>
              <option value="warning">Warning</option>
              <option value="error">Error</option>
            </select>
          </div>
          <div className="form-group">
            <label>Category</label>
            <input
              type="text"
              placeholder="e.g. connection_pool"
              value={category}
              onChange={(e) => onSettingsChange('category', e.target.value)}
            />
          </div>
        </div>
        <div className="toolbar-right">
          <button className="btn btn-primary" onClick={handleRefresh} disabled={loading}>
            Refresh
          </button>
        </div>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="card">
        <div className="card-header">
          <div>
            <h3>Recent events ({events.length})</h3>
            {lastRefreshed && (
              <p style={{ color: 'var(--text-secondary)' }}>
                Last refreshed: {formatTimestamp(lastRefreshed)}
              </p>
            )}
          </div>
        </div>

        <div className="table-meta">
          <div className="subtle-text">Data filtered every 15 seconds.</div>
          <div className="subtle-text">{loading ? 'Loading…' : 'Data is current'}</div>
        </div>

        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Severity</th>
                <th>Category</th>
                <th>Message</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {events.length === 0 && !loading ? (
                <tr>
                  <td colSpan="5" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    No events match the filter.
                  </td>
                </tr>
              ) : (
                events.map((event, idx) => (
                  <tr key={`${event.timestamp}-${idx}`}>
                    <td>{formatTimestamp(event.timestamp)}</td>
                    <td>
                      <span className={SEVERITY_CLASSES[event.severity] || 'badge badge-warning'}>
                        {event.severity.toUpperCase()}
                      </span>
                    </td>
                    <td>{event.category}</td>
                    <td>{event.message}</td>
                    <td title={event.details ? JSON.stringify(event.details) : 'None'}>
                      {truncateDetails(event.details)}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Main Telemetry Component
// ============================================================================

function Telemetry() {
  const [searchParams, setSearchParams] = useSearchParams()
  const [activeTab, setActiveTab] = useState(() => searchParams.get('tab') || 'metrics')
  const [alertCount, setAlertCount] = useState(0)

  // Logs tab settings
  const [logsSettings, setLogsSettings] = useState({
    minutes: 60,
    limit: 100,
    severity: 'all',
    category: ''
  })

  // Fetch alert count for badge
  useEffect(() => {
    const fetchAlertCount = async () => {
      try {
        const response = await fetch(getApiUrl('/api/telemetry/alerts?minutes=60'))
        if (response.ok) {
          const data = await response.json()
          setAlertCount(data.active_alerts?.length || 0)
        }
      } catch (e) {
        // Ignore errors for badge
      }
    }
    fetchAlertCount()
    const interval = setInterval(fetchAlertCount, 30000)
    return () => clearInterval(interval)
  }, [])

  const handleTabChange = (tabId) => {
    setActiveTab(tabId)
    setSearchParams({ tab: tabId })
  }

  const handleLogsSettingsChange = (key, value) => {
    setLogsSettings((prev) => ({ ...prev, [key]: value }))
  }

  return (
    <div className="tabs-container">
      <div className="page-header">
        <h2>Operational Telemetry</h2>
        <p>Real-time metrics, diagnostics, and system alerts.</p>
      </div>

      {/* Tabs Header */}
      <div className="tabs-header">
        {TABS.map((tab) => {
          const Icon = tab.icon
          const isActive = activeTab === tab.id
          const showBadge = tab.id === 'alerts' && alertCount > 0

          return (
            <button
              key={tab.id}
              className={`tab-button ${isActive ? 'active' : ''}`}
              onClick={() => handleTabChange(tab.id)}
            >
              <Icon size={18} />
              {tab.label}
              {showBadge && (
                <span className={`tab-badge ${alertCount > 0 ? 'alert' : ''}`}>
                  {alertCount}
                </span>
              )}
            </button>
          )
        })}
      </div>

      {/* Tab Content */}
      <div className="tab-content">
        {activeTab === 'metrics' && <MetricsTab minutes={logsSettings.minutes} />}
        {activeTab === 'errors' && <ErrorsTab minutes={logsSettings.minutes} />}
        {activeTab === 'alerts' && <AlertsTab minutes={logsSettings.minutes} />}
        {activeTab === 'logs' && (
          <LogsTab
            minutes={logsSettings.minutes}
            limit={logsSettings.limit}
            severity={logsSettings.severity}
            category={logsSettings.category}
            onSettingsChange={handleLogsSettingsChange}
          />
        )}
      </div>
    </div>
  )
}

export default Telemetry
