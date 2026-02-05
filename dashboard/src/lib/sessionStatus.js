export const SESSION_STATUS_OPTIONS = [
  { value: '', label: 'Any Status' },
  { value: 'active', label: 'Active' },
  { value: 'closed', label: 'Closed' },
  { value: 'terminated_by_admin', label: 'Terminated by Admin' },
  { value: 'failed', label: 'Failed' },
  { value: 'rejected_by_acl', label: 'Rejected by ACL' }
]

const STATUS_LABELS = {
  active: 'Active',
  closed: 'Closed',
  terminated_by_admin: 'Terminated by Admin',
  failed: 'Failed',
  rejected_by_acl: 'Rejected by ACL'
}

const STATUS_BADGES = {
  active: 'badge-success',
  closed: 'badge-warning',
  terminated_by_admin: 'badge-warning',
  failed: 'badge-danger',
  rejected_by_acl: 'badge-danger'
}

export const normalizeSessionStatus = (status) => (status || '').toLowerCase()

export const getSessionStatusLabel = (status) => {
  const normalized = normalizeSessionStatus(status)
  return STATUS_LABELS[normalized] || status || 'Unknown'
}

export const getSessionStatusBadgeClass = (status) => {
  const normalized = normalizeSessionStatus(status)
  return `badge ${STATUS_BADGES[normalized] || 'badge-warning'}`
}
