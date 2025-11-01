import React, { useState, useEffect } from 'react'
import { getApiUrl } from '../lib/basePath'

function Statistics() {
  const [stats, setStats] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  useEffect(() => {
    fetchStats()
  }, [])

  const fetchStats = async () => {
    try {
      const response = await fetch(getApiUrl('/api/sessions/stats'))
      if (!response.ok) throw new Error('Failed to fetch statistics')
      const data = await response.json()
      setStats(data)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const formatBytes = (bytes) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i]
  }

  if (loading) return <div className="loading">Loading statistics...</div>
  if (error) return <div className="error">Error: {error}</div>

  return (
    <div>
      <div className="page-header">
        <h2>Statistics</h2>
        <p>Detailed analytics and metrics</p>
      </div>

      <div className="stats-grid">
        <div className="stat-card">
          <h4>Total Sessions</h4>
          <div className="value">{stats.total_sessions}</div>
          <div className="change">All time</div>
        </div>

        <div className="stat-card">
          <h4>Active Now</h4>
          <div className="value">{stats.active_sessions}</div>
          <div className="change">Currently connected</div>
        </div>

        <div className="stat-card">
          <h4>Completed</h4>
          <div className="value">{stats.closed_sessions}</div>
          <div className="change">Successfully closed</div>
        </div>

        <div className="stat-card">
          <h4>Failed</h4>
          <div className="value">{stats.failed_sessions}</div>
          <div className="change">Connection errors</div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '24px' }}>
        <div className="card">
          <div className="card-header">
            <h3>Bandwidth by User</h3>
          </div>
          <div className="table-container">
            <table>
              <thead>
                <tr>
                  <th>User</th>
                  <th>Total Bandwidth</th>
                </tr>
              </thead>
              <tbody>
                {stats.top_users.map((user, idx) => (
                  <tr key={idx}>
                    <td><strong>{user.user}</strong></td>
                    <td>{formatBytes(user.bytes_sent + user.bytes_received)}</td>
                  </tr>
                ))}
                {stats.top_users.length === 0 && (
                  <tr>
                    <td colSpan="2" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                      No data available
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
                </tr>
              </thead>
              <tbody>
                {stats.top_destinations.map((dest, idx) => (
                  <tr key={idx}>
                    <td><code>{dest.destination}</code></td>
                    <td>{dest.session_count}</td>
                  </tr>
                ))}
                {stats.top_destinations.length === 0 && (
                  <tr>
                    <td colSpan="2" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                      No data available
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  )
}

export default Statistics
