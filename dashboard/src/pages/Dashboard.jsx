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
  Legend,
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
  const [poolStats, setPoolStats] = useState(null)
  const [poolError, setPoolError] = useState(null)

  // Load timeRange from localStorage
  const [timeRange, setTimeRange] = useState(() => {
    try {
      const stored = localStorage.getItem('rustsocks_dashboard_timerange')
      if (stored) {
        const parsed = parseInt(stored, 10)
        if (!isNaN(parsed) && parsed > 0) {
          return parsed
        }
      }
    } catch (e) {
      console.warn('Failed to load timeRange from localStorage:', e)
    }
    return 15 // default: 15 minutes
  })
  const navigate = useNavigate()

  // Oblicz zakres czasu dla osi X
  const timeWindow = useMemo(() => {
    if (statsHistory.length === 0) return { start: Date.now() - timeRange * 60 * 1000, end: Date.now() }

    const lastPoint = statsHistory[statsHistory.length - 1]
    const endTime = new Date(lastPoint.timestamp).getTime()
    const startTime = endTime - timeRange * 60 * 1000

    return { start: startTime, end: endTime }
  }, [statsHistory, timeRange])

  const chartHistory = useMemo(() => {
    if (statsHistory.length === 0) {
      return []
    }

    // Filtruj dane według wybranego zakresu czasowego
    const cutoffTime = timeWindow.start

    const filteredHistory = statsHistory
      .filter(point => {
        const pointTime = new Date(point.timestamp).getTime()
        return pointTime >= cutoffTime
      })
      .map(point => ({
        ...point,
        timestamp: new Date(point.timestamp).getTime() // Konwertuj na milisekundy dla XAxis
      }))

    if (filteredHistory.length === 0 && statsHistory.length > 0) {
      // Jeśli po filtrowaniu nie ma danych, pokaż ostatni punkt
      const last = statsHistory[statsHistory.length - 1]
      return [{
        ...last,
        timestamp: new Date(last.timestamp).getTime()
      }]
    }

    if (filteredHistory.length === 1) {
      const single = filteredHistory[0]
      const earlier = single.timestamp - 1000
      return [
        { ...single, timestamp: earlier },
        single
      ]
    }

    return filteredHistory
  }, [statsHistory, timeWindow])

  const bandwidthSeries = useMemo(() => {
    if (chartHistory.length === 0) return []
    return chartHistory.map((point, index, array) => {
      if (index === 0) {
        return { ...point, mbTransferred: 0 }
      }
      const prev = array[index - 1]
      const delta = Math.max(point.bandwidth - prev.bandwidth, 0)
      return {
        ...point,
        mbTransferred: Number((delta / (1024 * 1024)).toFixed(2))
      }
    })
  }, [chartHistory])

  const topDestinations = useMemo(() => {
    if (!poolStats || !poolStats.destinations_breakdown) return []
    return poolStats.destinations_breakdown.slice(0, 8)
  }, [poolStats])

  const formatPoolTimestamp = (value) => {
    if (!value) return '—'
    try {
      return new Date(value).toLocaleTimeString()
    } catch (err) {
      return value
    }
  }

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

  const fetchMetricsHistory = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl('/api/metrics/history'))
      if (!response.ok) throw new Error('Failed to fetch metrics history')
      const data = await response.json()

      // Transform backend data to frontend format
      const transformed = data.map(snapshot => ({
        timestamp: snapshot.timestamp,
        active: snapshot.active_sessions,
        total: snapshot.total_sessions,
        bandwidth: snapshot.bandwidth
      }))

      setStatsHistory(transformed)
    } catch (err) {
      console.warn('Failed to fetch metrics history:', err)
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

  const fetchPoolStats = useCallback(async () => {
    try {
      const response = await fetch(getApiUrl('/api/pool/stats'))
      if (!response.ok) throw new Error('Failed to fetch pool stats')
      const data = await response.json()
      setPoolStats(data)
      setPoolError(null)
    } catch (err) {
      setPoolError(err.message)
    }
  }, [])

  useEffect(() => {
    fetchStats()
    fetchMetricsHistory()
    fetchPoolStats()
    const statsInterval = setInterval(fetchStats, 5000) // Refresh every 5 seconds
    const historyInterval = setInterval(fetchMetricsHistory, 5000) // Refresh history every 5 seconds
    const poolInterval = setInterval(fetchPoolStats, 5000)
    return () => {
      clearInterval(statsInterval)
      clearInterval(historyInterval)
      clearInterval(poolInterval)
    }
  }, [fetchStats, fetchMetricsHistory, fetchPoolStats])

  useEffect(() => {
    fetchHealth()
    const interval = setInterval(fetchHealth, 15000)
    return () => clearInterval(interval)
  }, [fetchHealth])

  // Save timeRange to localStorage when it changes
  useEffect(() => {
    try {
      localStorage.setItem('rustsocks_dashboard_timerange', timeRange.toString())
    } catch (e) {
      console.warn('Failed to save timeRange to localStorage:', e)
    }
  }, [timeRange])

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

        <div className="stat-card">
          <h4>Connection Pool</h4>
          {poolStats ? (
            <>
              <div className="value">{poolStats.total_idle} idle</div>
              <div className="change">
                {poolStats.active_in_use} in use · hit {(poolStats.hit_rate * 100).toFixed(1)}%
              </div>
            </>
          ) : (
            <>
              <div className="value">-</div>
              <div className="change">{poolError ? 'Unavailable' : 'Loading...'}</div>
            </>
          )}
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

      {poolError && !poolStats && (
        <div className="error">Pool telemetry unavailable: {poolError}</div>
      )}

      {poolStats && (
        <div className="card">
          <div className="card-header">
            <h3>Connection Pool Overview</h3>
            <span
              className={`badge ${poolStats.enabled ? 'badge-success' : 'badge-warning'}`}
              style={{ textTransform: 'uppercase' }}
            >
              {poolStats.enabled ? 'Enabled' : 'Disabled'}
            </span>
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))',
              gap: '16px',
              padding: '16px 16px 0'
            }}
          >
            <div>
              <div className="detail-label">Idle / Active</div>
              <div className="detail-value">
                {poolStats.total_idle} / {poolStats.active_in_use}
              </div>
            </div>
            <div>
              <div className="detail-label">Hit Ratio</div>
              <div className="detail-value">
                {(poolStats.hit_rate * 100).toFixed(1)}% ({poolStats.pool_hits} hits / {poolStats.pool_misses} misses)
              </div>
            </div>
            <div>
              <div className="detail-label">Drops / Evicted</div>
              <div className="detail-value">
                {poolStats.dropped_full} drops / {poolStats.evicted} evicted
              </div>
            </div>
            <div>
              <div className="detail-label">Pending / Max Idle</div>
              <div className="detail-value">
                {poolStats.pending_creates} pending · cap {poolStats.config.max_total_idle}
              </div>
            </div>
          </div>
          {topDestinations.length > 0 ? (
            <div className="table-container" style={{ marginTop: '16px' }}>
              <table>
                <thead>
                  <tr>
                    <th>Destination</th>
                    <th>Idle</th>
                    <th>In Use</th>
                    <th>Hits</th>
                    <th>Misses</th>
                    <th>Drops</th>
                    <th>Evicted</th>
                    <th>Last Activity</th>
                  </tr>
                </thead>
                <tbody>
                  {topDestinations.map((dest, idx) => {
                    const lastEvent = dest.last_activity || dest.last_miss
                    return (
                      <tr key={`${dest.destination}-${idx}`}>
                        <td>{dest.destination}</td>
                        <td>{dest.idle_connections}</td>
                        <td>{dest.in_use}</td>
                        <td>{dest.pool_hits}</td>
                        <td>{dest.pool_misses}</td>
                        <td>{dest.drops}</td>
                        <td>{dest.evicted}</td>
                        <td>{formatPoolTimestamp(lastEvent)}</td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <div style={{ padding: '16px', color: 'var(--text-secondary)' }}>
              No pooled destinations yet
            </div>
          )}
        </div>
      )}

      {chartHistory.length > 0 && (
        <>
          <div className="card">
            <div className="card-header">
              <h3>Zakres czasowy wykresów</h3>
              <div style={{ display: 'flex', gap: '8px' }}>
                {[
                  { label: '5 min', value: 5 },
                  { label: '15 min', value: 15 },
                  { label: '30 min', value: 30 },
                  { label: '1h', value: 60 },
                  { label: '2h', value: 120 }
                ].map(option => (
                  <button
                    key={option.value}
                    type="button"
                    className={`btn ${timeRange === option.value ? 'btn-primary' : ''}`}
                    onClick={() => setTimeRange(option.value)}
                    style={{ padding: '6px 12px', fontSize: '14px' }}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="chart-grid">
            <div className="card">
              <div className="card-header">
                <h3>Sesje w czasie</h3>
              </div>
            <div className="chart-wrapper">
              <ResponsiveContainer width="100%" height={260}>
                <LineChart data={chartHistory}>
                  <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" strokeDasharray="3 3" />
                  <XAxis
                    dataKey="timestamp"
                    domain={[timeWindow.start, timeWindow.end]}
                    scale="time"
                    type="number"
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
                  />
                  <Legend />
                  <Line
                    type="monotone"
                    dataKey="active"
                    stroke="#38bdf8"
                    strokeWidth={2}
                    dot={chartHistory.length <= 2}
                    name="Aktywne"
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
                    domain={[timeWindow.start, timeWindow.end]}
                    scale="time"
                    type="number"
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
        </>
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
