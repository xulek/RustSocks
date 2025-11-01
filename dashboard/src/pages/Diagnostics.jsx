import React, { useState } from 'react'
import { Network, CheckCircle2, AlertTriangle } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function Diagnostics() {
  const [address, setAddress] = useState('')
  const [port, setPort] = useState('')
  const [timeout, setTimeoutValue] = useState('3000')
  const [result, setResult] = useState(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(null)

  const handleSubmit = async (event) => {
    event.preventDefault()

    const trimmedAddress = address.trim()
    if (!trimmedAddress) {
      setError('Destination address is required')
      return
    }

    const portNumber = Number.parseInt(port, 10)
    if (Number.isNaN(portNumber) || portNumber < 1 || portNumber > 65535) {
      setError('Port must be a number between 1 and 65535')
      return
    }

    const payload = {
      address: trimmedAddress,
      port: portNumber
    }

    const trimmedTimeout = timeout.toString().trim()
    if (trimmedTimeout.length > 0) {
      const timeoutNumber = Number.parseInt(trimmedTimeout, 10)
      if (Number.isNaN(timeoutNumber) || timeoutNumber < 1 || timeoutNumber > 120000) {
        setError('Timeout must be a number between 1 and 120000 milliseconds')
        return
      }
      payload.timeout_ms = timeoutNumber
    }

    setLoading(true)
    setError(null)

    try {
      const response = await fetch(getApiUrl('/api/diagnostics/connectivity'), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify(payload)
      })

      const data = await response.json()
      setResult(data)

      if (!response.ok && response.status >= 500) {
        setError(data.message || 'Connectivity test failed')
      }
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div>
      <div className="page-header">
        <h2>Diagnostics</h2>
        <p>Run ad-hoc connectivity checks towards a specific IP address and TCP port.</p>
      </div>

      {error && (
        <div className="error" style={{ marginBottom: '24px' }}>
          {error}
        </div>
      )}

      <div className="card">
        <div className="card-header">
          <h3 style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
            <Network size={20} />
            TCP Connectivity Test
          </h3>
        </div>

        <form onSubmit={handleSubmit} className="filters-form" style={{ gap: '20px' }}>
          <div className="filters-grid">
            <div className="form-group">
              <label htmlFor="diagnostics-address">Destination IP address</label>
              <input
                id="diagnostics-address"
                type="text"
                placeholder="e.g. 8.8.8.8"
                value={address}
                onChange={event => setAddress(event.target.value)}
                disabled={loading}
              />
            </div>
            <div className="form-group">
              <label htmlFor="diagnostics-port">Port</label>
              <input
                id="diagnostics-port"
                type="number"
                min="1"
                max="65535"
                placeholder="443"
                value={port}
                onChange={event => setPort(event.target.value)}
                disabled={loading}
              />
            </div>
            <div className="form-group">
              <label htmlFor="diagnostics-timeout">Timeout (ms)</label>
              <input
                id="diagnostics-timeout"
                type="number"
                min="1"
                max="120000"
                step="1"
                placeholder="3000"
                value={timeout}
                onChange={event => setTimeoutValue(event.target.value)}
                disabled={loading}
              />
              <span className="subtle-text">Leave empty to use the server default (3 seconds).</span>
            </div>
          </div>

          <div style={{ display: 'flex', gap: '12px', alignItems: 'center', flexWrap: 'wrap' }}>
            <button type="submit" className="btn btn-primary" disabled={loading}>
              {loading ? 'Testing...' : 'Test connection'}
            </button>
            {loading && <span className="subtle-text">Running connectivity test...</span>}
          </div>
        </form>
      </div>

      {result && (
        <div className="card">
          <div className="card-header">
            <h3 style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
              {result.success ? (
                <CheckCircle2 size={20} color="var(--success)" />
              ) : (
                <AlertTriangle size={20} color="var(--danger)" />
              )}
              Test result
            </h3>
            <span className={`badge ${result.success ? 'badge-success' : 'badge-danger'}`}>
              {result.success ? 'Success' : 'Failed'}
            </span>
          </div>

          <div style={{ display: 'grid', gap: '16px', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))' }}>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '6px' }}>Address</div>
              <div style={{ fontWeight: 600 }}>{result.address}</div>
            </div>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '6px' }}>Port</div>
              <div style={{ fontWeight: 600 }}>{result.port}</div>
            </div>
            <div>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '6px' }}>Latency</div>
              <div style={{ fontWeight: 600 }}>
                {typeof result.latency_ms === 'number' ? `${result.latency_ms} ms` : 'N/A'}
              </div>
            </div>
          </div>

          <div style={{ marginTop: '16px', color: 'var(--text-secondary)' }}>
            {result.message}
          </div>

          {!result.success && result.error && (
            <div className="subtle-text" style={{ marginTop: '8px' }}>
              Error: {result.error}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export default Diagnostics
