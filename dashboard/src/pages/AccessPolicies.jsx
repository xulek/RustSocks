import React, { useEffect, useMemo, useState } from 'react'
import { Edit2, Play, Plus, RefreshCcw, Trash2, X } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']
const AUTH_METHODS = ['none', 'userpass', 'pam.address', 'pam.username', 'gssapi']

const emptyPolicy = () => ({
  id: '',
  enabled: true,
  mode: 'enforce',
  description: '',
  users: [],
  groups: [],
  action: 'allow',
  destinations: ['*'],
  ports: ['*'],
  protocols: ['tcp'],
  priority: 100,
  enforce_conditions: false,
  conditions: {
    source_ips: [],
    auth_methods: [],
    schedule: null,
    not_before: null,
    expires_at: null,
    max_active_connections: null,
    max_connections_per_minute: null,
    daily_transfer_limit_bytes: null,
    monthly_transfer_limit_bytes: null
  },
  owner: null,
  ticket: null,
  tags: []
})

const csv = (values = []) => values.join(', ')
const splitCsv = (value) => value.split(',').map((item) => item.trim()).filter(Boolean)
const nullableNumber = (value) => value === '' || value === null || value === undefined ? null : Number(value)
const toLocalInput = (value) => value ? new Date(value).toISOString().slice(0, 16) : ''
const fromLocalInput = (value) => value ? new Date(value).toISOString() : null

