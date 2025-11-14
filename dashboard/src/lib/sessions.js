import { formatBytes, formatDateTime, formatDuration } from './format'

export const DEFAULT_PAGE_SIZE = 25

const cleanString = (value) => (value ? value.trim() : '')

export const buildHistoryQuery = ({
  user,
  destination,
  status,
  hours,
  page = 1,
  pageSize = DEFAULT_PAGE_SIZE,
  sortBy = 'start_time',
  sortDir = 'desc'
}) => {
  const params = new URLSearchParams()

  params.set('page', Math.max(Number(page) || 1, 1))
  params.set('page_size', Math.min(Math.max(Number(pageSize) || DEFAULT_PAGE_SIZE, 1), 1000))

  const filters = [
    ['user', cleanString(user)],
    ['dest_ip', cleanString(destination)],
    ['status', cleanString(status)]
  ]

  filters.forEach(([key, value]) => {
    if (value) params.set(key, value)
  })

  const hoursValue = Number(hours)
  if (!Number.isNaN(hoursValue) && hoursValue > 0) {
    params.set('hours', hoursValue.toString())
  }

  // Add sorting parameters
  if (sortBy) params.set('sort_by', sortBy)
  if (sortDir) params.set('sort_dir', sortDir)

  return `?${params.toString()}`
}

export const buildHistoryUrl = (baseUrl, filters) =>
  `${baseUrl}${buildHistoryQuery(filters)}`

const escapeCsvValue = (value) => {
  if (value === null || value === undefined) {
    return ''
  }

  const str = String(value)
  if (str.includes('"') || str.includes(',') || str.includes('\n')) {
    return `"${str.replace(/"/g, '""')}"`
  }

  return str
}

export const sessionsToCsv = (sessions) => {
  const headers = [
    'session_id',
    'user',
    'status',
    'source',
    'destination',
    'protocol',
    'bytes_sent',
    'bytes_received',
    'duration',
    'acl_decision',
    'acl_rule',
    'start_time',
    'end_time'
  ]

  const rows = sessions.map((session) => [
    session.id,
    session.user,
    session.status,
    `${session.source_ip}:${session.source_port}`,
    `${session.dest_ip}:${session.dest_port}`,
    session.protocol,
    formatBytes(session.bytes_sent),
    formatBytes(session.bytes_received),
    formatDuration(session.duration_seconds),
    session.acl_decision,
    session.acl_rule ?? '',
    formatDateTime(session.start_time),
    session.end_time ? formatDateTime(session.end_time) : ''
  ])

  return [headers, ...rows]
    .map((row) => row.map(escapeCsvValue).join(','))
    .join('\n')
}
