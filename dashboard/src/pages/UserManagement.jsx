import React, { useState, useEffect } from 'react'
import { Eye } from 'lucide-react'
import { getApiUrl } from '../lib/basePath'
import UserDetailModal from '../components/UserDetailModal'

function UserManagement() {
  const [users, setUsers] = useState([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [selectedUser, setSelectedUser] = useState(null)
  const [detailOpen, setDetailOpen] = useState(false)

  useEffect(() => {
    fetchUsers()
  }, [])

  const fetchUsers = async () => {
    try {
      const response = await fetch(getApiUrl('/api/acl/users'))
      if (!response.ok) throw new Error('Failed to fetch users')
      const data = await response.json()
      setUsers(data.users || [])
      setError(null)
    } catch (err) {
      setError(err.message)
    } finally {
      setLoading(false)
    }
  }

  const handleViewUserDetails = (user) => {
    setSelectedUser(user)
    setDetailOpen(true)
  }

  if (loading) return <div className="loading">Loading users...</div>

  return (
    <div>
      <div className="page-header">
        <h2>User Management</h2>
        <p>Manage users and their group memberships</p>
      </div>

      {error && <div className="error">Error: {error}</div>}

      <div className="card">
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
                      user.groups.map((group, gidx) => (
                        <span key={gidx} className="badge badge-success" style={{ marginRight: '4px' }}>
                          {group}
                        </span>
                      ))
                    ) : (
                      <span style={{ color: 'var(--text-secondary)' }}>No groups</span>
                    )}
                  </td>
                  <td>{user.rule_count} rules</td>
                  <td>
                    <button
                      type="button"
                      className="icon-button"
                      title="View user details and sessions"
                      onClick={() => handleViewUserDetails(user)}
                    >
                      <Eye size={16} />
                    </button>
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

      <UserDetailModal
        open={detailOpen}
        user={selectedUser}
        onClose={() => {
          setDetailOpen(false)
          setSelectedUser(null)
        }}
      />
    </div>
  )
}

export default UserManagement
