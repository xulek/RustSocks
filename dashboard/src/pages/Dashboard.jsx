import React, { useState, useEffect } from 'react'
import { Activity, Users, Shield, TrendingUp } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function Dashboard() {
  const [stats, setStats] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  useEffect(() => {
    fetchStats()
    const interval = setInterval(fetchStats, 5000) // Refresh every 5 seconds
    return () => clearInterval(interval)
  }, [])

  const fetchStats = async () => {
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
  }

  if (loading) return <div className="loading">Loading...</div>
  if (error) return <div className="error">Error: {error}</div>

  const formatBytes = (bytes) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i]
  }

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
              </tr>
            </thead>
            <tbody>
              {stats.top_users.slice(0, 5).map((user, idx) => (
                <tr key={idx}>
                  <td>{user.user}</td>
                  <td>{user.session_count}</td>
                  <td>{formatBytes(user.bytes_sent)}</td>
                  <td>{formatBytes(user.bytes_received)}</td>
                </tr>
              ))}
              {stats.top_users.length === 0 && (
                <tr>
                  <td colSpan="4" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
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
              </tr>
            </thead>
            <tbody>
              {stats.top_destinations.slice(0, 5).map((dest, idx) => (
                <tr key={idx}>
                  <td><code>{dest.destination}</code></td>
                  <td>{dest.session_count}</td>
                  <td>{formatBytes(dest.bytes_sent)}</td>
                  <td>{formatBytes(dest.bytes_received)}</td>
                </tr>
              ))}
              {stats.top_destinations.length === 0 && (
                <tr>
                  <td colSpan="4" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
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
