import React, { useState, useEffect } from 'react'
import { getApiUrl } from '../lib/basePath'

function SystemResources() {
  const [resources, setResources] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)

  useEffect(() => {
    fetchResources()
    const interval = setInterval(fetchResources, 5000) // Refresh every 5 seconds
    return () => clearInterval(interval)
  }, [])

  const fetchResources = async () => {
    try {
      const response = await fetch(getApiUrl('/api/system/resources'))
      if (!response.ok) throw new Error('Failed to fetch system resources')
      const data = await response.json()
      setResources(data)
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const getStatusColor = (percent) => {
    if (percent < 60) return '#10b981' // Green
    if (percent < 80) return '#f59e0b' // Yellow
    return '#ef4444' // Red
  }

  const getStatusLabel = (percent) => {
    if (percent < 60) return 'Normal'
    if (percent < 80) return 'Warning'
    return 'Critical'
  }

  const formatBytes = (bytes) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
  }

  if (loading && !resources) {
    return (
      <div className="card">
        <div className="card-header">
          <h3>System Resources</h3>
        </div>
        <div className="loading">Loading system resources...</div>
      </div>
    )
  }

  if (error) {
    return (
      <div className="card">
        <div className="card-header">
          <h3>System Resources</h3>
        </div>
        <div className="error">Unable to load system resources: {error}</div>
      </div>
    )
  }

  if (!resources) return null

  return (
    <div className="card">
      <div className="card-header">
        <h3>System Resources</h3>
      </div>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
          gap: '16px',
          padding: '16px 16px 0'
        }}
      >
        {/* System CPU */}
        <div className="resource-item">
          <div className="resource-label">System CPU</div>
          <div className="resource-value">{resources.system_cpu_percent.toFixed(1)}%</div>
          <div className="resource-bar-container">
            <div
              className="resource-bar"
              style={{
                width: `${Math.min(resources.system_cpu_percent, 100)}%`,
                backgroundColor: getStatusColor(resources.system_cpu_percent)
              }}
            />
          </div>
          <div
            className="resource-status"
            style={{ color: getStatusColor(resources.system_cpu_percent) }}
          >
            {getStatusLabel(resources.system_cpu_percent)}
          </div>
        </div>

        {/* System RAM */}
        <div className="resource-item">
          <div className="resource-label">System RAM</div>
          <div className="resource-value">{resources.system_ram_percent.toFixed(1)}%</div>
          <div className="resource-bar-container">
            <div
              className="resource-bar"
              style={{
                width: `${Math.min(resources.system_ram_percent, 100)}%`,
                backgroundColor: getStatusColor(resources.system_ram_percent)
              }}
            />
          </div>
          <div
            className="resource-status"
            style={{ color: getStatusColor(resources.system_ram_percent) }}
          >
            {formatBytes(resources.system_ram_used_bytes)} / {formatBytes(resources.system_ram_total_bytes)}
          </div>
        </div>

        {/* RustSocks CPU */}
        <div className="resource-item">
          <div className="resource-label">RustSocks CPU</div>
          <div className="resource-value">{resources.process_cpu_percent.toFixed(1)}%</div>
          <div className="resource-bar-container">
            <div
              className="resource-bar"
              style={{
                width: `${Math.min(resources.process_cpu_percent, 100)}%`,
                backgroundColor: getStatusColor(resources.process_cpu_percent)
              }}
            />
          </div>
          <div
            className="resource-status"
            style={{ color: getStatusColor(resources.process_cpu_percent) }}
          >
            {getStatusLabel(resources.process_cpu_percent)}
          </div>
        </div>

        {/* RustSocks RAM */}
        <div className="resource-item">
          <div className="resource-label">RustSocks RAM</div>
          <div className="resource-value">{formatBytes(resources.process_ram_bytes)}</div>
          <div className="resource-bar-container">
            <div
              className="resource-bar"
              style={{
                width: `${Math.min((resources.process_ram_bytes / resources.system_ram_total_bytes) * 100, 100)}%`,
                backgroundColor: getStatusColor((resources.process_ram_bytes / resources.system_ram_total_bytes) * 100)
              }}
            />
          </div>
          <div
            className="resource-status"
            style={{ color: 'var(--text-secondary)' }}
          >
            {((resources.process_ram_bytes / resources.system_ram_total_bytes) * 100).toFixed(1)}% of system
          </div>
        </div>
      </div>
    </div>
  )
}

export default SystemResources
