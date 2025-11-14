import React from 'react'
import { BrowserRouter, Routes, Route, NavLink, Navigate } from 'react-router-dom'
import {
  LayoutDashboard,
  Activity,
  Shield,
  Users,
  Settings,
  FileText,
  Stethoscope,
  SignalHigh,
  LogOut
} from 'lucide-react'

import { AuthProvider, useAuth } from './contexts/AuthContext'
import Dashboard from './pages/Dashboard'
import Sessions from './pages/Sessions'
import AclRules from './pages/AclRules'
import UserManagement from './pages/UserManagement'
import Statistics from './pages/Statistics'
import Configuration from './pages/Configuration'
import Diagnostics from './pages/Diagnostics'
import Telemetry from './pages/Telemetry'
import Login from './pages/Login'
import { ROUTER_BASENAME } from './lib/basePath'

function ProtectedRoute({ children }) {
  const { user, loading } = useAuth()

  if (loading) {
    return (
      <div className="loading">
        <p>Loading...</p>
      </div>
    )
  }

  if (!user) {
    return <Navigate to="/login" replace />
  }

  return children
}

function DashboardLayout() {
  const { user, logout } = useAuth()

  const handleLogout = async () => {
    if (confirm('Are you sure you want to logout?')) {
      await logout()
    }
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="sidebar-header">
          <h1>RustSocks</h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: '14px' }}>Admin Dashboard</p>
          {user && (
            <p style={{ color: 'var(--text-secondary)', fontSize: '12px', marginTop: '4px' }}>
              {user.username}
            </p>
          )}
        </div>

        <nav className="sidebar-nav">
          <NavLink to="/" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`} end>
            <LayoutDashboard />
            <span>Dashboard</span>
          </NavLink>
          <NavLink to="/sessions" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <Activity />
            <span>Sessions</span>
          </NavLink>
          <NavLink to="/acl" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <Shield />
            <span>ACL Rules</span>
          </NavLink>
          <NavLink to="/users" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <Users />
            <span>Users</span>
          </NavLink>
          <NavLink to="/statistics" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <FileText />
            <span>Statistics</span>
          </NavLink>
          <NavLink to="/diagnostics" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <Stethoscope />
            <span>Diagnostics</span>
          </NavLink>
          <NavLink to="/telemetry" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <SignalHigh />
            <span>Telemetry</span>
          </NavLink>
          <NavLink to="/config" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
            <Settings />
            <span>Configuration</span>
          </NavLink>
        </nav>

        <div className="sidebar-footer">
          <button onClick={handleLogout} className="nav-item logout-button">
            <LogOut />
            <span>Logout</span>
          </button>
        </div>
      </aside>

      <main className="main-content">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/sessions" element={<Sessions />} />
          <Route path="/acl" element={<AclRules />} />
          <Route path="/users" element={<UserManagement />} />
          <Route path="/statistics" element={<Statistics />} />
          <Route path="/diagnostics" element={<Diagnostics />} />
          <Route path="/telemetry" element={<Telemetry />} />
          <Route path="/config" element={<Configuration />} />
        </Routes>
      </main>
    </div>
  )
}

function App() {
  return (
    <BrowserRouter basename={ROUTER_BASENAME}>
      <AuthProvider>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route
            path="/*"
            element={
              <ProtectedRoute>
                <DashboardLayout />
              </ProtectedRoute>
            }
          />
        </Routes>
      </AuthProvider>
    </BrowserRouter>
  )
}

export default App
