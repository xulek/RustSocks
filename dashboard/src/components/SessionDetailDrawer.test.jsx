import React from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import SessionDetailDrawer from './SessionDetailDrawer'

const mockSession = {
  id: 'session-1',
  user: 'alice',
  status: 'active',
  acl_decision: 'allow',
  acl_rule: 'allow-web',
  source_ip: '10.0.0.1',
  source_port: 1080,
  dest_ip: '93.184.216.34',
  dest_port: 443,
  protocol: 'tcp',
  start_time: '2024-01-01T10:00:00Z',
  end_time: '2024-01-01T10:05:00Z',
  duration_seconds: 300,
  bytes_sent: 2048,
  bytes_received: 4096
}

describe('SessionDetailDrawer', () => {
  it('does not render when closed', () => {
    const { container } = render(
      <SessionDetailDrawer open={false} session={mockSession} loading={false} error={null} onClose={() => {}} />
    )
    expect(container).toBeEmptyDOMElement()
  })

  it('renders session information when open', () => {
    render(
      <SessionDetailDrawer open session={mockSession} loading={false} error={null} onClose={() => {}} />
    )

    expect(screen.getByText(/Session ID/i)).toBeInTheDocument()
    expect(screen.getByText(/alice/i)).toBeInTheDocument()
    expect(screen.getByText(/allow-web/i)).toBeInTheDocument()
  })

  it('triggers close handler', async () => {
    const onClose = vi.fn()
    const user = userEvent.setup()

    render(
      <SessionDetailDrawer open session={mockSession} loading={false} error={null} onClose={onClose} />
    )

    const closeButtons = screen.getAllByTitle('Zamknij panel')
    await user.click(closeButtons[closeButtons.length - 1])
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
