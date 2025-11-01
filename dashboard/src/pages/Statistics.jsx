import React, { useState, useEffect, useMemo } from 'react'
import {
  ResponsiveContainer,
  PieChart,
  Pie,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  Cell
} from 'recharts'
import { getApiUrl } from '../lib/basePath'
import { formatBytes } from '../lib/format'

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

  // Przygotuj dane dla wykresu statusów
  const statusChartData = useMemo(() => {
    if (!stats) return []
    return [
      { name: 'Active', value: stats.active_sessions, color: '#38bdf8' },
      { name: 'Closed', value: stats.closed_sessions, color: '#fbbf24' },
      { name: 'Failed', value: stats.failed_sessions, color: '#ef4444' }
    ].filter(item => item.value > 0)
  }, [stats])

  // Przygotuj dane dla wykresu top users by bandwidth
  const userBandwidthData = useMemo(() => {
    if (!stats || !stats.top_users.length) return []
    return stats.top_users.slice(0, 10).map(user => ({
      name: user.user,
      sent: user.bytes_sent,
      received: user.bytes_received,
      total: user.bytes_sent + user.bytes_received
    }))
  }, [stats])

  // Przygotuj dane dla wykresu top destinations
  const destData = useMemo(() => {
    if (!stats || !stats.top_destinations.length) return []
    return stats.top_destinations.slice(0, 10).map(dest => ({
      name: dest.destination.length > 20 ? dest.destination.substring(0, 17) + '...' : dest.destination,
      connections: dest.session_count,
      fullName: dest.destination
    }))
  }, [stats])

  // Oblicz dodatkowe metryki
  const metrics = useMemo(() => {
    if (!stats) return {}
    const total = stats.total_sessions
    const successRate = total > 0 ? ((stats.closed_sessions / total) * 100).toFixed(1) : 0
    const failRate = total > 0 ? ((stats.failed_sessions / total) * 100).toFixed(1) : 0
    const totalBandwidth = stats.total_bytes_sent + stats.total_bytes_received
    const avgDuration = stats.total_sessions > 0 ? Math.round(totalBandwidth / stats.total_sessions) : 0
    return { successRate, failRate, totalBandwidth, avgDuration }
  }, [stats])

  if (loading) return <div className="loading">Loading statistics...</div>
  if (error) return <div className="error">Error: {error}</div>

  return (
    <div>
      <div className="page-header">
        <h2>Statistics</h2>
        <p>Detailed analytics and comprehensive metrics</p>
      </div>

      <div className="stats-grid">
        <div className="stat-card">
          <h4>Total Sessions</h4>
          <div className="value">{stats.total_sessions}</div>
          <div className="change">All time</div>
        </div>

        <div className="stat-card">
          <h4>Success Rate</h4>
          <div className="value">{metrics.successRate}%</div>
          <div className="change">Sessions completed</div>
        </div>

        <div className="stat-card">
          <h4>Fail Rate</h4>
          <div className="value">{metrics.failRate}%</div>
          <div className="change">Failed connections</div>
        </div>

        <div className="stat-card">
          <h4>Total Bandwidth</h4>
          <div className="value">{formatBytes(metrics.totalBandwidth)}</div>
          <div className="change">Transferred</div>
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '24px', marginTop: '24px' }}>
        <div className="card">
          <div className="card-header">
            <h3>Session Status Distribution</h3>
          </div>
          {statusChartData.length > 0 ? (
            <div className="chart-wrapper">
              <ResponsiveContainer width="100%" height={260}>
                <PieChart>
                  <Pie
                    data={statusChartData}
                    cx="50%"
                    cy="50%"
                    labelLine={false}
                    label={({ name, value }) => `${name}: ${value}`}
                    outerRadius={80}
                    fill="#8884d8"
                    dataKey="value"
                  >
                    {statusChartData.map((entry, index) => (
                      <Cell key={`cell-${index}`} fill={entry.color} />
                    ))}
                  </Pie>
                  <Tooltip />
                </PieChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '40px' }}>
              No session data available
            </div>
          )}
        </div>

        <div className="card">
          <div className="card-header">
            <h3>Active vs Completed Sessions</h3>
          </div>
          <div className="chart-wrapper">
            <ResponsiveContainer width="100%" height={260}>
              <BarChart
                data={[
                  { name: 'Sessions', active: stats.active_sessions, completed: stats.closed_sessions }
                ]}
              >
                <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" />
                <XAxis dataKey="name" stroke="var(--text-secondary)" />
                <YAxis stroke="var(--text-secondary)" />
                <Tooltip />
                <Legend />
                <Bar dataKey="active" fill="#38bdf8" name="Active" />
                <Bar dataKey="completed" fill="#10b981" name="Completed" />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      </div>

      <div className="card" style={{ marginTop: '24px' }}>
        <div className="card-header">
          <h3>Bandwidth by User (Top 10)</h3>
        </div>
        {userBandwidthData.length > 0 ? (
          <div className="chart-wrapper">
            <ResponsiveContainer width="100%" height={300}>
              <BarChart
                data={userBandwidthData}
                margin={{ bottom: 60 }}
              >
                <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" />
                <XAxis
                  dataKey="name"
                  stroke="var(--text-secondary)"
                  angle={-45}
                  textAnchor="end"
                  height={80}
                  tick={{ fontSize: 12 }}
                />
                <YAxis
                  stroke="var(--text-secondary)"
                  tickFormatter={(value) => `${(value / (1024 * 1024)).toFixed(0)}MB`}
                />
                <Tooltip
                  formatter={(value) => formatBytes(value)}
                  labelStyle={{ color: 'var(--text-primary)' }}
                />
                <Legend />
                <Bar dataKey="sent" fill="#38bdf8" name="Sent" />
                <Bar dataKey="received" fill="#c084fc" name="Received" />
              </BarChart>
            </ResponsiveContainer>
          </div>
        ) : (
          <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '40px' }}>
            No user data available
          </div>
        )}
      </div>

      <div className="card" style={{ marginTop: '24px' }}>
        <div className="card-header">
          <h3>Connections by Destination (Top 10)</h3>
        </div>
        {destData.length > 0 ? (
          <div className="chart-wrapper">
            <ResponsiveContainer width="100%" height={300}>
              <BarChart
                data={destData}
                margin={{ bottom: 60 }}
              >
                <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" />
                <XAxis
                  dataKey="name"
                  stroke="var(--text-secondary)"
                  angle={-45}
                  textAnchor="end"
                  height={80}
                  tick={{ fontSize: 12 }}
                />
                <YAxis stroke="var(--text-secondary)" />
                <Tooltip
                  labelStyle={{ color: 'var(--text-primary)' }}
                  cursor={{ fill: 'rgba(59, 130, 246, 0.1)' }}
                />
                <Bar dataKey="connections" fill="#22d3ee" name="Connections" />
              </BarChart>
            </ResponsiveContainer>
          </div>
        ) : (
          <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '40px' }}>
            No destination data available
          </div>
        )}
      </div>
    </div>
  )
}

export default Statistics
