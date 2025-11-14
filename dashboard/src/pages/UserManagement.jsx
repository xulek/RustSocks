import React, { useState, useEffect } from 'react'
import { Eye, Plus, Trash2, UserPlus, Edit2, X } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'
import UserDetailModal from '../components/UserDetailModal'

function UserManagement() {
  const [users, setUsers] = useState([])
  const [groups, setGroups] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [selectedUser, setSelectedUser] = useState(null)
  const [detailOpen, setDetailOpen] = useState(false)

  // Create user modal
  const [showCreateUser, setShowCreateUser] = useState(false)
  const [newUsername, setNewUsername] = useState('')
  const [createUserLoading, setCreateUserLoading] = useState(false)
  const [createUserError, setCreateUserError] = useState(null)

  // Add user to group modal
  const [showAddToGroup, setShowAddToGroup] = useState(false)
  const [addToGroupUser, setAddToGroupUser] = useState(null)
  const [selectedGroupForAdd, setSelectedGroupForAdd] = useState('')
  const [addToGroupLoading, setAddToGroupLoading] = useState(false)
  const [addToGroupError, setAddToGroupError] = useState(null)

  // Add/Edit rule modal
  const [showRuleModal, setShowRuleModal] = useState(false)
  const [ruleModalUser, setRuleModalUser] = useState(null)
  const [editingRuleIndex, setEditingRuleIndex] = useState(null)
  const [ruleForm, setRuleForm] = useState({
    action: 'allow',
    description: '',
    destinations: '',
    ports: '',
    protocols: ['tcp'],
    priority: 50
  })
  const [ruleModalLoading, setRuleModalLoading] = useState(false)
  const [ruleModalError, setRuleModalError] = useState(null)

  useEffect(() => {
    fetchData()
  }, [])

  const fetchData = async () => {
    try {
      const [usersRes, groupsRes] = await Promise.all([
        fetch(getApiUrl('/api/acl/users')),
        fetch(getApiUrl('/api/acl/groups'))
      ])
      if (!usersRes.ok || !groupsRes.ok) throw new Error('Failed to fetch data')
      const usersData = await usersRes.json()
      const groupsData = await groupsRes.json()
      setUsers(usersData.users || [])
      setGroups(groupsData.groups || [])
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const fetchUserDetail = async (username) => {
    try {
      const response = await fetch(getApiUrl(`/api/acl/users/${username}`))
      if (!response.ok) throw new Error('Failed to fetch user details')
      return await response.json()
    } catch (err) {
      setError(err.message)
      return null
    }
  }

  const handleViewUserDetails = async (user) => {
    setSelectedUser(user)
    setDetailOpen(true)
    const detail = await fetchUserDetail(user.username)
    if (detail) {
      setSelectedUser(detail)
    }
  }

  const handleCreateUser = async (e) => {
    e.preventDefault()
    if (!newUsername.trim()) {
      setCreateUserError('Username is required')
      return
    }

    setCreateUserLoading(true)
    setCreateUserError(null)
    try {
      const response = await fetch(getApiUrl('/api/acl/users'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: newUsername.trim() })
      })

      const data = await response.json().catch(() => ({}))
      if (!response.ok) {
        throw new Error(data.message || 'Failed to create user')
      }

      setNewUsername('')
      setShowCreateUser(false)
      await fetchData()
    } catch (err) {
      setCreateUserError(err.message)
    } finally {
      setCreateUserLoading(false)
    }
  }

  const handleDeleteUser = async (username) => {
    if (!window.confirm(`Are you sure you want to delete user "${username}"?`)) return

    try {
      const response = await fetch(getApiUrl(`/api/acl/users/${username}`), {
        method: 'DELETE'
      })

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data.message || 'Failed to delete user')
      }

      await fetchData()
      if (selectedUser?.username === username) {
        const updatedDetail = await fetchUserDetail(username)
        if (updatedDetail) {
          setSelectedUser(updatedDetail)
        }
      }
    } catch (err) {
      alert(`Error: ${err.message}`)
    }
  }

  const handleOpenAddToGroup = (user) => {
    setAddToGroupUser(user)
    setSelectedGroupForAdd('')
    setShowAddToGroup(true)
  }

  const handleAddUserToGroup = async (e) => {
    e.preventDefault()
    if (!selectedGroupForAdd) {
      setAddToGroupError('Please select a group')
      return
    }

    setAddToGroupLoading(true)
    setAddToGroupError(null)
    try {
      const response = await fetch(
        getApiUrl(`/api/acl/users/${addToGroupUser.username}/groups`),
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ group_name: selectedGroupForAdd })
        }
      )

      const data = await response.json().catch(() => ({}))
      if (!response.ok) {
        throw new Error(data.message || 'Failed to add user to group')
      }

      setShowAddToGroup(false)
      setAddToGroupUser(null)
      await fetchData()
    } catch (err) {
      setAddToGroupError(err.message)
    } finally {
      setAddToGroupLoading(false)
    }
  }

  const handleRemoveFromGroup = async (username, groupName) => {
    if (!window.confirm(`Remove "${username}" from group "${groupName}"?`)) return

    try {
      const response = await fetch(
        getApiUrl(`/api/acl/users/${username}/groups/${groupName}`),
        { method: 'DELETE' }
      )

      if (!response.ok) {
        const data = await response.json().catch(() => ({}))
        throw new Error(data.message || 'Failed to remove user from group')
      }

      await fetchData()
    } catch (err) {
      alert(`Error: ${err.message}`)
    }
  }

  const handleOpenRuleModal = async (user, ruleIndex = null) => {
    let userDetail = user
    if (!Array.isArray(userDetail.rules)) {
      userDetail = await fetchUserDetail(user.username)
      if (!userDetail) return
    }

    setRuleModalUser(userDetail)
    setEditingRuleIndex(ruleIndex)

    if (ruleIndex !== null) {
      const rule = userDetail.rules[ruleIndex]
      setRuleForm({
        action: rule.action,
        description: rule.description,
        destinations: rule.destinations.join(', '),
        ports: rule.ports.join(', '),
        protocols: rule.protocols,
        priority: rule.priority
      })
    } else {
      setRuleForm({
        action: 'allow',
        description: '',
        destinations: '',
        ports: '',
        protocols: ['tcp'],
        priority: 50
      })
    }
    setShowRuleModal(true)
  }

  const handleSaveRule = async (e) => {
    e.preventDefault()
    if (!ruleModalUser) return
    const targetUsername = ruleModalUser.username

    const destinations = ruleForm.destinations
      .split(',')
      .map(d => d.trim())
      .filter(d => d.length > 0)

    const ports = ruleForm.ports
      .split(',')
      .map(p => p.trim())
      .filter(p => p.length > 0)

    if (destinations.length === 0 || ports.length === 0) {
      setRuleModalError('Please provide destinations and ports')
      return
    }

    setRuleModalLoading(true)
    setRuleModalError(null)
    try {
      const url = getApiUrl(`/api/acl/users/${ruleModalUser.username}/rules`)

      let body
      if (editingRuleIndex !== null) {
        const originalRule = ruleModalUser.rules[editingRuleIndex]
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
        throw new Error(data.message || 'Failed to save rule')
      }

      setShowRuleModal(false)
      setRuleModalUser(null)
      setEditingRuleIndex(null)
      await fetchData()
      if (selectedUser?.username === targetUsername) {
        const updatedDetail = await fetchUserDetail(targetUsername)
        if (updatedDetail) {
          setSelectedUser(updatedDetail)
        }
      }
    } catch (err) {
      setRuleModalError(err.message)
    } finally {
      setRuleModalLoading(false)
    }
  }

  const handleDeleteRule = async (username, rule) => {
    if (!window.confirm('Are you sure you want to delete this rule?')) return

    try {
      const response = await fetch(
        getApiUrl(`/api/acl/users/${username}/rules`),
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
        throw new Error(data.message || 'Failed to delete rule')
      }

      await fetchData()
    } catch (err) {
      alert(`Error: ${err.message}`)
    }
  }

  if (loading) return <div className="loading">Loading users...</div>

  return (
    <div>
      <div className="page-header">
        <h2>User Management</h2>
        <p>Manage users, group memberships, and per-user ACL rules</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
        <div className="card-header">
          <h3>Users</h3>
          <button
            className="btn btn-primary"
            onClick={() => setShowCreateUser(true)}
            style={{ display: 'flex', alignItems: 'center', gap: '8px', padding: '8px 16px' }}
          >
            <UserPlus size={16} />
            <span>New User</span>
          </button>
        </div>
        <div className="table-container">
          <table>
            <thead>
              <tr>
                <th>Username</th>
                <th>Groups</th>
                <th>ACL Rules</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {users.map((user, idx) => (
                <tr key={idx}>
                  <td><strong>{user.username}</strong></td>
                  <td>
                    {user.groups.length > 0 ? (
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px', alignItems: 'center' }}>
                        {user.groups.map((group, gidx) => (
                          <span
                            key={gidx}
                            className="badge badge-success"
                            style={{ display: 'inline-flex', alignItems: 'center', gap: '4px' }}
                          >
                            {group}
                            <button
                              type="button"
                              onClick={() => handleRemoveFromGroup(user.username, group)}
                              style={{
                                background: 'none',
                                border: 'none',
                                color: 'inherit',
                                cursor: 'pointer',
                                padding: '0',
                                marginLeft: '4px'
                              }}
                              title={`Remove from ${group}`}
                            >
                              <X size={12} />
                            </button>
                          </span>
                        ))}
                        <button
                          type="button"
                          className="icon-button"
                          onClick={() => handleOpenAddToGroup(user)}
                          title="Add to group"
                        >
                          <Plus size={14} />
                        </button>
                      </div>
                    ) : (
                      <button
                        type="button"
                        className="btn"
                        onClick={() => handleOpenAddToGroup(user)}
                        style={{ padding: '4px 8px', fontSize: '12px' }}
                      >
                        <Plus size={12} style={{ marginRight: '4px' }} />
                        Add to group
                      </button>
                    )}
                  </td>
                  <td>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                      <span>{user.rule_count} rules</span>
                      <button
                        type="button"
                        className="icon-button"
                        onClick={() => handleOpenRuleModal(user)}
                        title="Add rule"
                      >
                        <Plus size={14} />
                      </button>
                    </div>
                  </td>
                  <td>
                    <div style={{ display: 'flex', gap: '4px' }}>
                      <button
                        type="button"
                        className="icon-button"
                        title="View details"
                        onClick={() => handleViewUserDetails(user)}
                      >
                        <Eye size={16} />
                      </button>
                      <button
                        type="button"
                        className="icon-button"
                        onClick={() => handleDeleteUser(user.username)}
                        title="Delete user"
                        style={{ color: 'var(--danger)' }}
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
              {users.length === 0 && (
                <tr>
                  <td colSpan="4" style={{ textAlign: 'center', color: 'var(--text-secondary)' }}>
                    No users configured
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Modal: Create User */}
      {showCreateUser && (
        <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && setShowCreateUser(false)}>
          <div className="modal" style={{ width: '100%', maxWidth: '400px' }}>
            <div className="modal-header">
              <h3>Create New User</h3>
              <button type="button" className="icon-button" onClick={() => setShowCreateUser(false)}>
                <X size={20} />
              </button>
            </div>
            <form onSubmit={handleCreateUser}>
              <div className="modal-content">
                <div className="form-group">
                  <label>Username</label>
                  <input
                    type="text"
                    value={newUsername}
                    onChange={(e) => setNewUsername(e.target.value)}
                    placeholder="e.g. john"
                    autoFocus
                  />
                </div>
                {createUserError && <div className="error" style={{ marginTop: '12px' }}>{createUserError}</div>}
              </div>
              <div className="modal-footer">
                <button type="button" className="btn" onClick={() => setShowCreateUser(false)}>
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={createUserLoading}>
                  {createUserLoading ? 'Creating...' : 'Create'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Modal: Add User to Group */}
      {showAddToGroup && addToGroupUser && (
        <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && setShowAddToGroup(false)}>
          <div className="modal" style={{ width: '100%', maxWidth: '400px' }}>
            <div className="modal-header">
              <h3>Add "{addToGroupUser.username}" to Group</h3>
              <button type="button" className="icon-button" onClick={() => setShowAddToGroup(false)}>
                <X size={20} />
              </button>
            </div>
            <form onSubmit={handleAddUserToGroup}>
              <div className="modal-content">
                <div className="form-group">
                  <label>Select Group</label>
                  <select
                    value={selectedGroupForAdd}
                    onChange={(e) => setSelectedGroupForAdd(e.target.value)}
                    autoFocus
                  >
                    <option value="">-- Select a group --</option>
                    {groups
                      .filter(g => !addToGroupUser.groups.includes(g.name))
                      .map((group, idx) => (
                        <option key={idx} value={group.name}>
                          {group.name}
                        </option>
                      ))}
                  </select>
                </div>
                {addToGroupError && <div className="error" style={{ marginTop: '12px' }}>{addToGroupError}</div>}
              </div>
              <div className="modal-footer">
                <button type="button" className="btn" onClick={() => setShowAddToGroup(false)}>
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={addToGroupLoading}>
                  {addToGroupLoading ? 'Adding...' : 'Add to Group'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Modal: Add/Edit Rule */}
      {showRuleModal && ruleModalUser && (
        <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && setShowRuleModal(false)}>
          <div className="modal" style={{ width: '100%', maxWidth: '540px' }}>
            <div className="modal-header">
              <h3>{editingRuleIndex !== null ? 'Edit Rule' : 'Add Rule'} - user "{ruleModalUser.username}"</h3>
              <button type="button" className="icon-button" onClick={() => setShowRuleModal(false)}>
                <X size={20} />
              </button>
            </div>
            <form onSubmit={handleSaveRule}>
              <div className="modal-content">
                <div className="form-group">
                  <label>Action</label>
                  <select
                    value={ruleForm.action}
                    onChange={(e) => setRuleForm(prev => ({ ...prev, action: e.target.value }))}
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
                    onChange={(e) => setRuleForm(prev => ({ ...prev, description: e.target.value }))}
                    placeholder="e.g. Allow GitHub"
                  />
                </div>
                <div className="form-group">
                  <label>Destinations (comma separated)</label>
                  <input
                    type="text"
                    value={ruleForm.destinations}
                    onChange={(e) => setRuleForm(prev => ({ ...prev, destinations: e.target.value }))}
                    placeholder="e.g. github.com, 192.168.0.0/16, *.example.com"
                  />
                </div>
                <div className="form-group">
                  <label>Ports (comma separated or ranges)</label>
                  <input
                    type="text"
                    value={ruleForm.ports}
                    onChange={(e) => setRuleForm(prev => ({ ...prev, ports: e.target.value }))}
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
                              setRuleForm(prev => ({ ...prev, protocols: [...prev.protocols, proto] }))
                            } else {
                              setRuleForm(prev => ({ ...prev, protocols: prev.protocols.filter(p => p !== proto) }))
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
                    onChange={(e) => setRuleForm(prev => ({ ...prev, priority: e.target.value }))}
                  />
                </div>
                {ruleModalError && <div className="error" style={{ marginTop: '12px' }}>{ruleModalError}</div>}
              </div>
              <div className="modal-footer">
                <button type="button" className="btn" onClick={() => setShowRuleModal(false)}>
                  Cancel
                </button>
                <button type="submit" className="btn btn-primary" disabled={ruleModalLoading}>
                  {ruleModalLoading
                    ? (editingRuleIndex !== null ? 'Updating...' : 'Adding...')
                    : (editingRuleIndex !== null ? 'Update Rule' : 'Add Rule')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      <UserDetailModal
        open={detailOpen}
        user={selectedUser}
        onClose={() => {
          setDetailOpen(false)
          setSelectedUser(null)
        }}
        onEditRule={handleOpenRuleModal}
        onDeleteRule={handleDeleteRule}
      />
    </div>
  )
}

export default UserManagement
