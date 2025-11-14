import React, { useState, useEffect, useCallback, useMemo } from 'react'
import {
  RefreshCw,
  Filter,
  X,
  Download,
  Eye,
  ChevronLeft,
  ChevronRight,
  XCircle
} from 'lucide-react'
import { useSearchParams } from 'react-router-dom'
import { getApiUrl } from '../lib/basePath'
import { formatBytes, formatDateTime, formatDuration } from '../lib/format'
import {
  buildHistoryUrl,
  DEFAULT_PAGE_SIZE,
  sessionsToCsv
} from '../lib/sessions'
import SessionDetailDrawer from '../components/SessionDetailDrawer'

const statusOptions = [
  { value: '', label: 'Any Status' },
  { value: 'active', label: 'Active' },
  { value: 'closed', label: 'Closed' },
  { value: 'failed', label: 'Failed' },
  { value: 'rejected_by_acl', label: 'Rejected by ACL' }
]

const getInitialFilters = (params) => ({
  user: params.get('user') || '',
  destination: params.get('dest_ip') || params.get('destination') || '',
  status: params.get('status') || '',
  hours: params.get('hours') || '',
  page: Number(params.get('page')) || 1,
  pageSize: Number(params.get('page_size')) || DEFAULT_PAGE_SIZE,
  sortBy: params.get('sort_by') || 'start_time',
  sortDir: params.get('sort_dir') || 'desc'
})

