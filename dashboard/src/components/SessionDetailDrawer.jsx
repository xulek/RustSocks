import React from 'react'
import { X } from 'lucide-react'
import { formatBytes, formatDateTime, formatDuration } from '../lib/format'

const DetailRow = ({ label, value }) => (
  <div className="detail-row">
    <div className="detail-label">{label}</div>
    <div className="detail-value">{value ?? '—'}</div>
  </div>
)

const resolveStatusBadge = (status) => {
  const normalized = status?.toLowerCase()
  const map = {
    active: 'badge badge-success',
    closed: 'badge badge-warning',
    failed: 'badge badge-danger',
    rejected_by_acl: 'badge badge-danger'
  }
  return map[normalized] || 'badge badge-warning'
}

const resolveAclBadge = (decision) => {
  if (!decision) return 'badge badge-warning'
  return decision.toLowerCase() === 'allow' ? 'badge badge-success' : 'badge badge-danger'
}

function SessionDetailDrawer({ open, session, loading, error, onClose }) {
  if (!open) return null

  return (
    <div className="drawer-overlay">
      <div className="drawer">
        <div className="drawer-header">
          <h3>Szczegóły sesji</h3>
          <button type="button" className="icon-button" onClick={onClose} title="Zamknij panel">
            <X size={18} />
          </button>
        </div>

        {loading && <div className="loading">Ładowanie szczegółów...</div>}
        {error && <div className="error">Nie udało się pobrać danych: {error}</div>}

        {!loading && !error && session && (
          <div className="drawer-content">
            <div className="detail-grid">
              <DetailRow label="Session ID" value={<code>{session.id}</code>} />
              <DetailRow label="Użytkownik" value={session.user} />
              <DetailRow
                label="Status"
                value={
                  <span className={resolveStatusBadge(session.status)} style={{ textTransform: 'capitalize' }}>
                    {session.status}
                  </span>
                }
              />
              <DetailRow
                label="Decyzja ACL"
                value={
                  <span className={resolveAclBadge(session.acl_decision)} style={{ textTransform: 'uppercase' }}>
                    {session.acl_decision ?? 'N/A'}
                  </span>
                }
              />
              <DetailRow
                label="Reguła ACL"
                value={session.acl_rule ? <code>{session.acl_rule}</code> : '—'}
              />
              <DetailRow
                label="Źródło"
                value={<code>{session.source_ip}:{session.source_port}</code>}
              />
              <DetailRow
                label="Cel"
                value={<code>{session.dest_ip}:{session.dest_port}</code>}
              />
              <DetailRow label="Protokół" value={session.protocol?.toUpperCase()} />
              <DetailRow label="Start" value={formatDateTime(session.start_time)} />
              <DetailRow label="Koniec" value={formatDateTime(session.end_time)} />
              <DetailRow label="Czas trwania" value={formatDuration(session.duration_seconds)} />
              <DetailRow label="Wysłano" value={formatBytes(session.bytes_sent)} />
              <DetailRow label="Odebrano" value={formatBytes(session.bytes_received)} />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

export default SessionDetailDrawer
