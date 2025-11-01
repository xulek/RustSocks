import React, { useState, useEffect } from 'react'
import { RefreshCw } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function Sessions() {
  const [sessions, setSessions] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [showActive, setShowActive] = useState(true)

  useEffect(() => {
    fetchSessions()
    const interval = setInterval(fetchSessions, 3000) // Refresh every 3 seconds
    return () => clearInterval(interval)
  }, [showActive])

  const fetchSessions = async () => {
    try {
      const endpoint = showActive ? getApiUrl('/api/sessions/active') : getApiUrl('/api/sessions/history')
      const response = await fetch(endpoint)
      if (!response.ok) throw new Error('Failed to fetch sessions')
      const data = await response.json()
      setSessions(Array.isArray(data) ? data : data.sessions || [])
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
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i]
  }

  const formatTime = (timestamp) => {
    return new Date(timestamp).toLocaleString()
  }

  const getStatusBadge = (status) => {
    const statusMap = {
      active: 'badge-success',
      closed: 'badge-warning',
      failed: 'badge-danger',
      rejected_by_acl: 'badge-danger'
    }
    return `badge ${statusMap[status.toLowerCase()] || 'badge-warning'}`
  }

  if (loading) return <div className="loading">Loading sessions...</div>

  return (
    <div>
      <div className="page-header">
        <h2>Sessions</h2>
        <p>Real-time SOCKS5 session monitoring</p>
      </div>

      <div style={{ display: 'flex', gap: '16px', marginBottom: '24px' }}>
        <button
          className={`btn ${showActive ? 'btn-primary' : ''}`}
          onClick={() => setShowActive(true)}
          style={{ backgroundColor: showActive ? '' : 'var(--bg-light)' }}
        >
          Active Sessions ({sessions.filter(s => s.status === 'active').length})
        </button>
        <button
          className={`btn ${!showActive ? 'btn-primary' : ''}`}
          onClick={() => setShowActive(false)}
          style={{ backgroundColor: !showActive ? '' : 'var(--bg-light)' }}
        >
          Session History
        </button>
        <button
          className="btn"
          onClick={fetchSessions}
          style={{ marginLeft: 'auto', backgroundColor: 'var(--bg-light)' }}
        >
          <RefreshCw size={16} />
        </button>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>User</th>
                <th>Source</th>
                <th>Destination</th>
                <th>Protocol</th>
                <th>Status</th>
                <th>Sent</th>
                <th>Received</th>
                <th>Start Time</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session, idx) => (
                <tr key={idx}>
                  <td><strong>{session.user}</strong></td>
                  <td><code>{session.source_ip}:{session.source_port}</code></td>
                  <td><code>{session.dest_ip}:{session.dest_port}</code></td>
                  <td>{session.protocol.toUpperCase()}</td>
                  <td>
                    <span className={getStatusBadge(session.status)}>
                      {session.status}
                    </span>
                  </td>
                  <td>{formatBytes(session.bytes_sent)}</td>
                  <td>{formatBytes(session.bytes_received)}</td>
                  <td>{formatTime(session.start_time)}</td>
                </tr>
              ))}
              {sessions.length === 0 && (
                <tr>
                  <td colSpan="8" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    {showActive ? 'No active sessions' : 'No session history available'}
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

export default Sessions