function PolicyModal({ policy, editing, onClose, onSaved }) {
  const [form, setForm] = useState(() => ({
    ...policy,
    usersText: csv(policy.users),
    groupsText: csv(policy.groups),
    destinationsText: csv(policy.destinations),
    portsText: csv(policy.ports),
    sourceIpsText: csv(policy.conditions?.source_ips),
    tagsText: csv(policy.tags),
    notBeforeLocal: toLocalInput(policy.conditions?.not_before),
    expiresAtLocal: toLocalInput(policy.conditions?.expires_at),
    scheduleEnabled: Boolean(policy.conditions?.schedule),
    scheduleDays: policy.conditions?.schedule?.days || [],
    scheduleStart: policy.conditions?.schedule?.start || '07:00',
    scheduleEnd: policy.conditions?.schedule?.end || '20:00',
    scheduleOffset: policy.conditions?.schedule?.utc_offset_minutes ?? 0
  }))
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState(null)

  const setField = (name, value) => setForm((prev) => ({ ...prev, [name]: value }))
  const setCondition = (name, value) => setForm((prev) => ({
    ...prev,
    conditions: { ...prev.conditions, [name]: value }
  }))

  const toggleProtocol = (protocol) => {
    setField('protocols', form.protocols.includes(protocol)
      ? form.protocols.filter((item) => item !== protocol)
      : [...form.protocols, protocol])
  }

  const toggleAuthMethod = (method) => {
    const current = form.conditions?.auth_methods || []
    setCondition('auth_methods', current.includes(method)
      ? current.filter((item) => item !== method)
      : [...current, method])
  }

  const toggleDay = (day) => {
    setField('scheduleDays', form.scheduleDays.includes(day)
      ? form.scheduleDays.filter((item) => item !== day)
      : [...form.scheduleDays, day])
  }

  const buildPayload = () => ({
    id: form.id.trim(),
    enabled: Boolean(form.enabled),
    mode: form.mode,
    description: form.description.trim(),
    users: splitCsv(form.usersText),
    groups: splitCsv(form.groupsText),
    action: form.action,
    destinations: splitCsv(form.destinationsText),
    ports: splitCsv(form.portsText),
    protocols: form.protocols,
    priority: Number(form.priority),
    enforce_conditions: Boolean(form.enforce_conditions),
    conditions: {
      source_ips: splitCsv(form.sourceIpsText),
      auth_methods: form.conditions?.auth_methods || [],
      schedule: form.scheduleEnabled ? {
        days: form.scheduleDays,
        start: form.scheduleStart,
        end: form.scheduleEnd,
        utc_offset_minutes: Number(form.scheduleOffset)
      } : null,
      not_before: fromLocalInput(form.notBeforeLocal),
      expires_at: fromLocalInput(form.expiresAtLocal),
      max_active_connections: nullableNumber(form.conditions?.max_active_connections),
      max_connections_per_minute: nullableNumber(form.conditions?.max_connections_per_minute),
      daily_transfer_limit_bytes: nullableNumber(form.conditions?.daily_transfer_limit_bytes),
      monthly_transfer_limit_bytes: nullableNumber(form.conditions?.monthly_transfer_limit_bytes)
    },
    owner: form.owner?.trim() || null,
    ticket: form.ticket?.trim() || null,
    tags: splitCsv(form.tagsText)
  })

  const handleSubmit = async (event) => {
    event.preventDefault()
    setError(null)
    const payload = buildPayload()
    if (!payload.id || payload.destinations.length === 0 || payload.ports.length === 0 || payload.protocols.length === 0) {
      setError('ID, destination, port and at least one protocol are required.')
      return
    }
    setSaving(true)
    try {
      const response = await fetch(
        getApiUrl(editing ? `/api/acl/policies/${encodeURIComponent(policy.id)}` : '/api/acl/policies'),
        {
          method: editing ? 'PUT' : 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        }
      )
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new Error(data.message || 'Failed to save policy')
      onSaved()
    } catch (err) {
      setError(err.message)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="modal-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" style={{ maxWidth: 980, width: '95vw', maxHeight: '92vh', overflow: 'auto' }}>
        <div className="modal-header">
          <h3>{editing ? `Edit ${policy.id}` : 'Create access policy'}</h3>
          <button className="modal-close" onClick={onClose}><X /></button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="modal-content">
            {error && <div className="alert alert-error">{error}</div>}

            <div className="form-section">
              <h4>Identity and decision</h4>
              <div className="form-grid">
                <div className="form-group"><label>Policy ID</label><input value={form.id} disabled={editing} onChange={(e) => setField('id', e.target.value)} /></div>
                <div className="form-group"><label>Priority</label><input type="number" min="0" value={form.priority} onChange={(e) => setField('priority', e.target.value)} /></div>
                <div className="form-group"><label>Action</label><select value={form.action} onChange={(e) => setField('action', e.target.value)}><option value="allow">Allow</option><option value="block">Block</option></select></div>
                <div className="form-group"><label>Mode</label><select value={form.mode} onChange={(e) => setField('mode', e.target.value)}><option value="enforce">Enforce</option><option value="monitor">Monitor</option></select></div>
              </div>
              <div className="form-group"><label>Description</label><input value={form.description} onChange={(e) => setField('description', e.target.value)} /></div>
              <div style={{ display: 'flex', gap: 24, flexWrap: 'wrap', marginTop: 12 }}>
                <label><input type="checkbox" checked={form.enabled} onChange={(e) => setField('enabled', e.target.checked)} /> Enabled</label>
                <label><input type="checkbox" checked={form.enforce_conditions} disabled={form.action !== 'allow'} onChange={(e) => setField('enforce_conditions', e.target.checked)} /> Gate conditions</label>
              </div>
            </div>

            <div className="form-section">
              <h4>Subjects and targets</h4>
              <div className="form-grid">
                <div className="form-group"><label>Users (comma-separated)</label><input value={form.usersText} onChange={(e) => setField('usersText', e.target.value)} placeholder="alice, bob" /></div>
                <div className="form-group"><label>Groups</label><input value={form.groupsText} onChange={(e) => setField('groupsText', e.target.value)} placeholder="developers, vpn-users" /></div>
                <div className="form-group"><label>Destinations</label><input value={form.destinationsText} onChange={(e) => setField('destinationsText', e.target.value)} placeholder="github.com, *.githubusercontent.com, 10.0.0.0/8" /></div>
                <div className="form-group"><label>Ports</label><input value={form.portsText} onChange={(e) => setField('portsText', e.target.value)} placeholder="443, 8000-9000, *" /></div>
              </div>
              <div className="form-group"><label>Protocols</label><div style={{ display: 'flex', gap: 18 }}>{['tcp', 'udp', 'both'].map((item) => <label key={item}><input type="checkbox" checked={form.protocols.includes(item)} onChange={() => toggleProtocol(item)} /> {item}</label>)}</div></div>
            </div>

            <div className="form-section">
              <h4>Dynamic conditions</h4>
              <div className="form-grid">
                <div className="form-group"><label>Source IP/CIDR</label><input value={form.sourceIpsText} onChange={(e) => setField('sourceIpsText', e.target.value)} placeholder="10.0.0.0/8, 203.0.113.10" /></div>
                <div className="form-group"><label>UTC offset (minutes)</label><input type="number" min="-1440" max="1440" value={form.scheduleOffset} onChange={(e) => setField('scheduleOffset', e.target.value)} /></div>
                <div className="form-group"><label>Not before</label><input type="datetime-local" value={form.notBeforeLocal} onChange={(e) => setField('notBeforeLocal', e.target.value)} /></div>
                <div className="form-group"><label>Expires at</label><input type="datetime-local" value={form.expiresAtLocal} onChange={(e) => setField('expiresAtLocal', e.target.value)} /></div>
              </div>
              <div className="form-group"><label>Authentication methods</label><div style={{ display: 'flex', gap: 14, flexWrap: 'wrap' }}>{AUTH_METHODS.map((method) => <label key={method}><input type="checkbox" checked={(form.conditions?.auth_methods || []).includes(method)} onChange={() => toggleAuthMethod(method)} /> {method}</label>)}</div></div>
              <label style={{ display: 'block', margin: '14px 0 8px' }}><input type="checkbox" checked={form.scheduleEnabled} onChange={(e) => setField('scheduleEnabled', e.target.checked)} /> Enable schedule</label>
              {form.scheduleEnabled && <>
                <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginBottom: 12 }}>{DAYS.map((day) => <label key={day}><input type="checkbox" checked={form.scheduleDays.includes(day)} onChange={() => toggleDay(day)} /> {day}</label>)}</div>
                <div className="form-grid">
                  <div className="form-group"><label>Start</label><input type="time" value={form.scheduleStart} onChange={(e) => setField('scheduleStart', e.target.value)} /></div>
                  <div className="form-group"><label>End</label><input type="time" value={form.scheduleEnd} onChange={(e) => setField('scheduleEnd', e.target.value)} /></div>
                </div>
              </>}
            </div>

            <div className="form-section">
              <h4>Admission and quota limits</h4>
              <div className="form-grid">
                <div className="form-group"><label>Max active connections</label><input type="number" min="1" value={form.conditions?.max_active_connections ?? ''} onChange={(e) => setCondition('max_active_connections', e.target.value)} /></div>
                <div className="form-group"><label>Max connections / minute</label><input type="number" min="1" value={form.conditions?.max_connections_per_minute ?? ''} onChange={(e) => setCondition('max_connections_per_minute', e.target.value)} /></div>
                <div className="form-group"><label>Daily transfer bytes</label><input type="number" min="1" value={form.conditions?.daily_transfer_limit_bytes ?? ''} onChange={(e) => setCondition('daily_transfer_limit_bytes', e.target.value)} /></div>
                <div className="form-group"><label>Monthly transfer bytes</label><input type="number" min="1" value={form.conditions?.monthly_transfer_limit_bytes ?? ''} onChange={(e) => setCondition('monthly_transfer_limit_bytes', e.target.value)} /></div>
              </div>
            </div>

            <div className="form-section">
              <h4>Metadata</h4>
              <div className="form-grid">
                <div className="form-group"><label>Owner</label><input value={form.owner || ''} onChange={(e) => setField('owner', e.target.value)} /></div>
                <div className="form-group"><label>Ticket</label><input value={form.ticket || ''} onChange={(e) => setField('ticket', e.target.value)} /></div>
              </div>
              <div className="form-group"><label>Tags</label><input value={form.tagsText} onChange={(e) => setField('tagsText', e.target.value)} placeholder="production, temporary" /></div>
            </div>
          </div>
          <div className="modal-footer">
            <button type="button" className="btn btn-secondary" onClick={onClose}>Cancel</button>
            <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving...' : 'Save policy'}</button>
          </div>
        </form>
      </div>
    </div>
  )
}

