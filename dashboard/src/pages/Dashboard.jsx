import React, { useState, useEffect, useCallback, useMemo } from 'react'
import { ArrowUpRight } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { getApiUrl } from '../lib/basePath'
import { formatBytes, formatDuration } from '../lib/format'
import {
  ResponsiveContainer,
  LineChart,
  Line,
  CartesianGrid,
  XAxis,
  YAxis,
  Tooltip,
  AreaChart,
  Area
} from 'recharts'

function Dashboard() {
  const [stats, setStats] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [health, setHealth] = useState(null)
  const [healthError, setHealthError] = useState(null)
  const [statsHistory, setStatsHistory] = useState([])
  const navigate = useNavigate()
  const bandwidthSeries = useMemo(() => {
    if (statsHistory.length === 0) return []
    return statsHistory.map((point, index) => {
      if (index === 0) {
        return { ...point, mbTransferred: 0 }
      }
      const prev = statsHistory[index - 1]
      const delta = Math.max(point.bandwidth - prev.bandwidth, 0)
      return {
        ...point,
        mbTransferred: Number((delta / (1024 * 1024)).toFixed(2))
      }
    })
  }, [statsHistory])

  const fetchStats = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl('/api/sessions/stats'))
      if (!response.ok) throw new Error('Failed to fetch stats')
      const data = await response.json()
      setStats(data)
      setError(null)

      setStatsHistory((prev) => {
        const nextPoint = {
          timestamp: new Date().toISOString(),
          active: data.active_sessions,
          total: data.total_sessions,
          bandwidth: data.total_bytes_sent + data.total_bytes_received
        }
        const next = [...prev, nextPoint]
        // Limit history to last 24 samples (~2 minutes with 5s polling)
        return next.slice(-24)
      })
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

      {statsHistory.length > 1 && (
        <div className="chart-grid">
          <div className="card">
            <div className="card-header">
              <h3>Sesje w czasie</h3>
            </div>
            <div className="chart-wrapper">
              <ResponsiveContainer width="100%" height={260}>
                <LineChart data={statsHistory}>
                  <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" strokeDasharray="3 3" />
                  <XAxis
                    dataKey="timestamp"
                    tickFormatter={(value) => new Date(value).toLocaleTimeString()}
                    stroke="var(--text-secondary)"
                    tick={{ fontSize: 12 }}
                  />
                  <YAxis
                    stroke="var(--text-secondary)"
                    tick={{ fontSize: 12 }}
                    allowDecimals={false}
                  />
                  <Tooltip
                    labelFormatter={(value) => new Date(value).toLocaleTimeString()}
                    formatter={(val, name) => [
                      val,
                      name === 'active' ? 'Aktywne' : 'Łącznie'
                    ]}
                  />
                  <Line
                    type="monotone"
                    dataKey="active"
                    stroke="#38bdf8"
                    strokeWidth={2}
                    dot={false}
                    name="Aktywne"
                  />
                  <Line
                    type="monotone"
                    dataKey="total"
                    stroke="#c084fc"
                    strokeWidth={1.5}
                    dot={false}
                    name="Łącznie"
                  />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="card">
            <div className="card-header">
              <h3>Przepustowość łączna</h3>
            </div>
            <div className="chart-wrapper">
              <ResponsiveContainer width="100%" height={260}>
                <AreaChart data={bandwidthSeries}>
                  <defs>
                    <linearGradient id="bandwidthGradient" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#22d3ee" stopOpacity={0.8} />
                      <stop offset="95%" stopColor="#22d3ee" stopOpacity={0.1} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" strokeDasharray="3 3" />
                  <XAxis
                    dataKey="timestamp"
                    tickFormatter={(value) => new Date(value).toLocaleTimeString()}
                    stroke="var(--text-secondary)"
                    tick={{ fontSize: 12 }}
                  />
                  <YAxis
                    stroke="var(--text-secondary)"
                    tick={{ fontSize: 12 }}
                    tickFormatter={(value) => `${value} MB`}
                    allowDecimals={false}
                  />
                  <Tooltip
                    labelFormatter={(value) => new Date(value).toLocaleTimeString()}
                    formatter={(val) => [`${val} MB`, 'Transfer']}
                  />
                  <Area
                    type="monotone"
                    dataKey="mbTransferred"
                    stroke="#22d3ee"
                    fill="url(#bandwidthGradient)"
                    name="Transfer (MB)"
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>
        </div>
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
