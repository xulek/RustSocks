import React, { useState, useEffect, useCallback, useMemo } from 'react'
import {
  RefreshCw,
  Filter,
  X,
  Download,
  Eye,
  ChevronLeft,
  ChevronRight
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
  { value: '', label: 'Dowolny status' },
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
  pageSize: Number(params.get('page_size')) || DEFAULT_PAGE_SIZE
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
  const [sortBy, setSortBy] = useState('start_time')
  const [sortDir, setSortDir] = useState('desc')

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
    if (sortBy === field) {
      setSortDir(sortDir === 'asc' ? 'desc' : 'asc')
    } else {
      setSortBy(field)
      setSortDir('asc')
    }
  }

  const getSortedSessions = () => {
    const sorted = [...sessions].sort((a, b) => {
      let aVal = a[sortBy]
      let bVal = b[sortBy]

      // Handle null/undefined
      if (aVal === null || aVal === undefined) aVal = ''
      if (bVal === null || bVal === undefined) bVal = ''

      // Handle numeric values
      if (typeof aVal === 'number' && typeof bVal === 'number') {
        return sortDir === 'asc' ? aVal - bVal : bVal - aVal
      }

      // Handle string values
      const aStr = String(aVal).toLowerCase()
      const bStr = String(bVal).toLowerCase()
      return sortDir === 'asc' ? aStr.localeCompare(bStr) : bStr.localeCompare(aStr)
    })
    return sorted
  }

  const getSortIndicator = (field) => {
    if (sortBy !== field) return ' ↕'
    return sortDir === 'asc' ? ' ↑' : ' ↓'
  }

  if (loading) {
    return <div className="loading">Loading sessions...</div>
  }

  return (
    <div>
      <div className="page-header">
        <h2>Sessions</h2>
        <p>Monitoruj aktywne połączenia oraz historię z możliwością filtrowania.</p>
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
            title="Odśwież dane"
          >
            <RefreshCw size={16} />
          </button>
          <button
            className="btn"
            onClick={handleExport}
            disabled={!sessions.length}
            title="Eksportuj widoczne wiersze do CSV"
          >
            <Download size={16} />
          </button>
        </div>
      </div>

      {!showActive && (
        <div className="card">
          <div className="card-header">
            <h3>Filtry historii</h3>
          </div>
          <form onSubmit={handleApplyFilters} className="filters-form">
            <div className="filters-grid">
              <div className="form-group">
                <label>Użytkownik</label>
                <input
                  type="text"
                  value={filterDraft.user}
                  onChange={(e) => handleDraftChange('user', e.target.value)}
                  placeholder="np. admin"
                />
              </div>
              <div className="form-group">
                <label>Adres docelowy</label>
                <input
                  type="text"
                  value={filterDraft.destination}
                  onChange={(e) => handleDraftChange('destination', e.target.value)}
                  placeholder="np. 192.168.0.10"
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
                <label>Ostatnie godziny</label>
                <input
                  type="number"
                  min="1"
                  value={filterDraft.hours}
                  onChange={(e) => handleDraftChange('hours', e.target.value)}
                  placeholder="np. 24"
                />
              </div>
            </div>
            <div className="filter-actions">
              <button type="submit" className="btn btn-primary">
                <Filter size={16} style={{ marginRight: '8px' }} />
                Zastosuj filtry
              </button>
              <button type="button" className="btn" onClick={handleClearFilters}>
                <X size={16} style={{ marginRight: '8px' }} />
                Wyczyść
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
              Wyświetlono {sessions.length} z {pageInfo.total} rekordów
            </div>
          )}
          {!showActive && (
            <div className="pagination">
              <button
                type="button"
                className="icon-button"
                disabled={!pageInfo || pageInfo.page <= 1}
                onClick={() => handlePageChange('prev')}
                title="Poprzednia strona"
              >
                <ChevronLeft size={16} />
              </button>
              <span>
                Strona {pageInfo ? pageInfo.page : 1} z {pageInfo ? pageInfo.totalPages : 1}
              </span>
              <button
                type="button"
                className="icon-button"
                disabled={!pageInfo || pageInfo.page >= pageInfo.totalPages}
                onClick={() => handlePageChange('next')}
                title="Następna strona"
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
                    {size} / strona
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
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('user')}>
                  Użytkownik{getSortIndicator('user')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('source_ip')}>
                  Źródło{getSortIndicator('source_ip')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('dest_ip')}>
                  Cel{getSortIndicator('dest_ip')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('protocol')}>
                  Protokół{getSortIndicator('protocol')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('status')}>
                  Status{getSortIndicator('status')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('acl_decision')}>
                  ACL{getSortIndicator('acl_decision')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('bytes_sent')}>
                  Wysłano{getSortIndicator('bytes_sent')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('bytes_received')}>
                  Odebrano{getSortIndicator('bytes_received')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('duration_seconds')}>
                  Czas trwania{getSortIndicator('duration_seconds')}
                </th>
                <th style={{ cursor: 'pointer' }} onClick={() => handleSort('start_time')}>
                  Start{getSortIndicator('start_time')}
                </th>
                <th>Akcje</th>
              </tr>
            </thead>
            <tbody>
              {getSortedSessions().map((session) => (
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
                      title="Szczegóły sesji"
                      onClick={() => handleViewDetails(session)}
                    >
                      <Eye size={16} />
                    </button>
                  </td>
                </tr>
              ))}
              {sessions.length === 0 && (
                <tr>
                  <td colSpan="11" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    {showActive ? 'No active sessions' : 'Brak wyników dla wybranych filtrów'}
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
