import React, { useEffect } from 'react'
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

  if (!open) return null

  const handleOverlayClick = (e) => {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }

  return (
    <div className="drawer-overlay" onClick={handleOverlayClick}>
      <div className="drawer">
        <div className="drawer-header">
          <h3>Session Details</h3>
          <button type="button" className="icon-button" onClick={onClose} title="Close panel">
            <X size={18} />
          </button>
        </div>

        {loading && <div className="loading">Loading session details...</div>}
        {error && <div className="error">Failed to load details: {error}</div>}

        {!loading && !error && session && (
          <div className="drawer-content">
            <div className="detail-grid">
              <DetailRow label="Session ID" value={<code>{session.id}</code>} />
              <DetailRow label="User" value={session.user} />
              <DetailRow
                label="Status"
                value={
                  <span className={resolveStatusBadge(session.status)} style={{ textTransform: 'capitalize' }}>
                    {session.status}
                  </span>
                }
              />
              <DetailRow
                label="ACL Decision"
                value={
                  <span className={resolveAclBadge(session.acl_decision)} style={{ textTransform: 'uppercase' }}>
                    {session.acl_decision ?? 'N/A'}
                  </span>
                }
              />
              <DetailRow
                label="ACL Rule"
                value={session.acl_rule ? <code>{session.acl_rule}</code> : '—'}
              />
              <DetailRow
                label="Source"
                value={<code>{session.source_ip}:{session.source_port}</code>}
              />
              <DetailRow
                label="Destination"
                value={<code>{session.dest_ip}:{session.dest_port}</code>}
              />
              <DetailRow label="Protocol" value={session.protocol?.toUpperCase()} />
              <DetailRow label="Start" value={formatDateTime(session.start_time)} />
              <DetailRow label="End" value={formatDateTime(session.end_time)} />
              <DetailRow label="Duration" value={formatDuration(session.duration_seconds)} />
              <DetailRow label="Bytes Sent" value={formatBytes(session.bytes_sent)} />
              <DetailRow label="Bytes Received" value={formatBytes(session.bytes_received)} />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

export default SessionDetailDrawer
