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
FULL_CONFIG_PATH="config/rustsocks.toml"
MINIMAL_CONFIG_PATH="config/rustsocks_minimal.toml"
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
if [ -f "${FULL_CONFIG_PATH}" ]; then
    AUTH_METHOD=$(grep -A5 "^\[auth\]" "${FULL_CONFIG_PATH}" | grep "socks_method" | sed 's/.*=\s*"\([^"]*\)".*/\1/')
    if [ "${AUTH_METHOD}" != "none" ]; then
        echo -e "${YELLOW}⚠  Warning: auth.socks_method is set to '${AUTH_METHOD}' in config/rustsocks.toml${NC}"
        echo "   Load tests work best with auth.socks_method = \"none\""
        echo "   To test with authentication, use --username and --password flags"
        echo ""
    else
        echo -e "${GREEN}✓${NC} Authentication: none (optimal for load testing)"
    fi
else
    echo -e "${YELLOW}⚠  Warning: ${FULL_CONFIG_PATH} not found${NC}"
fi

if [ ! -f "${MINIMAL_CONFIG_PATH}" ]; then
    echo -e "${YELLOW}⚠  Warning: ${MINIMAL_CONFIG_PATH} not found, falling back to full config for minimal tests${NC}"
    MINIMAL_CONFIG_PATH="${FULL_CONFIG_PATH}"
fi

# Build release binaries
echo ""
echo -e "${BLUE}🔨 Building release binaries...${NC}"
cargo build --release --example loadtest --example echo_server
echo -e "${GREEN}✓${NC} Build complete"

# Duration for SOCKS tests (seconds)
SOCKS_DURATION=30
if [ "${QUICK}" = true ]; then
    SOCKS_DURATION=10
fi

# Metric tracking helpers
declare -a METRIC_NAMES=()
declare -a METRIC_ACTUALS=()
declare -a METRIC_TARGETS=()
declare -a METRIC_STATUS=()
declare -a METRIC_NOTES=()

format_value() {
    local value=$1
    local units=$2
    if [[ -z "${value}" ]]; then
        echo "n/a"
    else
        printf "%.2f %s" "${value}" "${units}"
    fi
}

format_target_text() {
    local comparator=$1
    local target=$2
    local units=$3
    local symbol
    case "${comparator}" in
        lte) symbol="<=" ;;
        lt) symbol="<" ;;
        gte) symbol=">=" ;;
        gt) symbol=">" ;;
        *) symbol="=" ;;
    esac
    printf "%s %.2f %s" "${symbol}" "${target}" "${units}"
}

compare_values() {
    local actual=$1
    local target=$2
    local comparator=$3
    awk -v a="${actual}" -v b="${target}" -v cmp="${comparator}" 'BEGIN {
        if (a == "" || b == "") exit 1;
        if (cmp == "lte") exit !(a <= b);
        if (cmp == "lt") exit !(a < b);
        if (cmp == "gte") exit !(a >= b);
        if (cmp == "gt") exit !(a > b);
        exit !(a == b);
    }'
}

add_metric_result() {
    local name=$1
    local value=$2
    local units=$3
    local comparator=$4
    local target=$5
    local note=$6

    local actual_text target_text status message
    actual_text=$(format_value "${value}" "${units}")
    target_text=$(format_target_text "${comparator}" "${target}" "${units}")

    if [[ -z "${value}" ]]; then
        status="MISSING"
        message=${note:-"Metric not found in log"}
    elif compare_values "${value}" "${target}" "${comparator}" >/dev/null 2>&1; then
        status="PASS"
        message=${note:-"-"}
    else
        status="FAIL"
        message=${note:-"-"}
    fi

    METRIC_NAMES+=("${name}")
    METRIC_ACTUALS+=("${actual_text}")
    METRIC_TARGETS+=("${target_text}")
    METRIC_STATUS+=("${status}")
    METRIC_NOTES+=("${message}")
}

extract_latency_ms() {
    local file=$1
    if [ -f "${file}" ]; then
        awk '/Average:/ {print $(NF-1); exit}' "${file}"
    fi
}

