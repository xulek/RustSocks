#!/bin/bash
# RustSocks Load Testing Runner
#
# This script runs comprehensive load tests for the RustSocks proxy server
# including SOCKS5 proxy tests and REST API tests.
#
# Usage:
#   ./loadtests/run_loadtests.sh [--all|--socks|--api] [--quick]
#
# Options:
#   --all     Run all tests (default)
#   --socks   Run only SOCKS5 proxy tests
#   --api     Run only API tests
#   --quick   Run quick version (reduced duration and connections)

set -e

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
PROXY_PORT=1080
API_PORT=9090
ECHO_PORT=9999
PROXY_ADDR="127.0.0.1:${PROXY_PORT}"
API_ADDR="127.0.0.1:${API_PORT}"
ECHO_ADDR="127.0.0.1:${ECHO_PORT}"
RESULTS_DIR="loadtests/results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Test mode
MODE="all"
QUICK=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            MODE="all"
            shift
            ;;
        --socks)
            MODE="socks"
            shift
            ;;
        --api)
            MODE="api"
            shift
            ;;
        --quick)
            QUICK=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--all|--socks|--api] [--quick]"
            exit 1
            ;;
    esac
done

# Create results directory
mkdir -p "${RESULTS_DIR}"

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║         RustSocks Load Testing Suite                         ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${YELLOW}Configuration:${NC}"
echo "  Proxy Address:    ${PROXY_ADDR}"
echo "  API Address:      ${API_ADDR}"
echo "  Echo Server:      ${ECHO_ADDR}"
echo "  Test Mode:        ${MODE}"
echo "  Quick Mode:       ${QUICK}"
echo "  Results Dir:      ${RESULTS_DIR}"
echo "  Timestamp:        ${TIMESTAMP}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust.${NC}"
    exit 1
fi

# Check if k6 is available (only warn, not required)
K6_AVAILABLE=false
if command -v k6 &> /dev/null; then
    K6_AVAILABLE=true
    echo -e "${GREEN}✓${NC} k6 found"
else
    echo -e "${YELLOW}⚠${NC}  k6 not found (API load tests will be skipped)"
    echo "   Install k6: https://k6.io/docs/get-started/installation/"
fi

# Check auth configuration
echo ""
if [ -f "config/rustsocks.toml" ]; then
    AUTH_METHOD=$(grep -A5 "^\[auth\]" config/rustsocks.toml | grep "socks_method" | sed 's/.*=\s*"\([^"]*\)".*/\1/')
    if [ "${AUTH_METHOD}" != "none" ]; then
        echo -e "${YELLOW}⚠  Warning: auth.socks_method is set to '${AUTH_METHOD}' in config/rustsocks.toml${NC}"
        echo "   Load tests work best with auth.socks_method = \"none\""
        echo "   To test with authentication, use --username and --password flags"
        echo ""
    else
        echo -e "${GREEN}✓${NC} Authentication: none (optimal for load testing)"
    fi
else
    echo -e "${YELLOW}⚠  Warning: config/rustsocks.toml not found${NC}"
fi

# Build release binaries
echo ""
echo -e "${BLUE}🔨 Building release binaries...${NC}"
cargo build --release --example loadtest --example echo_server
echo -e "${GREEN}✓${NC} Build complete"

# Function to check if a process is running on a port
check_port() {
    local port=$1
    if lsof -Pi :${port} -sTCP:LISTEN -t >/dev/null 2>&1 || \
       netstat -tuln 2>/dev/null | grep -q ":${port} " || \
       ss -tuln 2>/dev/null | grep -q ":${port} "; then
        return 0
    else
        return 1
    fi
}

# Function to wait for service to be ready
wait_for_service() {
    local host=$1
    local port=$2
    local max_wait=$3
    local elapsed=0

    echo -n "   Waiting for service on ${host}:${port}..."
    while ! check_port ${port}; do
        sleep 1
        elapsed=$((elapsed + 1))
        if [ ${elapsed} -ge ${max_wait} ]; then
            echo -e " ${RED}✗${NC} Timeout"
            return 1
        fi
    done
    echo -e " ${GREEN}✓${NC}"
    return 0
}

# Start echo server
echo ""
echo -e "${BLUE}🔊 Starting echo server...${NC}"
./target/release/examples/echo_server --port ${ECHO_PORT} > "${RESULTS_DIR}/echo_server_${TIMESTAMP}.log" 2>&1 &
ECHO_PID=$!
echo "   PID: ${ECHO_PID}"

if ! wait_for_service "127.0.0.1" ${ECHO_PORT} 10; then
    echo -e "${RED}❌ Failed to start echo server${NC}"
    kill ${ECHO_PID} 2>/dev/null || true
    exit 1
fi

# Start RustSocks proxy
echo ""
echo -e "${BLUE}🚀 Starting RustSocks proxy...${NC}"
./target/release/rustsocks \
    --config config/rustsocks.toml \
    --bind 127.0.0.1 \
    --port ${PROXY_PORT} \
    > "${RESULTS_DIR}/rustsocks_${TIMESTAMP}.log" 2>&1 &
PROXY_PID=$!
echo "   PID: ${PROXY_PID}"

if ! wait_for_service "127.0.0.1" ${PROXY_PORT} 30; then
    echo -e "${RED}❌ Failed to start RustSocks proxy${NC}"
    kill ${ECHO_PID} ${PROXY_PID} 2>/dev/null || true
    exit 1
fi

# Wait for API to be ready (if enabled in config)
sleep 2
if check_port ${API_PORT}; then
    echo -e "${GREEN}✓${NC} API server is running on port ${API_PORT}"
