import { describe, expect, it } from 'vitest'
import {
  DEFAULT_PAGE_SIZE,
  buildHistoryQuery,
  buildHistoryUrl,
  sessionsToCsv
} from './sessions'

describe('session helpers', () => {
  it('builds query string with filters and defaults', () => {
    const query = buildHistoryQuery({
      user: ' alice ',
      destination: '10.0.0.1',
      status: 'closed',
      hours: '24',
      page: 2,
      pageSize: 100
    })

    expect(query).toBe('?page=2&page_size=100&user=alice&dest_ip=10.0.0.1&status=closed&hours=24')
  })

  it('clamps page and page size to accepted ranges', () => {
    const query = buildHistoryQuery({
      page: 0,
      pageSize: 10_000
    })

    expect(query).toBe(`?page=1&page_size=1000`)
  })

  it('builds full history url', () => {
    const url = buildHistoryUrl('/api/sessions/history', { page: 3, pageSize: DEFAULT_PAGE_SIZE })
    expect(url).toBe(`/api/sessions/history?page=3&page_size=${DEFAULT_PAGE_SIZE}`)
  })

  it('converts sessions list to csv', () => {
    const csv = sessionsToCsv([
      {
        id: 'abc123',
        user: 'admin',
        status: 'active',
        source_ip: '10.0.0.5',
        source_port: 5555,
        dest_ip: '8.8.8.8',
        dest_port: 53,
        protocol: 'udp',
        bytes_sent: 512,
        bytes_received: 1024,
        duration_seconds: 30,
        acl_decision: 'allow',
        acl_rule: 'allow-dns',
        start_time: '2024-01-01T00:00:00Z',
        end_time: null
      }
    ])

    expect(csv.split('\n')[0]).toBe('session_id,user,status,source,destination,protocol,bytes_sent,bytes_received,duration,acl_decision,acl_rule,start_time,end_time')
    expect(csv).toContain('abc123')
    expect(csv).toContain('admin')
    expect(csv).toContain('allow-dns')
    expect(csv).toContain('allow')
  })
})
