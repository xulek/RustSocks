import React, { useState, useEffect, useMemo, useCallback } from 'react'
import { X } from 'lucide-react'
import {
  ResponsiveContainer,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend
} from 'recharts'
import { getApiUrl } from '../lib/basePath'
import { formatBytes, formatDateTime, formatDuration } from '../lib/format'

const DetailRow = ({ label, value }) => (
  <div className="detail-row">
    <div className="detail-label">{label}</div>
    <div className="detail-value">{value ?? '—'}</div>
  </div>
)

function UserDetailModal({ open, user, onClose }) {
  const [sessions, setSessions] = useState([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(null)

  useEffect(() => {
    if (open && user) {
      fetchUserSessions()
    }
  }, [open, user])

  useEffect(() => {
    if (!open) return

    const handleEscape = (e) => {
      if (e.key === 'Escape') {
        onClose()
      }
    }

    window.addEventListener('keydown', handleEscape)
    return () => window.removeEventListener('keydown', handleEscape)
  }, [open, onClose])

  const fetchUserSessions = async () => {
    setLoading(true)
    setError(null)
    try {
      const response = await fetch(getApiUrl(`/api/users/${user.username}/sessions`))
      if (!response.ok) throw new Error('Failed to fetch user sessions')
      const data = await response.json()
      setSessions(Array.isArray(data) ? data : data.sessions || [])
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  // Oblicz statystyki
  const stats = useMemo(() => {
    if (!sessions.length) {
      return {
        totalSessions: 0,
        totalBandwidth: 0,
        activeSessions: 0,
        closedSessions: 0,
        failedSessions: 0,
        totalBytesIn: 0,
        totalBytesOut: 0
      }
    }

    return {
      totalSessions: sessions.length,
      totalBandwidth: sessions.reduce((sum, s) => sum + (s.bytes_sent || 0) + (s.bytes_received || 0), 0),
      activeSessions: sessions.filter(s => s.status === 'active').length,
      closedSessions: sessions.filter(s => s.status === 'closed').length,
      failedSessions: sessions.filter(s => s.status === 'failed').length,
      totalBytesIn: sessions.reduce((sum, s) => sum + (s.bytes_received || 0), 0),
      totalBytesOut: sessions.reduce((sum, s) => sum + (s.bytes_sent || 0), 0)
    }
  }, [sessions])

  // Przygotuj dane dla wykresu aktywności
  const activityData = useMemo(() => {
    if (!sessions.length) return []

    // Grupuj sesje po godzinach
    const hourlyData = {}
    sessions.forEach(session => {
      if (!session.start_time) return
      const date = new Date(session.start_time)
      const hour = date.toISOString().slice(0, 13) + ':00:00'
      if (!hourlyData[hour]) {
        hourlyData[hour] = { timestamp: hour, sessions: 0, bandwidth: 0 }
      }
      hourlyData[hour].sessions += 1
      hourlyData[hour].bandwidth += (session.bytes_sent || 0) + (session.bytes_received || 0)
    })

    return Object.values(hourlyData)
      .sort((a, b) => new Date(a.timestamp) - new Date(b.timestamp))
      .slice(-24) // Ostatnie 24 godziny
  }, [sessions])

  // Ostatnie 5 sesji
  const recentSessions = useMemo(() => {
    return [...sessions]
      .sort((a, b) => new Date(b.start_time || 0) - new Date(a.start_time || 0))
      .slice(0, 5)
  }, [sessions])

  const handleOverlayClick = useCallback((e) => {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }, [onClose])

  if (!open) return null

  return (
    <div className="drawer-overlay" onClick={handleOverlayClick}>
      <div className="drawer">
        <div className="drawer-header">
          <h3>Szczegóły użytkownika: {user?.username}</h3>
          <button type="button" className="icon-button" onClick={onClose} title="Zamknij panel">
            <X size={18} />
          </button>
        </div>

        {loading && <div className="loading">Ładowanie danych użytkownika...</div>}
        {error && <div className="error">Nie udało się pobrać danych: {error}</div>}

        {!loading && !error && user && (
          <div className="drawer-content">
            {/* User Info */}
            <div style={{ marginBottom: '24px' }}>
              <h4 style={{ marginBottom: '12px' }}>Informacje</h4>
              <div className="detail-grid">
                <DetailRow label="Użytkownik" value={user.username} />
                <DetailRow
                  label="Grupy"
                  value={
                    user.groups && user.groups.length > 0
                      ? user.groups.join(', ')
                      : 'Brak grup'
                  }
                />
                <DetailRow label="Reguły ACL" value={user.rule_count || 0} />
              </div>
            </div>

            {/* Statistics */}
            {sessions.length > 0 && (
              <div style={{ marginBottom: '24px' }}>
                <h4 style={{ marginBottom: '12px' }}>Statystyki</h4>
                <div style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))',
                  gap: '12px',
                  marginBottom: '12px'
                }}>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Sesji ogółem</div>
                    <div style={{ fontSize: '18px', fontWeight: 'bold' }}>{stats.totalSessions}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Przepustowość</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBandwidth)}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Wysłano</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBytesOut)}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Odebrano</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBytesIn)}</div>
                  </div>
                </div>
              </div>
            )}

            {/* Activity Chart */}
            {activityData.length > 0 && (
              <div style={{ marginBottom: '24px' }}>
                <h4 style={{ marginBottom: '12px' }}>Aktywność w czasie</h4>
                <div className="chart-wrapper">
                  <ResponsiveContainer width="100%" height={240}>
                    <BarChart data={activityData}>
                      <CartesianGrid stroke="rgba(148, 163, 184, 0.2)" />
                      <XAxis
                        dataKey="timestamp"
                        stroke="var(--text-secondary)"
                        tickFormatter={(value) => new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                        tick={{ fontSize: 12 }}
                      />
                      <YAxis
                        yAxisId="left"
                        stroke="var(--text-secondary)"
                        tick={{ fontSize: 12 }}
                      />
                      <YAxis
                        yAxisId="right"
                        orientation="right"
                        stroke="var(--text-secondary)"
                        tick={{ fontSize: 12 }}
                        tickFormatter={(value) => `${(value / (1024 * 1024)).toFixed(0)}MB`}
                      />
                      <Tooltip
                        labelFormatter={(value) => new Date(value).toLocaleTimeString()}
                        formatter={(value, name) => {
                          if (name === 'bandwidth') return [formatBytes(value), 'Przepustowość']
                          return [value, 'Sesje']
                        }}
                      />
                      <Legend />
                      <Bar yAxisId="left" dataKey="sessions" fill="#38bdf8" name="Sesje" />
                      <Bar yAxisId="right" dataKey="bandwidth" fill="#22d3ee" name="Przepustowość" />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>
            )}

            {/* Recent Sessions */}
            {recentSessions.length > 0 && (
              <div style={{ marginBottom: '24px' }}>
                <h4 style={{ marginBottom: '12px' }}>Ostatnie sesje (Top 5)</h4>
                <div className="table-container" style={{ maxHeight: '300px', overflowY: 'auto' }}>
                  <table style={{ fontSize: '12px' }}>
                    <thead>
                      <tr>
                        <th>Cel</th>
                        <th>Status</th>
                        <th>Wysłano</th>
                        <th>Odebrano</th>
                        <th>Start</th>
                      </tr>
                    </thead>
                    <tbody>
                      {recentSessions.map((session, idx) => (
                        <tr key={idx}>
                          <td><code style={{ fontSize: '11px' }}>{session.dest_ip}:{session.dest_port}</code></td>
                          <td>
                            <span className={`badge ${
                              session.status === 'active' ? 'badge-success' :
                              session.status === 'closed' ? 'badge-warning' :
                              'badge-danger'
                            }`} style={{ fontSize: '11px' }}>
                              {session.status}
                            </span>
                          </td>
                          <td>{formatBytes(session.bytes_sent)}</td>
                          <td>{formatBytes(session.bytes_received)}</td>
                          <td style={{ fontSize: '11px' }}>{formatDateTime(session.start_time)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {sessions.length === 0 && !loading && (
              <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '40px 20px' }}>
                Brak sesji dla tego użytkownika
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

export default UserDetailModal