function PolicySimulator() {
  const [form, setForm] = useState({
    user: 'anonymous', groups: '', source_ip: '127.0.0.1', auth_method: 'none',
    destination: 'example.com', port: 443, protocol: 'tcp', now: '',
    active_connections: 0, connections_last_minute: 0, bytes_today: 0, bytes_this_month: 0
  })
  const [result, setResult] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(false)
  const set = (name, value) => setForm((prev) => ({ ...prev, [name]: value }))

  const run = async (event) => {
    event.preventDefault()
    setLoading(true); setError(null); setResult(null)
    try {
      const response = await fetch(getApiUrl('/api/acl/test'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          user: form.user.trim(), groups: splitCsv(form.groups), source_ip: form.source_ip.trim(),
          auth_method: form.auth_method, destination: form.destination.trim(), port: Number(form.port), protocol: form.protocol,
          now: form.now ? new Date(form.now).toISOString() : null,
          usage: {
            active_connections: Number(form.active_connections), connections_last_minute: Number(form.connections_last_minute),
            bytes_today: Number(form.bytes_today), bytes_this_month: Number(form.bytes_this_month)
          }
        })
      })
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new Error(data.matched_rule || data.message || 'Simulation failed')
      setResult(data)
    } catch (err) { setError(err.message) } finally { setLoading(false) }
  }

  return <div className="card">
    <div className="card-header"><h3>Policy Explain / Simulator</h3></div>
    <form onSubmit={run} style={{ padding: 20 }}>
      <div className="form-grid">
        <div className="form-group"><label>User</label><input value={form.user} onChange={(e) => set('user', e.target.value)} /></div>
        <div className="form-group"><label>Groups</label><input value={form.groups} onChange={(e) => set('groups', e.target.value)} /></div>
        <div className="form-group"><label>Source IP</label><input value={form.source_ip} onChange={(e) => set('source_ip', e.target.value)} /></div>
        <div className="form-group"><label>Authentication</label><select value={form.auth_method} onChange={(e) => set('auth_method', e.target.value)}>{AUTH_METHODS.map((m) => <option key={m}>{m}</option>)}</select></div>
        <div className="form-group"><label>Destination</label><input value={form.destination} onChange={(e) => set('destination', e.target.value)} /></div>
        <div className="form-group"><label>Port</label><input type="number" min="1" max="65535" value={form.port} onChange={(e) => set('port', e.target.value)} /></div>
        <div className="form-group"><label>Protocol</label><select value={form.protocol} onChange={(e) => set('protocol', e.target.value)}><option value="tcp">tcp</option><option value="udp">udp</option><option value="both">both</option></select></div>
        <div className="form-group"><label>Evaluation time (optional)</label><input type="datetime-local" value={form.now} onChange={(e) => set('now', e.target.value)} /></div>
        <div className="form-group"><label>Active connections</label><input type="number" min="0" value={form.active_connections} onChange={(e) => set('active_connections', e.target.value)} /></div>
        <div className="form-group"><label>Connections last minute</label><input type="number" min="0" value={form.connections_last_minute} onChange={(e) => set('connections_last_minute', e.target.value)} /></div>
        <div className="form-group"><label>Bytes today</label><input type="number" min="0" value={form.bytes_today} onChange={(e) => set('bytes_today', e.target.value)} /></div>
        <div className="form-group"><label>Bytes this month</label><input type="number" min="0" value={form.bytes_this_month} onChange={(e) => set('bytes_this_month', e.target.value)} /></div>
      </div>
      <button className="btn btn-primary" disabled={loading}><Play size={16} /> {loading ? 'Evaluating...' : 'Explain decision'}</button>
      {error && <div className="alert alert-error" style={{ marginTop: 16 }}>{error}</div>}
      {result && <div style={{ marginTop: 20 }}>
        <h4>Decision: <span className={`badge ${result.decision === 'allow' ? 'badge-success' : 'badge-danger'}`}>{result.decision}</span></h4>
        <p>Matched policy: <strong>{result.matched_policy_id || '—'}</strong> · Rule: {result.matched_rule || 'default policy'}</p>
        <div className="table-container"><table><thead><tr><th>Source</th><th>ID</th><th>Priority</th><th>Action</th><th>Mode</th><th>Target</th><th>Conditions</th><th>Effective</th><th>Reason</th></tr></thead><tbody>
          {(result.trace || []).map((row, index) => <tr key={`${row.source}-${row.id || index}-${index}`}><td>{row.source}</td><td>{row.id || '—'}</td><td>{row.priority}</td><td>{row.action}</td><td>{row.mode || '—'}</td><td>{String(row.target_matched)}</td><td>{row.conditions_matched === null ? '—' : String(row.conditions_matched)}</td><td>{String(row.effective)}</td><td>{row.reason}</td></tr>)}
        </tbody></table></div>
      </div>}
    </form>
  </div>
}