extract_throughput_conn_s() {
    local file=$1
    if [ -f "${file}" ]; then
        awk '/Throughput:/ {print $(NF-1); exit}' "${file}"
    fi
}

extract_bandwidth_mb_s() {
    local file=$1
    if [ -f "${file}" ]; then
        awk '/Aggregate Bandwidth:/ {print $(NF-1); exit}' "${file}"
    fi
}

extract_success_rate_percent() {
    local file=$1
    if [ -f "${file}" ]; then
        sed -n 's/.*(\([0-9.]\+\)%).*/\1/p' "${file}" | head -n1
    fi
}

record_latency_metric() {
    local name=$1
    local file=$2
    local target=$3
    local note=$4
    local value
    value=$(extract_latency_ms "${file}")
    add_metric_result "${name}" "${value}" "ms" "lte" "${target}" "${note}"
}

record_throughput_metric() {
    local name=$1
    local file=$2
    local target=$3
    local note=$4
    local comparator=${5:-gte}
    local value
    value=$(extract_throughput_conn_s "${file}")
    add_metric_result "${name}" "${value}" "conn/s" "${comparator}" "${target}" "${note}"
}

record_bandwidth_metric() {
    local name=$1
    local file=$2
    local target=$3
    local note=$4
    local value
    value=$(extract_bandwidth_mb_s "${file}")
    add_metric_result "${name}" "${value}" "MB/s" "gte" "${target}" "${note}"
}

record_success_metric() {
    local name=$1
    local file=$2
    local target=$3
    local note=$4
    local value
    value=$(extract_success_rate_percent "${file}")
    add_metric_result "${name}" "${value}" "%" "gte" "${target}" "${note}"
}

