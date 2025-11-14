import React, { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { Shield, Lock, User, AlertCircle } from 'lucide-react'
import { useAuth } from '../contexts/AuthContext'
import { ROUTER_BASENAME } from '../lib/basePath'

function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [altchaValue, setAltchaValue] = useState(null)
  const widgetRef = useRef(null)

  const { login, user, altchaEnabled, altchaChallengeUrl } = useAuth()
  const navigate = useNavigate()

  // Use configured challenge URL or fallback to built-in endpoint
  const challengeUrl = altchaChallengeUrl || `${ROUTER_BASENAME}/api/auth/altcha-challenge`

  useEffect(() => {
    if (user) {
      navigate('/')
    }
  }, [user, navigate])

  useEffect(() => {
    // Dynamically load Altcha web component
    if (altchaEnabled && !customElements.get('altcha-widget')) {
      import('altcha').then((module) => {
        // Altcha registers itself as a custom element
      }).catch(err => {
        console.error('Failed to load Altcha:', err)
      })
    }
  }, [altchaEnabled])

  useEffect(() => {
    if (altchaEnabled) {
      const handleStateChange = (ev) => {
        if (ev.detail.state === 'verified') {
          setAltchaValue(ev.detail.payload)
        } else if (ev.detail.state === 'error') {
          console.error('Altcha error:', ev.detail.error)
          setAltchaValue(null)
        } else {
          setAltchaValue(null)
        }
      }

      let attempts = 0
      const maxAttempts = 20
      const interval = setInterval(() => {
        const widget = document.querySelector('altcha-widget')
        if (widget) {
          widgetRef.current = widget
          widget.addEventListener('statechange', handleStateChange)
          clearInterval(interval)
        } else {
          attempts++
          if (attempts >= maxAttempts) {
            clearInterval(interval)
          }
        }
      }, 100)

      return () => {
        clearInterval(interval)
        if (widgetRef.current) {
          widgetRef.current.removeEventListener('statechange', handleStateChange)
        }
      }
    }
  }, [altchaEnabled])

  const handleSubmit = async (e) => {
    e.preventDefault()
    setError('')
    setLoading(true)

    try {
      const result = await login(username, password, altchaValue)

      if (result.success) {
        navigate('/')
      } else {
        setError(result.message || 'Invalid username or password')
        // Reset Altcha if enabled
        if (altchaEnabled) {
          setAltchaValue(null)
          const altchaWidget = document.querySelector('altcha-widget')
          if (altchaWidget) {
            altchaWidget.reset()
          }
        }
      }
    } catch (err) {
      setError('An error occurred during login. Please try again.')
      console.error('Login error:', err)
    } finally {
      setLoading(false)
    }
  }

  const isFormValid = username && password && (!altchaEnabled || altchaValue)

  return (
    <div className="login-container">
      <div className="login-box">
        <div className="login-header">
          <div className="login-icon">
            <Shield size={48} />
          </div>
          <h1>RustSocks</h1>
          <p>Admin Dashboard</p>
        </div>

        <form onSubmit={handleSubmit} className="login-form">
          {error && (
            <div className="login-error">
              <AlertCircle size={20} />
              <span>{error}</span>
            </div>
          )}

          <div className="form-group">
            <label htmlFor="username">
              <User size={18} />
              <span>Username</span>
            </label>
            <input
              id="username"
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="Enter your username"
              disabled={loading}
              autoComplete="username"
              autoFocus
            />
          </div>

          <div className="form-group">
            <label htmlFor="password">
              <Lock size={18} />
              <span>Password</span>
            </label>
            <input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter your password"
              disabled={loading}
              autoComplete="current-password"
            />
          </div>

          {altchaEnabled && (
            <div className="form-group altcha-container">
              <altcha-widget
                challengeurl={challengeUrl}
                hidefooter="true"
              />
            </div>
          )}

          <button
            type="submit"
            className="btn btn-primary login-button"
            disabled={loading || !isFormValid}
          >
            {loading ? 'Signing in...' : 'Sign In'}
          </button>
        </form>

        <div className="login-footer">
          <p>RustSocks v0.9.0</p>
        </div>
      </div>
    </div>
  )
}

export default Login
