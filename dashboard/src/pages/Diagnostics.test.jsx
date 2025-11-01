import React from 'react'
import { describe, it, beforeEach, afterEach, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import Diagnostics from './Diagnostics'

describe('Diagnostics page', () => {
  beforeEach(() => {
    global.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: () =>
        Promise.resolve({
          address: '127.0.0.1',
          port: 3000,
          success: true,
          latency_ms: 12,
          message: 'ok',
          error: null
        })
    })
  })

  afterEach(() => {
    vi.resetAllMocks()
  })

  it('accepts 3000 ms timeout', async () => {
    render(<Diagnostics />)

    fireEvent.change(screen.getByLabelText(/Destination IP address/i), {
      target: { value: '127.0.0.1' }
    })

    fireEvent.change(screen.getByLabelText(/Port/i), {
      target: { value: '1080' }
    })

    fireEvent.change(screen.getByLabelText(/Timeout \(ms\)/i), {
      target: { value: '3000' }
    })

    fireEvent.click(screen.getByRole('button', { name: /test connection/i }))

    await waitFor(() => {
      expect(global.fetch).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({
            address: '127.0.0.1',
            port: 1080,
            timeout_ms: 3000
          })
        })
      )
    })
  })
})
