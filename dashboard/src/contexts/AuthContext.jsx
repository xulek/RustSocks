import React, { createContext, useContext, useState, useEffect } from 'react'
import { ROUTER_BASENAME } from '../lib/basePath'

const AuthContext = createContext(null)

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null)
  const [loading, setLoading] = useState(true)
  const [altchaEnabled, setAltchaEnabled] = useState(false)
  const [altchaChallengeUrl, setAltchaChallengeUrl] = useState(null)

  const apiUrl = (path) => `${ROUTER_BASENAME}${path}`

  useEffect(() => {
    checkAuth()
    fetchAltchaConfig()
  }, [])

  const checkAuth = async () => {
    try {
      const response = await fetch(apiUrl('/api/auth/check'), {
        credentials: 'include',
      })
      const data = await response.json()
      if (data.authenticated) {
        setUser({ username: data.username })
      } else {
        setUser(null)
      }
    } catch (error) {
      console.error('Auth check failed:', error)
      setUser(null)
    } finally {
      setLoading(false)
    }
  }

  const fetchAltchaConfig = async () => {
    try {
      const response = await fetch(apiUrl('/api/auth/altcha-config'))
      const data = await response.json()
      setAltchaEnabled(data.enabled)
      setAltchaChallengeUrl(data.challenge_url || null)
    } catch (error) {
      console.error('Failed to fetch Altcha config:', error)
    }
  }

  const login = async (username, password, altcha = null) => {
    const response = await fetch(apiUrl('/api/auth/login'), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
      body: JSON.stringify({ username, password, altcha }),
    })

    const data = await response.json()

    if (response.ok && data.success) {
      setUser({ username: data.username })
      return { success: true }
    } else {
      return { success: false, message: data.message || 'Login failed' }
    }
  }

  const logout = async () => {
    try {
      await fetch(apiUrl('/api/auth/logout'), {
        method: 'POST',
        credentials: 'include',
      })
    } catch (error) {
      console.error('Logout error:', error)
    } finally {
      setUser(null)
    }
  }

  return (
    <AuthContext.Provider value={{ user, loading, login, logout, altchaEnabled, altchaChallengeUrl }}>
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider')
  }
  return context
}
