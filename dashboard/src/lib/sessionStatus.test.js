import { describe, expect, it } from 'vitest'
import {
  getSessionStatusBadgeClass,
  getSessionStatusLabel,
  SESSION_STATUS_OPTIONS
} from './sessionStatus'

describe('sessionStatus helpers', () => {
  it('returns readable label for admin-terminated sessions', () => {
    expect(getSessionStatusLabel('terminated_by_admin')).toBe('Terminated by Admin')
  })

  it('returns warning badge for admin-terminated sessions', () => {
    expect(getSessionStatusBadgeClass('terminated_by_admin')).toBe('badge badge-warning')
  })

  it('contains admin-terminated status in filter options', () => {
    const optionValues = SESSION_STATUS_OPTIONS.map((option) => option.value)
    expect(optionValues).toContain('terminated_by_admin')
  })
})
