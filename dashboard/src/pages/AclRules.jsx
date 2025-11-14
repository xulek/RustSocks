import React, { useState, useEffect, useCallback } from 'react'
import { Play, RefreshCcw, Plus, X, Edit2, Trash2 } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'

function AclRules() {
  const [groups, setGroups] = useState([])
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [selectedGroup, setSelectedGroup] = useState(null)

  // Create group modal
  const [showCreateGroup, setShowCreateGroup] = useState(false)
  const [newGroupName, setNewGroupName] = useState('')
  const [createGroupLoading, setCreateGroupLoading] = useState(false)
  const [createGroupError, setCreateGroupError] = useState(null)

  // Add/Edit rule to group
  const [showAddRuleToGroup, setShowAddRuleToGroup] = useState(false)
  const [editingRuleIndex, setEditingRuleIndex] = useState(null)
  const [ruleForm, setRuleForm] = useState({
    action: 'allow',
    description: '',
    destinations: '',
    ports: '',
    protocols: ['tcp'],
    priority: 50
  })
  const [addRuleLoading, setAddRuleLoading] = useState(false)
  const [addRuleError, setAddRuleError] = useState(null)

  // Test ACL
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

  useEffect(() => {
    const handleEscape = (e) => {
      if (e.key === 'Escape') {
        if (showCreateGroup) {
          setShowCreateGroup(false)
        }
        if (showAddRuleToGroup) {
          setShowAddRuleToGroup(false)
          setEditingRuleIndex(null)
        }
      }
    }

    window.addEventListener('keydown', handleEscape)
    return () => window.removeEventListener('keydown', handleEscape)
  }, [showCreateGroup, showAddRuleToGroup])

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

  const handleOverlayClick = useCallback((e, onClose) => {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }, [])

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
      setTestError('Please provide user, destination, and port.')
      return
    }

    const portValue = Number(testForm.port)
    if (Number.isNaN(portValue) || portValue <= 0 || portValue > 65535) {
      setTestError('Port must be a number between 1 and 65535.')
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

  const handleCreateGroup = async (e) => {
    e.preventDefault()
    if (!newGroupName.trim()) {
      setCreateGroupError('Group name is required')
      return
    }

    setCreateGroupLoading(true)
    setCreateGroupError(null)
    try {
      const response = await fetch(getApiUrl('/api/acl/groups'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: newGroupName.trim() })
      })

      const data = await response.json().catch(() => ({}))
      if (!response.ok) {
        throw new Error(data?.message || 'Failed to create group')
      }

      setNewGroupName('')
      setShowCreateGroup(false)
      await fetchAclData()
    } catch (err) {
      setCreateGroupError(err.message)
    } finally {
      setCreateGroupLoading(false)
    }
  }

  const handleAddRuleChange = (field, value) => {
    setRuleForm(prev => ({
      ...prev,
      [field]: value
    }))
  }

  const handleAddRuleToGroup = async (e) => {
    e.preventDefault()
    if (!selectedGroup) {
      setAddRuleError('Select a group')
      return
    }

    const destinations = ruleForm.destinations
      .split(',')
      .map(d => d.trim())
      .filter(d => d.length > 0)

    const ports = ruleForm.ports
      .split(',')
      .map(p => p.trim())
      .filter(p => p.length > 0)

    if (destinations.length === 0 || ports.length === 0) {
      setAddRuleError('Provide destinations and ports')
      return
    }

    setAddRuleLoading(true)
    setAddRuleError(null)
    try {
      const url = getApiUrl(`/api/acl/groups/${selectedGroup.name}/rules`)

      let body
      if (editingRuleIndex !== null) {
      // UPDATE - send match (original rule) and update (new rule)
        const originalRule = selectedGroup.rules[editingRuleIndex]
        body = JSON.stringify({
          match: {
            destinations: originalRule.destinations,
            ports: originalRule.ports
          },
          update: {
            action: ruleForm.action,
            description: ruleForm.description || 'Custom rule',
            destinations,
            ports,
            protocols: ruleForm.protocols,
            priority: Number(ruleForm.priority)
          }
        })
      } else {
        // POST - standard addition
        body = JSON.stringify({
          action: ruleForm.action,
          description: ruleForm.description || 'Custom rule',
          destinations,
          ports,
          protocols: ruleForm.protocols,
          priority: Number(ruleForm.priority)
        })
      }

      const response = await fetch(url, {
        method: editingRuleIndex !== null ? 'PUT' : 'POST',
        headers: { 'Content-Type': 'application/json' },
        body
      })

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data?.message || `Failed to ${editingRuleIndex !== null ? 'update' : 'add'} rule`)
      }

      setRuleForm({
        action: 'allow',
        description: '',
        destinations: '',
        ports: '',
        protocols: ['tcp'],
        priority: 50
      })
      setEditingRuleIndex(null)
      setShowAddRuleToGroup(false)
      await fetchAclData()
      if (selectedGroup) {
        await fetchGroupDetail(selectedGroup.name)
      }
    } catch (err) {
      setAddRuleError(err.message)
    } finally {
      setAddRuleLoading(false)
    }
  }

  const handleEditRule = (rule, index) => {
    setEditingRuleIndex(index)
    setRuleForm({
      action: rule.action,
      description: rule.description,
      destinations: rule.destinations.join(', '),
      ports: rule.ports.join(', '),
      protocols: rule.protocols,
      priority: rule.priority
    })
    setShowAddRuleToGroup(true)
  }

  const handleDeleteRule = async (rule) => {
    if (!selectedGroup) return
    if (!window.confirm('Are you sure you want to delete this rule?')) return

    try {
      const response = await fetch(
        getApiUrl(`/api/acl/groups/${selectedGroup.name}/rules`),
        {
          method: 'DELETE',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            destinations: rule.destinations,
            ports: rule.ports
          })
        }
      )

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data?.message || 'Failed to delete rule')
      }

      await fetchAclData()
      if (selectedGroup) {
        await fetchGroupDetail(selectedGroup.name)
      }
    } catch (err) {
      alert(`Error: ${err.message}`)
    }
  }

  const handleDeleteGroup = async (groupName) => {
    if (!window.confirm(`Are you sure you want to delete group "${groupName}"?`)) return

    try {
      const response = await fetch(
        getApiUrl(`/api/acl/groups/${groupName}`),
        { method: 'DELETE' }
      )

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data?.message || 'Failed to delete group')
      }

      if (selectedGroup?.name === groupName) {
        setSelectedGroup(null)
      }
      await fetchAclData()
    } catch (err) {
      alert(`Error: ${err.message}`)
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
        throw new Error(data?.message || 'Failed to reload ACL')
      }

      setReloadStatus({
        success: data?.success ?? true,
        message: data?.message || 'ACL configuration reloaded.'
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
            <button
              className="btn btn-primary"
              onClick={() => setShowCreateGroup(true)}
              style={{ display: 'flex', alignItems: 'center', gap: '8px', padding: '8px 16px' }}
            >
              <Plus size={16} />
              <span>New Group</span>
            </button>
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
                  border: '1px solid var(--border)',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center'
                }}
              >
                <div
                  style={{ flex: 1, cursor: 'pointer' }}
                  onClick={() => fetchGroupDetail(group.name)}
                >
                  <div style={{ fontWeight: 'bold' }}>{group.name}</div>
                  <div style={{ fontSize: '14px', color: 'var(--text-secondary)' }}>
                    {group.rule_count} rules
                  </div>
                </div>
                <button
                  type="button"
                  className="icon-button"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleDeleteGroup(group.name)
                  }}
                  title="Delete group"
                  style={{ color: 'var(--danger)' }}
                >
                  <Trash2 size={16} />
                </button>
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
            {selectedGroup && (
              <button
                className="btn btn-primary"
                onClick={() => setShowAddRuleToGroup(true)}
                style={{ display: 'flex', alignItems: 'center', gap: '8px', padding: '8px 16px' }}
              >
                <Plus size={16} />
                <span>Add rule</span>
              </button>
            )}
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
                    <th>Actions</th>
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
                      <td>
                        <div style={{ display: 'flex', gap: '4px' }}>
                          <button
                            type="button"
                            className="icon-button"
                            onClick={() => handleEditRule(rule, idx)}
                            title="Edit rule"
                          >
                            <Edit2 size={14} />
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            onClick={() => handleDeleteRule(rule)}
                            title="Delete rule"
                            style={{ color: 'var(--danger)' }}
                          >
                            <Trash2 size={14} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                  {selectedGroup.rules.length === 0 && (
                    <tr>
                      <td colSpan="7" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
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
            <h4>Test ACL decision</h4>
            <p className="subtle-text">Verify whether a connection will be allowed or blocked.</p>
            <div className="form-group">
              <label>User</label>
              <input
                type="text"
                value={testForm.user}
                onChange={(e) => handleTestChange('user', e.target.value)}
                placeholder="e.g. admin"
              />
            </div>
            <div className="form-group">
              <label>Destination address</label>
              <input
                type="text"
                value={testForm.destination}
                onChange={(e) => handleTestChange('destination', e.target.value)}
                placeholder="e.g. 8.8.8.8 or example.com"
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
                placeholder="e.g. 443"
              />
            </div>
            <div className="form-group">
              <label>Protocol</label>
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
              {testLoading ? 'Testing...' : 'Test rules'}
            </button>
            {testError && (
              <div className="status-message error">{testError}</div>
            )}
            {testResult && (
              <div className={`status-message ${testResult.decision === 'allow' ? 'success' : 'error'}`}>
                <div style={{ marginBottom: '8px' }}>
                  Decision:{' '}
                  <span className={`badge ${testResult.decision === 'allow' ? 'badge-success' : 'badge-danger'}`}>
                    {testResult.decision.toUpperCase()}
                  </span>
                </div>
                {testResult.matched_rule ? (
                  <div>
                    Matched rule: <code>{testResult.matched_rule}</code>
                  </div>
                ) : (
                  <div>No matching rule.</div>
                )}
              </div>
            )}
          </form>
          <div className="tool-card">
            <h4>Reload ACL configuration</h4>
            <p className="subtle-text">
              Load changes from the configuration file without restarting the service.
            </p>
            <button
              type="button"
              className="btn"
              onClick={handleReloadAcl}
              disabled={reloadLoading}
            >
              <RefreshCcw size={16} style={{ marginRight: '8px' }} />
              {reloadLoading ? 'Reloading...' : 'Reload now'}
            </button>
            {reloadStatus && (
              <div className={`status-message ${reloadStatus.success ? 'success' : 'error'}`}>
                {reloadStatus.message}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Modal: Create Group */}
      {showCreateGroup && (
        <div className="modal-overlay" onClick={(e) => handleOverlayClick(e, () => setShowCreateGroup(false))}>
          <div className="modal" style={{ width: '100%', maxWidth: '400px' }}>
            <div className="modal-header">
            <h3>New Group</h3>
              <button
                type="button"
                className="icon-button"
                onClick={() => setShowCreateGroup(false)}
              >
                <X size={20} />
              </button>
            </div>
            <form onSubmit={handleCreateGroup}>
              <div className="modal-content">
                <div className="form-group">
                  <label>Group name</label>
                  <input
                    type="text"
                    value={newGroupName}
                    onChange={(e) => setNewGroupName(e.target.value)}
                    placeholder="e.g. developers"
                    autoFocus
                  />
                </div>
                {createGroupError && <div className="error" style={{ marginTop: '12px' }}>{createGroupError}</div>}
              </div>
              <div className="modal-footer">
                <button
                  type="button"
                  className="btn"
                  onClick={() => setShowCreateGroup(false)}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={createGroupLoading}
                >
                  {createGroupLoading ? 'Creating...' : 'Create'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Modal: Add Rule to Group */}
      {showAddRuleToGroup && selectedGroup && (
        <div className="modal-overlay" onClick={(e) => handleOverlayClick(e, () => {
          setShowAddRuleToGroup(false)
          setEditingRuleIndex(null)
        })}>
          <div className="modal" style={{ width: '100%', maxWidth: '540px' }}>
            <div className="modal-header">
              <h3>{editingRuleIndex !== null ? 'Edit rule' : 'Add rule'} - group "{selectedGroup.name}"</h3>
              <button
                type="button"
                className="icon-button"
                onClick={() => {
                  setShowAddRuleToGroup(false)
                  setEditingRuleIndex(null)
                }}
              >
                <X size={20} />
              </button>
            </div>
            <form onSubmit={handleAddRuleToGroup}>
              <div className="modal-content">
                <div className="form-group">
                  <label>Action</label>
                  <select
                    value={ruleForm.action}
                    onChange={(e) => handleAddRuleChange('action', e.target.value)}
                  >
                    <option value="allow">Allow</option>
                    <option value="block">Block</option>
                  </select>
                </div>
                <div className="form-group">
                  <label>Description</label>
                  <input
                    type="text"
                    value={ruleForm.description}
                    onChange={(e) => handleAddRuleChange('description', e.target.value)}
                    placeholder="e.g. Allow GitHub"
                  />
                </div>
                <div className="form-group">
                  <label>Destination addresses (comma-separated)</label>
                  <input
                    type="text"
                    value={ruleForm.destinations}
                    onChange={(e) => handleAddRuleChange('destinations', e.target.value)}
                    placeholder="e.g. github.com, 192.168.0.0/16, *.example.com"
                  />
                </div>
                <div className="form-group">
                  <label>Ports (comma-separated or ranges)</label>
                  <input
                    type="text"
                    value={ruleForm.ports}
                    onChange={(e) => handleAddRuleChange('ports', e.target.value)}
                    placeholder="e.g. 80,443,8000-9000, *"
                  />
                </div>
                <div className="form-group">
                  <label>Protocol</label>
                  <div style={{ display: 'flex', gap: '12px' }}>
                    {['tcp', 'udp', 'both'].map(proto => (
                      <label key={proto} style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                        <input
                          type="checkbox"
                          checked={ruleForm.protocols.includes(proto)}
                          onChange={(e) => {
                            if (e.target.checked) {
                              handleAddRuleChange('protocols', [...ruleForm.protocols, proto])
                            } else {
                              handleAddRuleChange('protocols', ruleForm.protocols.filter(p => p !== proto))
                            }
                          }}
                        />
                        {proto.toUpperCase()}
                      </label>
                    ))}
                  </div>
                </div>
                <div className="form-group">
                  <label>Priority</label>
                  <input
                    type="number"
                    min="0"
                    max="1000"
                    value={ruleForm.priority}
                    onChange={(e) => handleAddRuleChange('priority', e.target.value)}
                  />
                </div>
                {addRuleError && <div className="error" style={{ marginTop: '12px' }}>{addRuleError}</div>}
              </div>
              <div className="modal-footer">
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    setShowAddRuleToGroup(false)
                    setEditingRuleIndex(null)
                  }}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="btn btn-primary"
                  disabled={addRuleLoading}
                >
                  {addRuleLoading
                    ? (editingRuleIndex !== null ? 'Updating...' : 'Adding...')
                    : (editingRuleIndex !== null ? 'Update Rule' : 'Add Rule')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  )
}

export default AclRules
