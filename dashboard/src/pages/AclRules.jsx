import React, { useState, useEffect } from 'react'
import { Shield, Plus, Trash2, Edit } from 'lucide-react'

function AclRules() {
  const [groups, setGroups] = useState([])
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [selectedGroup, setSelectedGroup] = useState(null)

  useEffect(() => {
    fetchAclData()
  }, [])

  const fetchAclData = async () => {
    try {
      const [groupsRes, usersRes] = await Promise.all([
        fetch('/api/acl/groups'),
        fetch('/api/acl/users')
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
      const response = await fetch(`/api/acl/groups/${groupName}`)
      if (!response.ok) throw new Error('Failed to fetch group details')
      const data = await response.json()
      setSelectedGroup(data)
    } catch (err) {
      setError(err.message)
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
    </div>
  )
}

export default AclRules