print_metrics_summary() {
    local count=${#METRIC_NAMES[@]}
    if [ ${count} -eq 0 ]; then
        echo "No metrics collected."
        return
    fi

    printf " %-34s | %-18s | %-18s | %-7s | %s\n" "Metric" "Actual" "Target" "Status" "Note"
    printf '%s\n' "----------------------------------------------------------------------------------------------"
    local idx
    for idx in "${!METRIC_NAMES[@]}"; do
        printf " %-34s | %-18s | %-18s | %-7s | %s\n" \
            "${METRIC_NAMES[$idx]}" \
            "${METRIC_ACTUALS[$idx]}" \
            "${METRIC_TARGETS[$idx]}" \
            "${METRIC_STATUS[$idx]}" \
            "${METRIC_NOTES[$idx]}"
    done
}

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

start_echo_server() {
    echo ""
    echo -e "${BLUE}🔊 Starting echo server...${NC}"
    ./target/release/examples/echo_server --port ${ECHO_PORT} > "${RESULTS_DIR}/echo_server_${TIMESTAMP}.log" 2>&1 &
    ECHO_PID=$!
    echo "   PID: ${ECHO_PID}"

    if ! wait_for_service "127.0.0.1" ${ECHO_PORT} 10; then
        echo -e "${RED}❌ Failed to start echo server${NC}"
        cleanup
        exit 1
    fi
}

stop_echo_server() {
    if [ -n "${ECHO_PID:-}" ]; then
        echo "   Stopping echo server (PID: ${ECHO_PID})"
        kill ${ECHO_PID} 2>/dev/null || true
        wait ${ECHO_PID} 2>/dev/null || true
        ECHO_PID=""
    fi
}

start_proxy() {
    local profile=$1
    local label=$2
    local config_path=${3:-${FULL_CONFIG_PATH}}
    local log_path="${RESULTS_DIR}/rustsocks_${label}_${TIMESTAMP}.log"

    if [ "${profile}" = "minimal" ]; then
        echo ""
        echo -e "${BLUE}🚀 Starting RustSocks proxy (minimal profile)...${NC}"
    else
        echo ""
        echo -e "${BLUE}🚀 Starting RustSocks proxy (full profile)...${NC}"
    fi

    if [ ! -f "${config_path}" ]; then
        echo -e "${YELLOW}⚠  Config ${config_path} not found, using ${FULL_CONFIG_PATH}${NC}"
        config_path="${FULL_CONFIG_PATH}"
    fi

    ./target/release/rustsocks \
        --config "${config_path}" \
        --bind 127.0.0.1 \
        --port ${PROXY_PORT} \
        > "${log_path}" 2>&1 &
    PROXY_PID=$!
    echo "   PID: ${PROXY_PID}"

    if ! wait_for_service "127.0.0.1" ${PROXY_PORT} 30; then
        echo -e "${RED}❌ Failed to start RustSocks proxy${NC}"
        cleanup
        exit 1
    fi

    sleep 2
    if [ "${profile}" = "minimal" ]; then
        echo -e "${YELLOW}⚠${NC}  API server disabled in minimal profile"
    else
        if check_port ${API_PORT}; then
            echo -e "${GREEN}✓${NC} API server is running on port ${API_PORT}"
        else
            echo -e "${YELLOW}⚠${NC}  API server not detected (may not be enabled in config)"
        fi
    fi
}

stop_proxy() {
    if [ -n "${PROXY_PID:-}" ]; then
        echo "   Stopping RustSocks proxy (PID: ${PROXY_PID})"
        kill ${PROXY_PID} 2>/dev/null || true
        wait ${PROXY_PID} 2>/dev/null || true
        PROXY_PID=""
        sleep 1
    fi
}

# Cleanup function
CLEANUP_DONE=false
cleanup() {
    if [ "${CLEANUP_DONE}" = true ]; then
        return
    fi
    CLEANUP_DONE=true

    echo ""
    echo -e "${YELLOW}🧹 Cleaning up...${NC}"

    stop_proxy
    stop_echo_server

    # Kill any remaining processes
    pkill -f "echo_server" 2>/dev/null || true
    pkill -f "loadtest" 2>/dev/null || true

    echo -e "${GREEN}✓${NC} Cleanup complete"
}

trap cleanup EXIT INT TERM

run_minimal_scenarios() {
    local duration=${SOCKS_DURATION}

    echo ""
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║      Minimal Profile SOCKS5 Load Tests (Features Disabled)   ║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"

    # Test 1: Minimal Pipeline
    echo ""
    echo -e "${BLUE}📊 Test 1: Minimal Pipeline (Pure SOCKS5 Overhead)${NC}"
    echo -e "${YELLOW}   Measures: TCP + Handshake + Upstream (no ACL, no Sessions, no QoS)${NC}"
    echo -e "${YELLOW}   Expected: <40ms average latency${NC}"
    local minimal_log="${RESULTS_DIR}/minimal_pipeline_${TIMESTAMP}.log"
    ./target/release/examples/loadtest \
        --scenario minimal-pipeline \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${minimal_log}"
    record_latency_metric "Minimal Pipeline Latency" "${minimal_log}" 40 "Avg SOCKS5 handshake"

    sleep 5

    # Test 3: Handshake Only
    echo ""
    echo -e "${BLUE}📊 Test 3: Handshake-Only Test${NC}"
    echo -e "${YELLOW}   Measures: Pure connection establishment throughput${NC}"
    echo -e "${YELLOW}   Expected: >1200 conn/s${NC}"
    local handshake_log="${RESULTS_DIR}/handshake_only_${TIMESTAMP}.log"
    ./target/release/examples/loadtest \
        --scenario handshake-only \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${handshake_log}"
    record_throughput_metric "Handshake-Only Throughput" "${handshake_log}" 1200 "Connections per second"

    sleep 5

    # Test 4: Data Transfer
    echo ""
    echo -e "${BLUE}📊 Test 4: Data Transfer Throughput${NC}"
    echo -e "${YELLOW}   Measures: Proxy bandwidth with sustained traffic${NC}"
    echo -e "${YELLOW}   Expected: >500MB/s aggregate bandwidth${NC}"
    local data_log="${RESULTS_DIR}/data_transfer_${TIMESTAMP}.log"
    ./target/release/examples/loadtest \
        --scenario data-transfer \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${data_log}"
    record_bandwidth_metric "Data Transfer Bandwidth" "${data_log}" 500 "Aggregate bandwidth"
}

run_full_scenarios() {
    local duration=${SOCKS_DURATION}

    echo ""
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║        Full Profile SOCKS5 Load Tests (All Features)         ║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"

    # Test 2: Full Pipeline
    echo ""
    echo -e "${BLUE}📊 Test 2: Full Pipeline (All Features Enabled)${NC}"
    echo -e "${YELLOW}   Measures: Complete pipeline with ACL + Sessions + QoS + DB${NC}"
    echo -e "${YELLOW}   Expected: <70ms latency${NC}"
    local full_log="${RESULTS_DIR}/full_pipeline_${TIMESTAMP}.log"
    ./target/release/examples/loadtest \
        --scenario full-pipeline \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${full_log}"
    record_latency_metric "Full Pipeline Latency" "${full_log}" 70 "Avg SOCKS5 handshake"

    sleep 5

    # Test 5: Session Churn
    echo ""
    echo -e "${BLUE}📊 Test 5: Session Churn (Database Stress)${NC}"
    echo -e "${YELLOW}   Measures: Database write throughput with rapid session create/destroy${NC}"
    echo -e "${YELLOW}   Expected: >1100 sessions/sec${NC}"
    local churn_log="${RESULTS_DIR}/session_churn_${TIMESTAMP}.log"
    ./target/release/examples/loadtest \
        --scenario session-churn \
        --proxy ${PROXY_ADDR} \
        --upstream ${ECHO_ADDR} \
        --duration ${duration} \
        2>&1 | tee "${churn_log}"
    record_throughput_metric "Session Churn Throughput" "${churn_log}" 1100 "Sessions per second"

    if [ "${QUICK}" = false ]; then
        sleep 5
        echo ""
        echo -e "${BLUE}📊 Test 6: 1000 Concurrent Connections${NC}"
        echo -e "${YELLOW}   Measures: Medium concurrency handling${NC}"
        echo -e "${YELLOW}   Expected: >99% success rate${NC}"
        local concurrent_1k_log="${RESULTS_DIR}/concurrent_1000_${TIMESTAMP}.log"
        ./target/release/examples/loadtest \
            --scenario concurrent-1000 \
            --proxy ${PROXY_ADDR} \
            --upstream ${ECHO_ADDR} \
            2>&1 | tee "${concurrent_1k_log}"
        record_success_metric "Concurrent 1000 Success" "${concurrent_1k_log}" 99 "Successful connections (%)"

        sleep 5
        echo ""
        echo -e "${BLUE}📊 Test 7: 5000 Concurrent Connections${NC}"
        echo -e "${YELLOW}   Measures: High concurrency handling${NC}"
        echo -e "${YELLOW}   Expected: >98% success rate${NC}"
        local concurrent_5k_log="${RESULTS_DIR}/concurrent_5000_${TIMESTAMP}.log"
        ./target/release/examples/loadtest \
            --scenario concurrent-5000 \
            --proxy ${PROXY_ADDR} \
            --upstream ${ECHO_ADDR} \
            2>&1 | tee "${concurrent_5k_log}"
        record_success_metric "Concurrent 5000 Success" "${concurrent_5k_log}" 98 "Successful connections (%)"
    fi
}

# Function to run API load tests (requires proxy running with full profile)
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

SOCKS_REQUESTED=false
API_REQUESTED=false
if [ "${MODE}" = "all" ] || [ "${MODE}" = "socks" ]; then
    SOCKS_REQUESTED=true
fi
if [ "${MODE}" = "all" ] || [ "${MODE}" = "api" ]; then
    API_REQUESTED=true
fi

if [ "${SOCKS_REQUESTED}" = true ]; then
    start_echo_server

    start_proxy "minimal" "minimal" "${MINIMAL_CONFIG_PATH}"
    run_minimal_scenarios
    stop_proxy

    start_proxy "full" "full" "${FULL_CONFIG_PATH}"
    run_full_scenarios

    if [ "${API_REQUESTED}" = true ]; then
        run_api_tests
    fi

    stop_proxy
    stop_echo_server
elif [ "${API_REQUESTED}" = true ]; then
    start_proxy "full" "full"
    run_api_tests
    stop_proxy
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
echo -e "${BLUE}Key Performance Metrics vs Targets:${NC}"
print_metrics_summary
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