function Sessions() {
  const [searchParams, setSearchParams] = useSearchParams()
  const initialFilters = useMemo(() => getInitialFilters(searchParams), [searchParams])
  const [showActive, setShowActive] = useState(searchParams.get('view') !== 'history')
  const [sessions, setSessions] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [filters, setFilters] = useState(initialFilters)
  const [filterDraft, setFilterDraft] = useState(initialFilters)
  const [pageInfo, setPageInfo] = useState(null)
  const [detailOpen, setDetailOpen] = useState(false)
  const [detailSession, setDetailSession] = useState(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailError, setDetailError] = useState(null)
  const [activeCount, setActiveCount] = useState(0)

  const syncSearchParams = useCallback(
    (nextShowActive, nextFilters) => {
      const params = new URLSearchParams()
      if (!nextShowActive) {
        params.set('view', 'history')
        if (nextFilters.user) params.set('user', nextFilters.user)
        if (nextFilters.destination) params.set('dest_ip', nextFilters.destination)
        if (nextFilters.status) params.set('status', nextFilters.status)
        if (nextFilters.hours) params.set('hours', nextFilters.hours)
        params.set('page', String(nextFilters.page))
        params.set('page_size', String(nextFilters.pageSize))
        if (nextFilters.sortBy) params.set('sort_by', nextFilters.sortBy)
        if (nextFilters.sortDir) params.set('sort_dir', nextFilters.sortDir)
      }
      setSearchParams(params)
    },
    [setSearchParams]
  )

  const fetchSessions = useCallback(
    async ({ forceSpinner = false } = {}) => {
      if (forceSpinner) {
        setLoading(true)
      }

      try {
        if (showActive) {
          const response = await fetch(getApiUrl('/api/sessions/active'))
          if (!response.ok) throw new Error('Failed to fetch active sessions')
          const data = await response.json()
          const activeSessions = Array.isArray(data) ? data : data.sessions || []
          setSessions(activeSessions)
          setPageInfo(null)
          const activeSessionsCount = activeSessions.filter(
            (session) => session.status?.toLowerCase() === 'active'
          ).length
          setActiveCount(activeSessionsCount)
        } else {
          const url = buildHistoryUrl(getApiUrl('/api/sessions/history'), filters)
          const response = await fetch(url)
          if (!response.ok) throw new Error('Failed to fetch session history')
          const data = await response.json()

          const historySessions = Array.isArray(data.data) ? data.data : data.sessions || []
          setSessions(historySessions)
          setPageInfo({
            page: data.page || filters.page,
            totalPages: data.total_pages || 1,
            total: data.total || historySessions.length
          })

          try {
            const activeResponse = await fetch(getApiUrl('/api/sessions/active'))
            if (activeResponse.ok) {
              const activeData = await activeResponse.json()
              const activeSessions = Array.isArray(activeData) ? activeData : activeData.sessions || []
              const activeSessionsCount = activeSessions.filter(
                (session) => session.status?.toLowerCase() === 'active'
              ).length
              setActiveCount(activeSessionsCount)
            }
          } catch (activeError) {
            console.warn('Failed to refresh active sessions count', activeError)
          }
        }
        setError(null)
      } catch (err) {
        setError(err.message)
      } finally {
        setLoading(false)
      }
    },
    [showActive, filters]
  )

  useEffect(() => {
    fetchSessions({ forceSpinner: true })
  }, [showActive, filters, fetchSessions])

  useEffect(() => {
    if (!showActive) {
      return undefined
    }

    const interval = setInterval(() => {
      fetchSessions()
    }, 3000)

    return () => clearInterval(interval)
  }, [showActive, fetchSessions])

  const handleToggleView = (nextShowActive) => {
    if (nextShowActive === showActive) return

    setShowActive(nextShowActive)
    if (nextShowActive) {
      syncSearchParams(true, filters)
    } else {
      const nextFilters = { ...filters, page: 1 }
      setFilters(nextFilters)
      setFilterDraft(nextFilters)
      syncSearchParams(false, nextFilters)
    }
  }

  const handleDraftChange = (key, value) => {
    setFilterDraft((prev) => ({
      ...prev,
      [key]: value
    }))
  }

  const handleApplyFilters = (event) => {
    event.preventDefault()
    const nextFilters = {
      ...filterDraft,
      page: 1,
      pageSize: Number(filterDraft.pageSize) || DEFAULT_PAGE_SIZE
    }
    setFilters(nextFilters)
    syncSearchParams(false, nextFilters)
  }

  const handleClearFilters = () => {
    const cleared = {
      user: '',
      destination: '',
      status: '',
      hours: '',
      page: 1,
      pageSize: DEFAULT_PAGE_SIZE
    }
    setFilterDraft(cleared)
    setFilters(cleared)
    syncSearchParams(false, cleared)
  }

  const handlePageChange = (direction) => {
    if (!pageInfo) return
    const nextPage = direction === 'prev' ? Math.max(pageInfo.page - 1, 1) : Math.min(pageInfo.page + 1, pageInfo.totalPages)
    if (nextPage === pageInfo.page) return

    const nextFilters = { ...filters, page: nextPage }
    setFilters(nextFilters)
    syncSearchParams(false, nextFilters)
  }

  const handlePageSizeChange = (event) => {
    const value = Number(event.target.value) || DEFAULT_PAGE_SIZE
    const nextFilters = { ...filters, pageSize: value, page: 1 }
    setFilters(nextFilters)
    setFilterDraft((prev) => ({ ...prev, pageSize: value }))
    syncSearchParams(false, nextFilters)
  }

  const handleExport = () => {
    if (!sessions.length) return

    const csv = sessionsToCsv(sessions)
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' })
    const url = window.URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `rustsocks-sessions-${showActive ? 'active' : 'history'}-${new Date().toISOString()}.csv`
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    window.URL.revokeObjectURL(url)
  }

  const handleViewDetails = async (session) => {
    setDetailOpen(true)
    setDetailSession(session)
    setDetailLoading(true)
    setDetailError(null)

    try {
      const response = await fetch(getApiUrl(`/api/sessions/${session.id}`))
      if (!response.ok) throw new Error('Failed to fetch session details')
      const data = await response.json()
      setDetailSession(data)
    } catch (err) {
      setDetailError(err.message)
    } finally {
      setDetailLoading(false)
    }
  }

  const handleTerminate = async (session) => {
    if (!window.confirm(`Are you sure you want to terminate the session for ${session.user}?`)) {
      return
    }

    try {
      const response = await fetch(getApiUrl(`/api/sessions/${session.id}/terminate`), {
        method: 'POST'
      })

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}))
        throw new Error(errorData.error || 'Failed to terminate session')
      }

      // Refresh sessions list
      await fetchSessions()
    } catch (err) {
      alert(`Error terminating session: ${err.message}`)
    }
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

  const aclBadgeClass = (decision) => {
    if (!decision) return 'badge badge-warning'
    return decision.toLowerCase() === 'allow' ? 'badge badge-success' : 'badge badge-danger'
  }

  const handleSort = (field) => {
    if (!showActive) {
      // Update filters to trigger server-side sorting
      const newDir = filters.sortBy === field && filters.sortDir === 'asc' ? 'desc' : 'asc'
      const nextFilters = {
        ...filters,
        sortBy: field,
        sortDir: newDir,
        page: 1 // Reset to first page when sorting changes
      }
      setFilters(nextFilters)
      setFilterDraft(nextFilters)
      syncSearchParams(false, nextFilters)
    }
  }

  const getSortIndicator = (field) => {
    if (!showActive) {
      // For history view, show server-side sort state
      if (filters.sortBy !== field) return ' ↕'
      return filters.sortDir === 'asc' ? ' ↑' : ' ↓'
    }
    // For active sessions, no sorting
    return ''
  }

  if (loading) {
    return <div className="loading">Loading sessions...</div>
  }

  return (
    <div>
      <div className="page-header">
        <h2>Sessions</h2>
        <p>Monitor active connections and browse the history with flexible filters.</p>
      </div>

      <div className="toolbar">
        <div className="toolbar-left">
          <button
            className={`btn ${showActive ? 'btn-primary' : ''}`}
            onClick={() => handleToggleView(true)}
          >
            Active Sessions ({activeCount})
          </button>
          <button
            className={`btn ${!showActive ? 'btn-primary' : ''}`}
            onClick={() => handleToggleView(false)}
          >
            Session History
          </button>
        </div>
        <div className="toolbar-right">
          <button
            className="btn"
            onClick={() => fetchSessions({ forceSpinner: true })}
            title="Refresh data"
          >
            <RefreshCw size={16} />
          </button>
          <button
            className="btn"
            onClick={handleExport}
            disabled={!sessions.length}
            title="Export visible rows to CSV"
          >
            <Download size={16} />
          </button>
        </div>
      </div>

      {!showActive && (
        <div className="card">
          <div className="card-header">
            <h3>History filters</h3>
          </div>
          <form onSubmit={handleApplyFilters} className="filters-form">
            <div className="filters-grid">
              <div className="form-group">
                <label>User</label>
                <input
                  type="text"
                  value={filterDraft.user}
                  onChange={(e) => handleDraftChange('user', e.target.value)}
                  placeholder="e.g. admin"
                />
              </div>
              <div className="form-group">
                <label>Destination address</label>
                <input
                  type="text"
                  value={filterDraft.destination}
                  onChange={(e) => handleDraftChange('destination', e.target.value)}
                  placeholder="e.g. 192.168.0.10"
                />
              </div>
              <div className="form-group">
                <label>Status</label>
                <select
                  value={filterDraft.status}
                  onChange={(e) => handleDraftChange('status', e.target.value)}
                >
                  {statusOptions.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="form-group">
                <label>Recent hours</label>
                <input
                  type="number"
                  min="1"
                  value={filterDraft.hours}
                  onChange={(e) => handleDraftChange('hours', e.target.value)}
                  placeholder="e.g. 24"
                />
              </div>
            </div>
            <div className="filter-actions">
              <button type="submit" className="btn btn-primary">
                <Filter size={16} style={{ marginRight: '8px' }} />
                Apply filters
              </button>
              <button type="button" className="btn" onClick={handleClearFilters}>
                <X size={16} style={{ marginRight: '8px' }} />
                Clear
              </button>
            </div>
          </form>
        </div>
      )}

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
        <div className="table-meta">
          {!showActive && pageInfo && (
            <div>
              Showing {sessions.length} of {pageInfo.total} records
            </div>
          )}
          {!showActive && (
            <div className="pagination">
              <button
                type="button"
                className="icon-button"
                disabled={!pageInfo || pageInfo.page <= 1}
                onClick={() => handlePageChange('prev')}
                title="Previous page"
              >
                <ChevronLeft size={16} />
              </button>
              <span>
                Page {pageInfo ? pageInfo.page : 1} of {pageInfo ? pageInfo.totalPages : 1}
              </span>
              <button
                type="button"
                className="icon-button"
                disabled={!pageInfo || pageInfo.page >= pageInfo.totalPages}
                onClick={() => handlePageChange('next')}
                title="Next page"
              >
                <ChevronRight size={16} />
              </button>
              <select
                value={filters.pageSize}
                onChange={handlePageSizeChange}
                className="page-size-select"
              >
                {[25, 50, 100, 250].map((size) => (
                    <option key={size} value={size}>
                      {size} / page
                    </option>
                ))}
              </select>
            </div>
          )}
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('user')}>
                  User{getSortIndicator('user')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('source_ip')}>
                  Source{getSortIndicator('source_ip')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('dest_ip')}>
                  Destination{getSortIndicator('dest_ip')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('protocol')}>
                  Protocol{getSortIndicator('protocol')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('status')}>
                  Status{getSortIndicator('status')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('acl_decision')}>
                  ACL{getSortIndicator('acl_decision')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('bytes_sent')}>
                  Sent{getSortIndicator('bytes_sent')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('bytes_received')}>
                  Received{getSortIndicator('bytes_received')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('duration_seconds')}>
                  Duration{getSortIndicator('duration_seconds')}
                </th>
                <th style={{ cursor: showActive ? 'default' : 'pointer' }} onClick={() => handleSort('start_time')}>
                  Start{getSortIndicator('start_time')}
                </th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session) => (
                <tr key={session.id}>
                  <td><strong>{session.user}</strong></td>
                  <td><code>{session.source_ip}:{session.source_port}</code></td>
                  <td><code>{session.dest_ip}:{session.dest_port}</code></td>
                  <td>{session.protocol.toUpperCase()}</td>
                  <td>
                    <span className={getStatusBadge(session.status)}>
                      {session.status}
                    </span>
                  </td>
                  <td>
                    <span className={aclBadgeClass(session.acl_decision)}>
                      {session.acl_decision || 'N/A'}
                    </span>
                    {session.acl_rule && (
                      <div className="subtle-text">
                        {session.acl_rule}
                      </div>
                    )}
                  </td>
                  <td>{formatBytes(session.bytes_sent)}</td>
                  <td>{formatBytes(session.bytes_received)}</td>
                  <td>{formatDuration(session.duration_seconds)}</td>
                  <td>{formatDateTime(session.start_time)}</td>
                  <td>
                    <button
                      type="button"
                      className="icon-button"
                      title="Session details"
                      onClick={() => handleViewDetails(session)}
                    >
                      <Eye size={16} />
                    </button>
                    {session.status?.toLowerCase() === 'active' && (
                      <button
                        type="button"
                        className="icon-button"
                        title="Terminate session"
                        onClick={() => handleTerminate(session)}
                        style={{ marginLeft: '8px', color: 'var(--danger)' }}
                      >
                        <XCircle size={16} />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {sessions.length === 0 && (
                <tr>
                  <td colSpan="11" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    {showActive ? 'No active sessions' : 'No results for selected filters'}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      <SessionDetailDrawer
        open={detailOpen}
        session={detailSession}
        loading={detailLoading}
        error={detailError}
        onClose={() => setDetailOpen(false)}
      />
    </div>
  )
}

export default Sessions
