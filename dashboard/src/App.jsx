import React from 'react'
import { BrowserRouter, Routes, Route, NavLink } from 'react-router-dom'
import {
  LayoutDashboard,
  Activity,
  Shield,
  Users,
  Settings,
  FileText
} from 'lucide-react'

import Dashboard from './pages/Dashboard'
import Sessions from './pages/Sessions'
import AclRules from './pages/AclRules'
import UserManagement from './pages/UserManagement'
import Statistics from './pages/Statistics'
import Configuration from './pages/Configuration'

function App() {
  return (
    <BrowserRouter>
      <div className="app">
        <aside className="sidebar">
          <div className="sidebar-header">
            <h1>RustSocks</h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '14px' }}>Admin Dashboard</p>
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
            <NavLink to="/config" className={({ isActive }) => `nav-item ${isActive ? 'active' : ''}`}>
              <Settings />
              <span>Configuration</span>
            </NavLink>
          </nav>
        </aside>

        <main className="main-content">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/sessions" element={<Sessions />} />
            <Route path="/acl" element={<AclRules />} />
            <Route path="/users" element={<UserManagement />} />
            <Route path="/statistics" element={<Statistics />} />
            <Route path="/config" element={<Configuration />} />
          </Routes>
        </main>
      </div>
    </BrowserRouter>
  )
}

export default App