else
    echo -e "${YELLOW}⚠${NC}  API server not detected (may not be enabled in config)"
fi

# Cleanup function
CLEANUP_DONE=false
cleanup() {
    # Only cleanup once
    if [ "${CLEANUP_DONE}" = true ]; then
        return
    fi
    CLEANUP_DONE=true

    echo ""
    echo -e "${YELLOW}🧹 Cleaning up...${NC}"

    if [ -n "${PROXY_PID}" ]; then
        echo "   Stopping RustSocks proxy (PID: ${PROXY_PID})"
        kill ${PROXY_PID} 2>/dev/null || true
    fi

    if [ -n "${ECHO_PID}" ]; then
        echo "   Stopping echo server (PID: ${ECHO_PID})"
        kill ${ECHO_PID} 2>/dev/null || true
    fi

    # Kill any remaining processes
    pkill -f "echo_server" 2>/dev/null || true
    pkill -f "loadtest" 2>/dev/null || true

    echo -e "${GREEN}✓${NC} Cleanup complete"
}

# Set trap to cleanup on exit
trap cleanup EXIT INT TERM

# Function to run SOCKS5 load tests
run_socks_tests() {
    echo ""
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║           SOCKS5 Proxy Load Tests                             ║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"

    local duration=30
    if [ "${QUICK}" = true ]; then
        duration=10
    fi

    # Test 1: 1000 Concurrent Connections
    echo ""
    echo -e "${BLUE}📊 Test 1: 1000 Concurrent Connections${NC}"
    ./target/release/examples/loadtest \
        --scenario concurrent-1000 \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        2>&1 | tee "${RESULTS_DIR}/concurrent_1000_${TIMESTAMP}.log"

    sleep 5

    # Test 2: ACL Performance
    echo ""
    echo -e "${BLUE}📊 Test 2: ACL Performance Test${NC}"
    ./target/release/examples/loadtest \
        --scenario acl-perf \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${RESULTS_DIR}/acl_perf_${TIMESTAMP}.log"

    sleep 5

    # Test 3: Session Tracking Overhead
    echo ""
    echo -e "${BLUE}📊 Test 3: Session Tracking Overhead${NC}"
    ./target/release/examples/loadtest \
        --scenario session-overhead \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${RESULTS_DIR}/session_overhead_${TIMESTAMP}.log"

    sleep 5

    # Test 4: Database Write Throughput
    echo ""
    echo -e "${BLUE}📊 Test 4: Database Write Throughput${NC}"
    ./target/release/examples/loadtest \
        --scenario db-throughput \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${RESULTS_DIR}/db_throughput_${TIMESTAMP}.log"

    # Test 5: 5000 Concurrent Connections (only in full mode)
    if [ "${QUICK}" = false ]; then
        sleep 5
        echo ""
        echo -e "${BLUE}📊 Test 5: 5000 Concurrent Connections${NC}"
        ./target/release/examples/loadtest \
            --scenario concurrent-5000 \
            --proxy ${PROXY_ADDR} \
            --upstream ${ECHO_ADDR} \
            2>&1 | tee "${RESULTS_DIR}/concurrent_5000_${TIMESTAMP}.log"
    fi
}

# Function to run API load tests
run_api_tests() {
    if [ "${K6_AVAILABLE}" = false ]; then
        echo ""
        echo -e "${YELLOW}⚠  Skipping API tests: k6 not installed${NC}"
        return
    fi

    if ! check_port ${API_PORT}; then
        echo ""
        echo -e "${YELLOW}⚠  Skipping API tests: API server not running${NC}"
        return
    fi

    echo ""
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║           REST API Load Tests                                 ║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"

    echo ""
    echo -e "${BLUE}📊 Running k6 API Load Test${NC}"

    API_URL="http://${API_ADDR}" k6 run \
        --out json="${RESULTS_DIR}/k6_api_${TIMESTAMP}.json" \
        loadtests/k6/api_load_test.js \
        2>&1 | tee "${RESULTS_DIR}/k6_api_${TIMESTAMP}.log"
}

# Run tests based on mode
if [ "${MODE}" = "all" ] || [ "${MODE}" = "socks" ]; then
    run_socks_tests
fi

if [ "${MODE}" = "all" ] || [ "${MODE}" = "api" ]; then
    run_api_tests
fi

# Generate summary report
echo ""
echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           Load Test Summary                                   ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${GREEN}✅ All tests completed successfully!${NC}"
echo ""
echo "Results saved to: ${RESULTS_DIR}"
echo ""
echo "Log files:"
ls -lh "${RESULTS_DIR}"/*_${TIMESTAMP}* 2>/dev/null || echo "  No log files found"
echo ""

# Extract key metrics from logs
echo -e "${BLUE}Key Performance Metrics:${NC}"
echo ""

if [ -f "${RESULTS_DIR}/concurrent_1000_${TIMESTAMP}.log" ]; then
    echo "1000 Concurrent Connections:"
    grep -E "Success|Throughput|Average:" "${RESULTS_DIR}/concurrent_1000_${TIMESTAMP}.log" | head -5 || true
    echo ""
fi

if [ -f "${RESULTS_DIR}/acl_perf_${TIMESTAMP}.log" ]; then
    echo "ACL Performance:"
    grep -E "Throughput|Average:" "${RESULTS_DIR}/acl_perf_${TIMESTAMP}.log" | head -3 || true
    echo ""
fi

if [ -f "${RESULTS_DIR}/session_overhead_${TIMESTAMP}.log" ]; then
    echo "Session Tracking:"
    grep -E "Throughput|Average:" "${RESULTS_DIR}/session_overhead_${TIMESTAMP}.log" | head -3 || true
    echo ""
fi

echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