export default function AccessPolicies() {
  const [policies, setPolicies] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [modal, setModal] = useState(null)

  const load = async () => {
    setLoading(true)
    try {
      const response = await fetch(getApiUrl('/api/acl/policies'))
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new Error(data.message || 'Failed to load access policies')
      setPolicies(data.policies || [])
      setError(null)
    } catch (err) { setError(err.message) } finally { setLoading(false) }
  }
  useEffect(() => { load() }, [])

  const sorted = useMemo(() => [...policies].sort((a, b) => Number(b.priority) - Number(a.priority)), [policies])
  const remove = async (policy) => {
    if (!confirm(`Delete policy "${policy.id}"?`)) return
    try {
      const response = await fetch(getApiUrl(`/api/acl/policies/${encodeURIComponent(policy.id)}`), { method: 'DELETE' })
      const data = await response.json().catch(() => ({}))
      if (!response.ok) throw new Error(data.message || 'Failed to delete policy')
      await load()
    } catch (err) { setError(err.message) }
  }

  return <div>
    <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 16 }}>
      <div><h2>Access Policies</h2><p>Dynamic policy engine with schedules, source networks, authentication requirements and admission quotas.</p></div>
      <div style={{ display: 'flex', gap: 8 }}><button className="btn btn-secondary" onClick={load}><RefreshCcw size={16} /> Refresh</button><button className="btn btn-primary" onClick={() => setModal({ policy: emptyPolicy(), editing: false })}><Plus size={16} /> New policy</button></div>
    </div>
    {error && <div className="alert alert-error">{error}</div>}
    <div className="card" style={{ marginBottom: 24 }}>
      <div className="card-header"><h3>Policies</h3></div>
      <div className="table-container"><table><thead><tr><th>ID</th><th>Priority</th><th>Decision</th><th>Mode</th><th>Subjects</th><th>Targets</th><th>Conditions</th><th>Status</th><th>Actions</th></tr></thead><tbody>
        {loading && <tr><td colSpan="9">Loading...</td></tr>}
        {!loading && sorted.length === 0 && <tr><td colSpan="9">No dynamic policies configured.</td></tr>}
        {sorted.map((policy) => {
          const subjects = [...(policy.users || []).map((v) => `user:${v}`), ...(policy.groups || []).map((v) => `group:${v}`)]
          const c = policy.conditions || {}
          const cond = [c.source_ips?.length ? 'source' : null, c.auth_methods?.length ? 'auth' : null, c.schedule ? 'schedule' : null, c.not_before || c.expires_at ? 'validity' : null, c.max_active_connections || c.max_connections_per_minute ? 'admission' : null, c.daily_transfer_limit_bytes || c.monthly_transfer_limit_bytes ? 'quota' : null].filter(Boolean)
          return <tr key={policy.id}>
            <td><strong>{policy.id}</strong>{policy.description && <div style={{ fontSize: 12, opacity: .7 }}>{policy.description}</div>}</td><td>{policy.priority}</td>
            <td><span className={`badge ${policy.action === 'allow' ? 'badge-success' : 'badge-danger'}`}>{policy.action}</span></td>
            <td><span className={`badge ${policy.mode === 'monitor' ? 'badge-warning' : 'badge-success'}`}>{policy.mode}</span>{policy.enforce_conditions && <div style={{ fontSize: 11 }}>GATE</div>}</td>
            <td>{subjects.length ? subjects.join(', ') : 'global'}</td><td>{(policy.destinations || []).join(', ')} · {(policy.ports || []).join(', ')}</td><td>{cond.length ? cond.join(', ') : 'none'}</td>
            <td>{policy.enabled ? 'enabled' : 'disabled'}</td><td><div style={{ display: 'flex', gap: 6 }}><button className="btn btn-secondary" onClick={() => setModal({ policy, editing: true })}><Edit2 size={14} /></button><button className="btn btn-secondary" onClick={() => remove(policy)}><Trash2 size={14} /></button></div></td>
          </tr>
        })}
      </tbody></table></div>
    </div>
    <PolicySimulator />
    {modal && <PolicyModal policy={modal.policy} editing={modal.editing} onClose={() => setModal(null)} onSaved={async () => { setModal(null); await load() }} />}
  </div>
}
