export const formatBytes = (bytes) => {
  if (bytes === undefined || bytes === null) return '—'
  if (bytes === 0) return '0 B'

  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  const value = bytes / Math.pow(k, i)

  return `${Math.round(value * 100) / 100} ${sizes[i]}`
}

export const formatDateTime = (input) => {
  if (!input) return '—'
  const date = typeof input === 'string' ? new Date(input) : input
  return date.toLocaleString()
}

export const formatDuration = (seconds) => {
  if (seconds === undefined || seconds === null) return '—'
  if (seconds < 60) return `${seconds}s`

  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)

  const parts = []
  if (hours) parts.push(`${hours}h`)
  if (minutes) parts.push(`${minutes}m`)
  if (!hours && secs) parts.push(`${secs}s`)

  return parts.join(' ')
}
