import React, { useState, useEffect, useMemo, useCallback } from 'react'
import { X, Plus, Edit2, Trash2 } from 'lucide-react'
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

function UserDetailModal({ open, user, onClose, onEditRule, onDeleteRule }) {
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

  // Compute statistics
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

  // Prepare data for the activity chart
  const activityData = useMemo(() => {
    if (!sessions.length) return []

    // Group sessions by hour
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
      .slice(-24) // Last 24 hours
  }, [sessions])

  // Last 5 sessions
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

  const ruleCount = user ? (user.rules?.length ?? user.rule_count ?? 0) : 0

  if (!open) return null

  return (
    <div className="modal-overlay user-detail-overlay" onClick={handleOverlayClick}>
      <div className="modal user-detail-modal">
        <div className="modal-header user-detail-header">
          <div>
            <p className="subtle-text" style={{ marginBottom: '4px' }}>User ACL Details</p>
            <h3 style={{ margin: 0 }}>User Details: {user?.username}</h3>
          </div>
          <button type="button" className="icon-button" onClick={onClose} title="Close panel">
            <X size={18} />
          </button>
        </div>

        {loading && <div className="loading" style={{ padding: '24px' }}>Loading user data...</div>}
        {error && <div className="error" style={{ padding: '24px' }}>Failed to load user data: {error}</div>}

        {!loading && !error && user && (
          <div className="modal-content user-detail-content">
            <div className="user-detail-top">
              <div className="user-detail-info-card">
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <h4 style={{ margin: 0 }}>Information</h4>
                  <span className="badge badge-success" style={{ fontSize: '11px', padding: '4px 10px' }}>
                    {ruleCount} rules
                  </span>
                </div>
                <div className="detail-grid" style={{ marginTop: '16px' }}>
                  <DetailRow label="User" value={user.username} />
                  <DetailRow
                    label="Groups"
                    value={user.groups && user.groups.length > 0 ? user.groups.join(', ') : 'No groups'}
                  />
                  <DetailRow label="ACL Rules" value={ruleCount} />
                </div>
                <div className="user-detail-groups">
                  {user.groups && user.groups.length > 0 ? (
                    user.groups.map((group, idx) => (
                      <span key={`group-${idx}`} className="badge badge-warning">
                        {group}
                      </span>
                    ))
                  ) : (
                    <span className="subtle-text">User is not part of any group</span>
                  )}
                </div>
              </div>
            </div>

            <div className="user-detail-section user-detail-rules-section">
              <div className="user-detail-section-header">
                <h4 style={{ margin: 0 }}>ACL Rules</h4>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => onEditRule?.(user)}
                  style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '12px', padding: '8px 12px' }}
                >
                  <Plus size={14} />
                  Add rule
                </button>
              </div>
              {user.rules && user.rules.length > 0 ? (
                <div className="table-container user-detail-rules-table">
                  <table>
                    <thead>
                      <tr>
                        <th>Action</th>
                        <th>Description</th>
                        <th>Destinations</th>
                        <th>Ports</th>
                        <th>Protocol</th>
                        <th>Priority</th>
                        <th>Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {user.rules.map((rule, idx) => (
                        <tr key={`rule-${idx}`}>
                          <td>
                            <span className={`badge ${rule.action === 'allow' ? 'badge-success' : 'badge-danger'}`}>
                              {rule.action}
                            </span>
                          </td>
                      <td>{rule.description || 'No description'}</td>
                          <td><code>{rule.destinations.join(', ')}</code></td>
                          <td><code>{rule.ports.join(', ')}</code></td>
                          <td>{rule.protocols.join(', ')}</td>
                          <td>{rule.priority}</td>
                          <td>
                            <div style={{ display: 'flex', gap: '6px' }}>
                              <button
                                type="button"
                                className="icon-button"
                                onClick={() => onEditRule?.(user, idx)}
                                title="Edit rule"
                              >
                                <Edit2 size={14} />
                              </button>
                              <button
                                type="button"
                                className="icon-button"
                                onClick={() => onDeleteRule?.(user.username, rule)}
                                title="Delete rule"
                                style={{ color: 'var(--danger)' }}
                              >
                                <Trash2 size={14} />
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <p style={{ color: 'var(--text-secondary)', margin: 0 }}>No ACL rules for this user.</p>
              )}
            </div>

            {sessions.length > 0 && (
              <div className="user-detail-section user-detail-stats-section">
                <h4 style={{ marginBottom: '12px' }}>Statistics</h4>
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
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Total sessions</div>
                    <div style={{ fontSize: '18px', fontWeight: 'bold' }}>{stats.totalSessions}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Bandwidth</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBandwidth)}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Sent</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBytesOut)}</div>
                  </div>
                  <div style={{
                    backgroundColor: 'var(--bg-dark)',
                    padding: '12px',
                    borderRadius: '6px',
                    textAlign: 'center'
                  }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>Received</div>
                    <div style={{ fontSize: '14px', fontWeight: 'bold' }}>{formatBytes(stats.totalBytesIn)}</div>
                  </div>
                </div>
              </div>
            )}

            {activityData.length > 0 && (
              <div className="user-detail-section user-detail-activity-section">
                <h4 style={{ marginBottom: '12px' }}>Activity over time</h4>
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
                          if (name === 'Bandwidth') return [formatBytes(value), 'Bandwidth']
                          if (name === 'Sessions') return [value, 'Sessions']
                          return [value, name]
                        }}
                      />
                      <Legend
                        formatter={(value) => value}
                      />
                      <Bar yAxisId="left" dataKey="sessions" fill="#38bdf8" name="Sessions" />
                      <Bar yAxisId="right" dataKey="bandwidth" fill="#22d3ee" name="Bandwidth" />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>
            )}

            {recentSessions.length > 0 && (
              <div className="user-detail-section user-detail-recent-section">
                <h4 style={{ marginBottom: '12px' }}>Recent sessions (Top 5)</h4>
                <div className="table-container" style={{ maxHeight: '300px', overflowY: 'auto' }}>
                  <table style={{ fontSize: '12px' }}>
                    <thead>
                      <tr>
                        <th>Destination</th>
                        <th>Status</th>
                        <th>Sent</th>
                        <th>Received</th>
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

            {sessions.length === 0 && (
              <div className="user-detail-section user-detail-empty">
              <p style={{ textAlign: 'center', color: 'var(--text-secondary)', margin: 0 }}>No sessions for this user</p>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

export default UserDetailModal
