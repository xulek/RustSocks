import { describe, expect, it } from 'vitest'
import { formatBytes, formatDateTime, formatDuration } from './format'

describe('format helpers', () => {
  it('formats bytes with appropriate unit', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(2048)).toBe('2 KB')
    expect(formatBytes(5 * 1024 * 1024)).toBe('5 MB')
    expect(formatBytes(null)).toBe('—')
  })

  it('formats dates and handles empty input', () => {
    expect(formatDateTime(null)).toBe('—')
    expect(formatDateTime('2024-01-01T00:00:00Z')).toContain('2024')
  })

  it('formats duration into readable chunks', () => {
    expect(formatDuration(45)).toBe('45s')
    expect(formatDuration(75)).toBe('1m 15s')
    expect(formatDuration(3665)).toBe('1h 1m')
    expect(formatDuration(null)).toBe('—')
  })
})
