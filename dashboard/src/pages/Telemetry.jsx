import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { ClockIcon } from 'lucide-react'

import { getApiUrl } from '../lib/basePath'

const SEVERITY_CLASSES = {
  info: 'badge badge-success',
  warning: 'badge badge-warning',
  error: 'badge badge-danger'
}

const clamp = (value, min, max) => Math.min(Math.max(value, min), max)

const formatTimestamp = (value) => {
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) {
    return value
  }
  return parsed.toLocaleString()
}

const truncateDetails = (details) => {
  if (!details) {
    return '-'
  }
  const serialized = JSON.stringify(details)
  if (serialized.length <= 120) {
    return serialized
  }
  return `${serialized.slice(0, 117)}…`
}

function Telemetry() {
  const [events, setEvents] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [minutes, setMinutes] = useState(60)
  const [limit, setLimit] = useState(100)
  const [severity, setSeverity] = useState('all')
  const [category, setCategory] = useState('')
  const [lastRefreshed, setLastRefreshed] = useState(null)

  const fetchEvents = useCallback(
    async ({ showLoading = false } = {}) => {
      if (showLoading) {
        setLoading(true)
      }
      setError(null)

      const params = new URLSearchParams()
      params.set('minutes', clamp(minutes, 1, 1440).toString())
      params.set('limit', clamp(limit, 1, 500).toString())
      if (severity !== 'all') {
        params.set('severity', severity)
      }
      if (category.trim()) {
        params.set('category', category.trim())
      }

      const query = params.toString()
      const url = getApiUrl(`/api/telemetry/events${query ? `?${query}` : ''}`)

      try {
        const response = await fetch(url)
        if (!response.ok) {
          throw new Error('Failed to fetch telemetry')
        }
        const data = await response.json()
        setEvents(data)
        setLastRefreshed(new Date().toISOString())
      } catch (err) {
        setError(err.message)
      } finally {
        if (showLoading) {
          setLoading(false)
        }
      }
    },
    [minutes, limit, severity, category]
  )

  useEffect(() => {
    fetchEvents({ showLoading: true })
    const refresher = setInterval(() => {
      fetchEvents()
    }, 15000)
    return () => clearInterval(refresher)
  }, [fetchEvents])

  const handleRefresh = () => {
    fetchEvents({ showLoading: true })
  }

  const memoizedCount = useMemo(() => events.length, [events])
  const formattedRefreshed = useMemo(() => {
    if (!lastRefreshed) return null
    return formatTimestamp(lastRefreshed)
  }, [lastRefreshed])

  return (
    <div>
      <div className="page-header">
        <h2>Operational Telemetry</h2>
        <p>Latest alerts about connection pool pressure and upstream errors.</p>
      </div>

      <div className="toolbar">
        <div className="toolbar-left">
          <ClockIcon size={18} />
          <div className="form-group">
            <label>Time window (minutes)</label>
            <input
              type="number"
              min="1"
              max="1440"
              value={minutes}
              onChange={(event) => setMinutes(clamp(Number(event.target.value) || 1, 1, 1440))}
            />
          </div>
          <div className="form-group">
            <label>Event limit</label>
            <input
              type="number"
              min="1"
              max="500"
              value={limit}
              onChange={(event) => setLimit(clamp(Number(event.target.value) || 1, 1, 500))}
            />
          </div>
          <div className="form-group">
            <label>Severity</label>
            <select value={severity} onChange={(event) => setSeverity(event.target.value)}>
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
              onChange={(event) => setCategory(event.target.value)}
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
            <h3>Recent events ({memoizedCount})</h3>
            {formattedRefreshed && <p style={{ color: 'var(--text-secondary)' }}>Last refreshed: {formattedRefreshed}</p>}
          </div>
        </div>

        <div className="table-meta">
          <div className="subtle-text">
            Data filtered every 15 seconds. Increase the limit or time window above if needed.
          </div>
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
                events.map((event) => (
                  <tr key={event.timestamp + event.message}>
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

export default Telemetry
