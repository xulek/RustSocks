import React, { useState, useEffect, useCallback } from 'react'
import { ArrowUpRight } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { getApiUrl } from '../lib/basePath'
import { formatBytes, formatDuration } from '../lib/format'

function Dashboard() {
  const [stats, setStats] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [health, setHealth] = useState(null)
  const [healthError, setHealthError] = useState(null)
  const navigate = useNavigate()

  const fetchStats = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl('/api/sessions/stats'))
      if (!response.ok) throw new Error('Failed to fetch stats')
      const data = await response.json()
      setStats(data)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }, [])

  const fetchHealth = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl('/health'))
      if (!response.ok) throw new Error('Failed to fetch health status')
      const data = await response.json()
      setHealth(data)
      setHealthError(null)
    } catch (err) {
      setHealth(null)
      setHealthError(err.message)
    }
  }, [])

  useEffect(() => {
    fetchStats()
    const interval = setInterval(fetchStats, 5000) // Refresh every 5 seconds
    return () => clearInterval(interval)
  }, [fetchStats])

  useEffect(() => {
    fetchHealth()
    const interval = setInterval(fetchHealth, 15000)
    return () => clearInterval(interval)
  }, [fetchHealth])

  const navigateToSessions = (params) => {
    const search = new URLSearchParams({ view: 'history' })
    if (params.user) search.set('user', params.user)
    if (params.dest_ip) search.set('dest_ip', params.dest_ip)
    navigate({
      pathname: '/sessions',
      search: `?${search.toString()}`
    })
  }

  if (loading) return <div className="loading">Loading...</div>
  if (error) return <div className="error">Error: {error}</div>

  return (
    <div>
      <div className="page-header">
        <h2>Dashboard</h2>
        <p>Real-time overview of your SOCKS5 proxy server</p>
      </div>

      <div className="stats-grid">
        <div className="stat-card">
          <h4>Active Sessions</h4>
          <div className="value">{stats.active_sessions}</div>
          <div className="change">Currently connected</div>
        </div>

        <div className="stat-card">
          <h4>Total Sessions</h4>
          <div className="value">{stats.total_sessions}</div>
          <div className="change">All time</div>
        </div>

        <div className="stat-card">
          <h4>Closed Sessions</h4>
          <div className="value">{stats.closed_sessions}</div>
          <div className="change">Completed</div>
        </div>

        <div className="stat-card">
          <h4>Total Bandwidth</h4>
          <div className="value">
            {formatBytes(stats.total_bytes_sent + stats.total_bytes_received)}
          </div>
          <div className="change">Transferred</div>
        </div>
      </div>

      {health && (
        <div className="card">
          <div className="card-header">
            <h3>System Health</h3>
          </div>
          <div className="health-grid">
            <div>
              <div className="detail-label">Status</div>
              <span className="badge badge-success" style={{ textTransform: 'capitalize' }}>
                {health.status}
              </span>
            </div>
            <div>
              <div className="detail-label">Version</div>
              <div className="detail-value">{health.version}</div>
            </div>
            <div>
              <div className="detail-label">Uptime</div>
              <div className="detail-value">{formatDuration(health.uptime_seconds)}</div>
            </div>
          </div>
        </div>
      )}
      {healthError && (
        <div className="error">Health check unavailable: {healthError}</div>
      )}

      <div className="card">
        <div className="card-header">
          <h3>Top Users by Sessions</h3>
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>User</th>
                <th>Sessions</th>
                <th>Sent</th>
                <th>Received</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {stats.top_users.slice(0, 5).map((user, idx) => (
                <tr key={idx}>
                  <td>{user.user}</td>
                  <td>{user.session_count}</td>
                  <td>{formatBytes(user.bytes_sent)}</td>
                  <td>{formatBytes(user.bytes_received)}</td>
                  <td>
                    <button
                      type="button"
                      className="icon-button"
                      title="Open filtered session history"
                      onClick={() => navigateToSessions({ user: user.user })}
                    >
                      <ArrowUpRight size={16} />
                    </button>
                  </td>
                </tr>
              ))}
              {stats.top_users.length === 0 && (
                <tr>
                  <td colSpan="5" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    No active users
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h3>Top Destinations</h3>
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>Destination</th>
                <th>Connections</th>
                <th>Sent</th>
                <th>Received</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {stats.top_destinations.slice(0, 5).map((dest, idx) => (
                <tr key={idx}>
                  <td><code>{dest.destination}</code></td>
                  <td>{dest.session_count}</td>
                  <td>{formatBytes(dest.bytes_sent)}</td>
                  <td>{formatBytes(dest.bytes_received)}</td>
                  <td>
                    <button
                      type="button"
                      className="icon-button"
                      title="Open history filtered by destination"
                      onClick={() => {
                        const [destIp] = dest.destination.split(':')
                        navigateToSessions({ dest_ip: destIp })
                      }}
                    >
                      <ArrowUpRight size={16} />
                    </button>
                  </td>
                </tr>
              ))}
              {stats.top_destinations.length === 0 && (
                <tr>
                  <td colSpan="5" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    No destinations
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}

export default Dashboard
