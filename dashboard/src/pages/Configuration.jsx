import React, { useState, useEffect } from 'react'
import { Server, Shield, Database, Settings } from 'lucide-react'

function Configuration() {
  const [health, setHealth] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  useEffect(() => {
    fetchHealth()
  }, [])

  const fetchHealth = async () => {
    try {
      const response = await fetch('/health')
      if (!response.ok) throw new Error('Failed to fetch health')
      const data = await response.json()
      setHealth(data)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  if (loading) return <div className="loading">Loading configuration...</div>

  return (
    <div>
      <div className="page-header">
        <h2>Configuration</h2>
        <p>System configuration and health status</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      {health && (
        <div className="card">
          <div className="card-header">
            <h3>Health Status</h3>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: '20px' }}>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '8px' }}>Status</div>
              <div style={{ fontSize: '20px', fontWeight: 'bold' }}>
                <span className="badge badge-success">{health.status}</span>
              </div>
            </div>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '8px' }}>Version</div>
              <div style={{ fontSize: '20px', fontWeight: 'bold' }}>{health.version}</div>
            </div>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '8px' }}>Uptime</div>
              <div style={{ fontSize: '20px', fontWeight: 'bold' }}>
                {Math.floor(health.uptime_seconds / 3600)}h {Math.floor((health.uptime_seconds % 3600) / 60)}m
              </div>
            </div>
          </div>
        </div>
      )}

      <div className="card">
        <div className="card-header">
          <h3>Configuration Info</h3>
        </div>
        <div style={{ color: 'var(--text-secondary)' }}>
          <p style={{ marginBottom: '12px' }}>
            Configuration can be modified by editing the <code>rustsocks.toml</code> file and restarting the server.
          </p>
          <p>
            For dashboard and Swagger settings:
          </p>
          <pre style={{
            backgroundColor: 'var(--bg-dark)',
            padding: '16px',
            borderRadius: '8px',
            overflowX: 'auto',
            marginTop: '12px'
          }}>
{`[sessions]
stats_api_enabled = true
swagger_enabled = true
dashboard_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090`}
          </pre>
        </div>
      </div>

      <div className="card">
        <div className="card-header">
          <h3>API Endpoints</h3>
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>Endpoint</th>
                <th>Description</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td><code>/health</code></td>
                <td>Health check and server status</td>
              </tr>
              <tr>
                <td><code>/metrics</code></td>
                <td>Prometheus metrics</td>
              </tr>
              <tr>
                <td><code>/api/sessions/active</code></td>
                <td>List active SOCKS5 sessions</td>
              </tr>
              <tr>
                <td><code>/api/sessions/history</code></td>
                <td>Session history with filtering</td>
              </tr>
              <tr>
                <td><code>/api/sessions/stats</code></td>
                <td>Aggregated session statistics</td>
              </tr>
              <tr>
                <td><code>/api/acl/groups</code></td>
                <td>List ACL groups</td>
              </tr>
              <tr>
                <td><code>/api/acl/users</code></td>
                <td>List users with ACL rules</td>
              </tr>
              <tr>
                <td><code>/swagger-ui/</code></td>
                <td>Interactive API documentation</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}

export default Configuration
