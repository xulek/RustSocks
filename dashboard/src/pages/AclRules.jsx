import React, { useState, useEffect } from 'react'
import { Play, RefreshCcw } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function AclRules() {
  const [groups, setGroups] = useState([])
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [selectedGroup, setSelectedGroup] = useState(null)
  const [testForm, setTestForm] = useState({
    user: '',
    destination: '',
    port: '',
    protocol: 'tcp'
  })
  const [testResult, setTestResult] = useState(null)
  const [testLoading, setTestLoading] = useState(false)
  const [testError, setTestError] = useState(null)
  const [reloadStatus, setReloadStatus] = useState(null)
  const [reloadLoading, setReloadLoading] = useState(false)

  useEffect(() => {
    fetchAclData()
  }, [])

  const fetchAclData = async () => {
    try {
      const [groupsRes, usersRes] = await Promise.all([
        fetch(getApiUrl('/api/acl/groups')),
        fetch(getApiUrl('/api/acl/users'))
      ])

      if (!groupsRes.ok || !usersRes.ok) {
        throw new Error('Failed to fetch ACL data')
      }

      const groupsData = await groupsRes.json()
      const usersData = await usersRes.json()

      setGroups(groupsData.groups || [])
      setUsers(usersData.users || [])
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const fetchGroupDetail = async (groupName) => {
    try {
      const response = await fetch(getApiUrl(`/api/acl/groups/${groupName}`))
      if (!response.ok) throw new Error('Failed to fetch group details')
      const data = await response.json()
      setSelectedGroup(data)
    } catch (err) {
      setError(err.message)
    }
  }

  const handleTestChange = (field, value) => {
    setTestForm((prev) => ({
      ...prev,
      [field]: value
    }))
  }

  const handleTestSubmit = async (event) => {
    event.preventDefault()
    setTestError(null)
    setTestResult(null)

    if (!testForm.user.trim() || !testForm.destination.trim() || !testForm.port) {
      setTestError('Uzupełnij użytkownika, adres i port.')
      return
    }

    const portValue = Number(testForm.port)
    if (Number.isNaN(portValue) || portValue <= 0 || portValue > 65535) {
      setTestError('Port musi być liczbą z zakresu 1-65535.')
      return
    }

    setTestLoading(true)
    try {
      const response = await fetch(getApiUrl('/api/acl/test'), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({
          user: testForm.user.trim(),
          destination: testForm.destination.trim(),
          port: portValue,
          protocol: testForm.protocol
        })
      })

      const data = await response.json().catch(() => null)
      if (!response.ok) {
        throw new Error(data?.matched_rule || data?.message || 'ACL test failed')
      }

      setTestResult(data)
      setTestError(null)
    } catch (err) {
      setTestError(err.message)
    } finally {
      setTestLoading(false)
    }
  }

  const handleReloadAcl = async () => {
    setReloadLoading(true)
    setReloadStatus(null)
    try {
      const response = await fetch(getApiUrl('/api/admin/reload-acl'), {
        method: 'POST'
      })

      const data = await response.json().catch(() => null)
      if (!response.ok) {
        throw new Error(data?.message || 'Nie udało się przeładować ACL')
      }

      setReloadStatus({
        success: data?.success ?? true,
        message: data?.message || 'Konfiguracja ACL została przeładowana.'
      })

      await fetchAclData()
    } catch (err) {
      setReloadStatus({
        success: false,
        message: err.message
      })
    } finally {
      setReloadLoading(false)
    }
  }

  if (loading) return <div className="loading">Loading ACL rules...</div>

  return (
    <div>
      <div className="page-header">
        <h2>ACL Rules</h2>
        <p>Manage Access Control List rules for groups and users</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 2fr', gap: '24px' }}>
        {/* Groups List */}
        <div className="card">
          <div className="card-header">
            <h3>Groups</h3>
          </div>
          <div>
            {groups.map((group, idx) => (
              <div
                key={idx}
                style={{
                  padding: '12px',
                  marginBottom: '8px',
                  backgroundColor: selectedGroup?.name === group.name ? 'rgba(59, 130, 246, 0.2)' : 'transparent',
                  borderRadius: '8px',
                  cursor: 'pointer',
                  border: '1px solid var(--border)'
                }}
                onClick={() => fetchGroupDetail(group.name)}
              >
                <div style={{ fontWeight: 'bold' }}>{group.name}</div>
                <div style={{ fontSize: '14px', color: 'var(--text-secondary)' }}>
                  {group.rule_count} rules
                </div>
              </div>
            ))}
            {groups.length === 0 && (
              <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '20px' }}>
                No groups configured
              </div>
            )}
          </div>
        </div>

        {/* Group Details */}
        <div className="card">
          <div className="card-header">
            <h3>{selectedGroup ? selectedGroup.name : 'Select a group'}</h3>
          </div>
          {selectedGroup ? (
            <div className="table-container">
              <table>
                <thead>
                  <tr>
                    <th>Action</th>
                    <th>Description</th>
                    <th>Destinations</th>
                    <th>Ports</th>
                    <th>Protocol</th>
                    <th>Priority</th>
                  </tr>
                </thead>
                <tbody>
                  {selectedGroup.rules.map((rule, idx) => (
                    <tr key={idx}>
                      <td>
                        <span className={`badge ${rule.action === 'allow' ? 'badge-success' : 'badge-danger'}`}>
                          {rule.action}
                        </span>
                      </td>
                      <td>{rule.description}</td>
                      <td><code>{rule.destinations.join(', ')}</code></td>
                      <td><code>{rule.ports.join(', ')}</code></td>
                      <td>{rule.protocols.join(', ')}</td>
                      <td>{rule.priority}</td>
                    </tr>
                  ))}
                  {selectedGroup.rules.length === 0 && (
                    <tr>
                      <td colSpan="6" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                        No rules configured for this group
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          ) : (
            <div style={{ textAlign: 'center', color: 'var(--text-secondary)', padding: '40px' }}>
              Select a group to view its rules
            </div>
          )}
        </div>
      </div>

      {/* Users Section */}
      <div className="card" style={{ marginTop: '24px' }}>
        <div className="card-header">
          <h3>Users with ACL Rules</h3>
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>Username</th>
                <th>Groups</th>
                <th>Rules</th>
              </tr>
            </thead>
            <tbody>
              {users.map((user, idx) => (
                <tr key={idx}>
                  <td><strong>{user.username}</strong></td>
                  <td>
                    {user.groups.map((group, gidx) => (
                      <span key={gidx} className="badge badge-success" style={{ marginRight: '4px' }}>
                        {group}
                      </span>
                    ))}
                  </td>
                  <td>{user.rule_count} rules</td>
                </tr>
              ))}
              {users.length === 0 && (
                <tr>
                  <td colSpan="3" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    No users with ACL rules
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      <div className="card" style={{ marginTop: '24px' }}>
        <div className="card-header">
          <h3>ACL Toolbox</h3>
        </div>
        <div className="tools-grid">
          <form className="tool-card" onSubmit={handleTestSubmit}>
            <h4>Przetestuj decyzję ACL</h4>
            <p className="subtle-text">Sprawdź czy połączenie zostanie przepuszczone lub zablokowane.</p>
            <div className="form-group">
              <label>Użytkownik</label>
              <input
                type="text"
                value={testForm.user}
                onChange={(e) => handleTestChange('user', e.target.value)}
                placeholder="np. admin"
              />
            </div>
            <div className="form-group">
              <label>Adres docelowy</label>
              <input
                type="text"
                value={testForm.destination}
                onChange={(e) => handleTestChange('destination', e.target.value)}
                placeholder="np. 8.8.8.8 lub domena"
              />
            </div>
            <div className="form-group">
              <label>Port</label>
              <input
                type="number"
                min="1"
                max="65535"
                value={testForm.port}
                onChange={(e) => handleTestChange('port', e.target.value)}
                placeholder="np. 443"
              />
            </div>
            <div className="form-group">
              <label>Protokół</label>
              <select
                value={testForm.protocol}
                onChange={(e) => handleTestChange('protocol', e.target.value)}
              >
                <option value="tcp">TCP</option>
                <option value="udp">UDP</option>
                <option value="both">TCP + UDP</option>
              </select>
            </div>
            <button type="submit" className="btn btn-primary" disabled={testLoading}>
              <Play size={16} style={{ marginRight: '8px' }} />
              {testLoading ? 'Testowanie...' : 'Testuj reguły'}
            </button>
            {testError && (
              <div className="status-message error">{testError}</div>
            )}
            {testResult && (
              <div className={`status-message ${testResult.decision === 'allow' ? 'success' : 'error'}`}>
                <div style={{ marginBottom: '8px' }}>
                  Decyzja:{' '}
                  <span className={`badge ${testResult.decision === 'allow' ? 'badge-success' : 'badge-danger'}`}>
                    {testResult.decision.toUpperCase()}
                  </span>
                </div>
                {testResult.matched_rule ? (
                  <div>
                    Dopasowana reguła: <code>{testResult.matched_rule}</code>
                  </div>
                ) : (
                  <div>Brak dopasowanej reguły.</div>
                )}
              </div>
            )}
          </form>
          <div className="tool-card">
            <h4>Przeładuj konfigurację ACL</h4>
            <p className="subtle-text">
              Wczytaj zmiany z pliku konfiguracyjnego bez restartu usługi.
            </p>
            <button
              type="button"
              className="btn"
              onClick={handleReloadAcl}
              disabled={reloadLoading}
            >
              <RefreshCcw size={16} style={{ marginRight: '8px' }} />
              {reloadLoading ? 'Przeładowywanie...' : 'Przeładuj teraz'}
            </button>
            {reloadStatus && (
              <div className={`status-message ${reloadStatus.success ? 'success' : 'error'}`}>
                {reloadStatus.message}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

export default AclRules
