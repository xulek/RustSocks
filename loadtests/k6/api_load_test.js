// k6 Load Test for RustSocks REST API
//
// This script tests the RustSocks REST API endpoints under load
//
// Usage:
//   k6 run --vus 10 --duration 30s loadtests/k6/api_load_test.js
//
// Options:
//   --vus: Number of virtual users (concurrent requests)
//   --duration: Test duration (e.g., 30s, 5m, 1h)

import http from 'k6/http';
import { check, group, sleep } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const apiLatency = new Trend('api_latency');
const requestCount = new Counter('requests');

// Configuration
const BASE_URL = __ENV.API_URL || 'http://127.0.0.1:9090';

// Test options
export const options = {
    stages: [
        { duration: '10s', target: 10 },  // Ramp up to 10 users
        { duration: '30s', target: 50 },  // Ramp up to 50 users
        { duration: '20s', target: 100 }, // Spike to 100 users
        { duration: '30s', target: 50 },  // Scale down to 50 users
        { duration: '10s', target: 0 },   // Ramp down to 0 users
    ],
    thresholds: {
        'http_req_duration': ['p(95)<500', 'p(99)<1000'], // 95% < 500ms, 99% < 1s
        'http_req_failed': ['rate<0.05'],  // Less than 5% errors
        'errors': ['rate<0.05'],           // Less than 5% check failures
    },
};

export default function () {
    let response;
    let success;

    // Test 1: Health Check
    group('Health Check', () => {
        response = http.get(`${BASE_URL}/health`);
        success = check(response, {
            'health status is 200': (r) => r.status === 200,
            'health response time < 100ms': (r) => r.timings.duration < 100,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(0.5);

    // Test 2: Get Active Sessions
    group('Active Sessions', () => {
        response = http.get(`${BASE_URL}/api/sessions/active`);
        success = check(response, {
            'active sessions status is 200': (r) => r.status === 200,
            'active sessions has data field': (r) => {
                try {
                    const json = JSON.parse(r.body);
                    return json.hasOwnProperty('data');
                } catch (e) {
                    return false;
                }
            },
            'active sessions response time < 200ms': (r) => r.timings.duration < 200,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(0.5);

    // Test 3: Get Session History
    group('Session History', () => {
        const params = {
            'hours': '24',
            'page': '1',
            'page_size': '50',
        };
        const url = `${BASE_URL}/api/sessions/history?${Object.keys(params)
            .map(key => `${key}=${params[key]}`)
            .join('&')}`;

        response = http.get(url);
        success = check(response, {
            'history status is 200': (r) => r.status === 200,
            'history has sessions array': (r) => {
                try {
                    const json = JSON.parse(r.body);
                    return Array.isArray(json.sessions);
                } catch (e) {
                    return false;
                }
            },
            'history response time < 300ms': (r) => r.timings.duration < 300,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(0.5);

    // Test 4: Get Stats
    group('Statistics', () => {
        response = http.get(`${BASE_URL}/api/sessions/stats?window_hours=24`);
        success = check(response, {
            'stats status is 200': (r) => r.status === 200,
            'stats has required fields': (r) => {
                try {
                    const json = JSON.parse(r.body);
                    return json.hasOwnProperty('active_sessions') &&
                           json.hasOwnProperty('total_sessions') &&
                           json.hasOwnProperty('total_bytes_sent');
                } catch (e) {
                    return false;
                }
            },
            'stats response time < 200ms': (r) => r.timings.duration < 200,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(0.5);

    // Test 5: Prometheus Metrics
    group('Prometheus Metrics', () => {
        response = http.get(`${BASE_URL}/metrics`);
        success = check(response, {
            'metrics status is 200': (r) => r.status === 200,
            'metrics has rustsocks_ prefix': (r) => r.body.includes('rustsocks_'),
            'metrics response time < 100ms': (r) => r.timings.duration < 100,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(0.5);

    // Test 6: ACL Rules Summary
    group('ACL Rules', () => {
        response = http.get(`${BASE_URL}/api/acl/rules`);
        success = check(response, {
            'acl rules status is 200 or 404': (r) => r.status === 200 || r.status === 404,
            'acl rules response time < 100ms': (r) => r.timings.duration < 100,
        });
        errorRate.add(!success);
        apiLatency.add(response.timings.duration);
        requestCount.add(1);
    });

    sleep(1);
}

export function handleSummary(data) {
    return {
        'stdout': textSummary(data, { indent: ' ', enableColors: true }),
        'loadtests/results/api_load_test.json': JSON.stringify(data, null, 2),
    };
}

function textSummary(data, options) {
    const indent = options?.indent || '';
    const enableColors = options?.enableColors || false;

    let output = '\n';
    output += `${indent}📊 k6 API Load Test Summary\n`;
    output += `${indent}${'='.repeat(60)}\n\n`;

    // Request statistics
    const requests = data.metrics.http_reqs?.values;
    if (requests) {
        output += `${indent}📈 Requests:\n`;
        output += `${indent}  Total:     ${requests.count}\n`;
        output += `${indent}  Rate:      ${requests.rate.toFixed(2)} req/s\n`;
    }

    // Duration statistics
    const duration = data.metrics.http_req_duration?.values;
    if (duration) {
        output += `\n${indent}⏱️  Response Time:\n`;
        output += `${indent}  Avg:       ${duration.avg.toFixed(2)} ms\n`;
        output += `${indent}  Min:       ${duration.min.toFixed(2)} ms\n`;
        output += `${indent}  Max:       ${duration.max.toFixed(2)} ms\n`;
        output += `${indent}  p(50):     ${duration.med.toFixed(2)} ms\n`;
        output += `${indent}  p(90):     ${duration['p(90)'].toFixed(2)} ms\n`;
        output += `${indent}  p(95):     ${duration['p(95)'].toFixed(2)} ms\n`;
        output += `${indent}  p(99):     ${duration['p(99)'].toFixed(2)} ms\n`;
    }

    // Error rate
    const failed = data.metrics.http_req_failed?.values;
    if (failed) {
        const errorPercent = (failed.rate * 100).toFixed(2);
        output += `\n${indent}❌ Errors:\n`;
        output += `${indent}  Failed:    ${failed.fails} (${errorPercent}%)\n`;
    }

    // Custom metrics
    const customErrorRate = data.metrics.errors?.values;
    if (customErrorRate) {
        output += `${indent}  Check Failures: ${(customErrorRate.rate * 100).toFixed(2)}%\n`;
    }

    output += `\n${indent}${'='.repeat(60)}\n`;
    return output;
}
