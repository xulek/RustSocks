#!/bin/bash
# Benchmark Regression Testing Script
#
# This script runs performance benchmarks and compares results against baseline
# to detect performance regressions.
#
# Usage:
#   ./loadtests/scripts/benchmark_regression.sh [--baseline|--compare]
#
# Options:
#   --baseline   Create new baseline from current results
#   --compare    Compare current results against baseline (default)

set -e

# Configuration
BASELINE_DIR="loadtests/baseline"
RESULTS_DIR="loadtests/results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

MODE="compare"

# Parse arguments
if [ "$1" = "--baseline" ]; then
    MODE="baseline"
elif [ "$1" = "--compare" ]; then
    MODE="compare"
fi

# Create directories
mkdir -p "${BASELINE_DIR}"
mkdir -p "${RESULTS_DIR}"

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║         Benchmark Regression Testing                          ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "Mode: ${MODE}"
echo ""

# Function to extract metric from log file
extract_metric() {
    local file=$1
    local pattern=$2
    local default=$3

    if [ ! -f "${file}" ]; then
        echo "${default}"
        return
    fi

    local value=$(grep -E "${pattern}" "${file}" | head -1 | sed -E 's/.*:[ ]*([0-9.]+).*/\1/')
    if [ -z "${value}" ]; then
        echo "${default}"
    else
        echo "${value}"
    fi
}

# Function to run benchmarks
run_benchmarks() {
    echo -e "${BLUE}🏃 Running benchmarks...${NC}"
    echo ""

    # Run quick load tests
    ./loadtests/run_loadtests.sh --socks --quick

    echo ""
    echo -e "${GREEN}✓${NC} Benchmarks complete"
}

# Function to create baseline
create_baseline() {
    echo -e "${BLUE}📊 Creating baseline from current results...${NC}"
    echo ""

    # Find latest result files
    local latest_1000=$(ls -t ${RESULTS_DIR}/concurrent_1000_*.log 2>/dev/null | head -1)
    local latest_acl=$(ls -t ${RESULTS_DIR}/acl_perf_*.log 2>/dev/null | head -1)
    local latest_session=$(ls -t ${RESULTS_DIR}/session_overhead_*.log 2>/dev/null | head -1)
    local latest_db=$(ls -t ${RESULTS_DIR}/db_throughput_*.log 2>/dev/null | head -1)

    if [ -z "${latest_1000}" ]; then
        echo -e "${RED}❌ No benchmark results found. Run benchmarks first.${NC}"
        exit 1
    fi

    # Extract metrics and save to baseline file
    {
        echo "# Benchmark Baseline"
        echo "# Created: $(date)"
        echo ""
        echo "[concurrent_1000]"
        echo "success_rate=$(extract_metric "${latest_1000}" "✅ Successful:.*\((.*%)\)" "0")"
        echo "throughput=$(extract_metric "${latest_1000}" "🔄 Throughput:" "0")"
        echo "avg_latency=$(extract_metric "${latest_1000}" "Average:" "0")"
        echo ""
        echo "[acl_performance]"
        echo "throughput=$(extract_metric "${latest_acl}" "🔄 Throughput:" "0")"
        echo "avg_latency=$(extract_metric "${latest_acl}" "Average:" "0")"
        echo ""
        echo "[session_overhead]"
        echo "throughput=$(extract_metric "${latest_session}" "🔄 Throughput:" "0")"
        echo "avg_latency=$(extract_metric "${latest_session}" "Average:" "0")"
        echo ""
        echo "[db_throughput]"
        echo "throughput=$(extract_metric "${latest_db}" "🔄 Throughput:" "0")"
        echo "avg_latency=$(extract_metric "${latest_db}" "Average:" "0")"
    } > "${BASELINE_DIR}/baseline.txt"

    # Copy log files
    cp "${latest_1000}" "${BASELINE_DIR}/concurrent_1000_baseline.log" 2>/dev/null || true
    cp "${latest_acl}" "${BASELINE_DIR}/acl_perf_baseline.log" 2>/dev/null || true
    cp "${latest_session}" "${BASELINE_DIR}/session_overhead_baseline.log" 2>/dev/null || true
    cp "${latest_db}" "${BASELINE_DIR}/db_throughput_baseline.log" 2>/dev/null || true

    echo -e "${GREEN}✓${NC} Baseline created: ${BASELINE_DIR}/baseline.txt"
    echo ""
    echo "Baseline Metrics:"
    cat "${BASELINE_DIR}/baseline.txt"
}

# Function to compare against baseline
compare_baseline() {
    if [ ! -f "${BASELINE_DIR}/baseline.txt" ]; then
        echo -e "${RED}❌ No baseline found. Create baseline first:${NC}"
        echo "   ./loadtests/scripts/benchmark_regression.sh --baseline"
        exit 1
    fi

    echo -e "${BLUE}📊 Comparing against baseline...${NC}"
    echo ""

    # Find latest result files
    local latest_1000=$(ls -t ${RESULTS_DIR}/concurrent_1000_*.log 2>/dev/null | head -1)
    local latest_acl=$(ls -t ${RESULTS_DIR}/acl_perf_*.log 2>/dev/null | head -1)
    local latest_session=$(ls -t ${RESULTS_DIR}/session_overhead_*.log 2>/dev/null | head -1)
    local latest_db=$(ls -t ${RESULTS_DIR}/db_throughput_*.log 2>/dev/null | head -1)

    if [ -z "${latest_1000}" ]; then
        echo -e "${RED}❌ No current results found. Run benchmarks first.${NC}"
        exit 1
    fi

    # Load baseline values
    source "${BASELINE_DIR}/baseline.txt" 2>/dev/null || true

    # Extract current metrics
    local current_1000_success=$(extract_metric "${latest_1000}" "✅ Successful:.*\((.*%)\)" "0" | sed 's/%//')
    local current_1000_throughput=$(extract_metric "${latest_1000}" "🔄 Throughput:" "0")
    local current_1000_latency=$(extract_metric "${latest_1000}" "Average:" "0")

    local current_acl_throughput=$(extract_metric "${latest_acl}" "🔄 Throughput:" "0")
    local current_acl_latency=$(extract_metric "${latest_acl}" "Average:" "0")

    local current_session_throughput=$(extract_metric "${latest_session}" "🔄 Throughput:" "0")
    local current_session_latency=$(extract_metric "${latest_session}" "Average:" "0")

    local current_db_throughput=$(extract_metric "${latest_db}" "🔄 Throughput:" "0")
    local current_db_latency=$(extract_metric "${latest_db}" "Average:" "0")

    # Regression threshold (10% degradation = regression)
    local threshold=0.90

    local regressions=0

    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}Benchmark Regression Report${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo ""

    # Compare 1000 concurrent connections
    echo "1000 Concurrent Connections:"
    echo "  Throughput:"
    printf "    Baseline: %.2f conn/s\n" ${throughput:-0}
    printf "    Current:  %.2f conn/s" ${current_1000_throughput}
    # Check regression
    if awk "BEGIN {exit !(${current_1000_throughput} < ${throughput:-0} * ${threshold})}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi

    echo "  Latency:"
    printf "    Baseline: %.2f ms\n" ${avg_latency:-0}
    printf "    Current:  %.2f ms" ${current_1000_latency}
    # Lower is better for latency, so check if increased
    if awk "BEGIN {exit !(${current_1000_latency} > ${avg_latency:-999} * 1.10)}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi
    echo ""

    # Compare ACL performance
    echo "ACL Performance:"
    echo "  Throughput:"
    printf "    Baseline: %.2f conn/s\n" ${throughput:-0}
    printf "    Current:  %.2f conn/s" ${current_acl_throughput}
    if awk "BEGIN {exit !(${current_acl_throughput} < ${throughput:-0} * ${threshold})}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi

    echo "  Latency:"
    printf "    Baseline: %.2f ms\n" ${avg_latency:-0}
    printf "    Current:  %.2f ms" ${current_acl_latency}
    if awk "BEGIN {exit !(${current_acl_latency} > ${avg_latency:-999} * 1.10)}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi
    echo ""

    # Compare session overhead
    echo "Session Tracking:"
    echo "  Throughput:"
    printf "    Baseline: %.2f conn/s\n" ${throughput:-0}
    printf "    Current:  %.2f conn/s" ${current_session_throughput}
    if awk "BEGIN {exit !(${current_session_throughput} < ${throughput:-0} * ${threshold})}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi
    echo ""

    # Compare DB throughput
    echo "Database Write Throughput:"
    echo "  Throughput:"
    printf "    Baseline: %.2f conn/s\n" ${throughput:-0}
    printf "    Current:  %.2f conn/s" ${current_db_throughput}
    if awk "BEGIN {exit !(${current_db_throughput} < ${throughput:-0} * ${threshold})}"; then
        echo -e " ${RED}[REGRESSION]${NC}"
        regressions=$((regressions + 1))
    else
        echo -e " ${GREEN}[OK]${NC}"
    fi
    echo ""

    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"

    if [ ${regressions} -eq 0 ]; then
        echo -e "${GREEN}✅ No regressions detected!${NC}"
        return 0
    else
        echo -e "${RED}❌ ${regressions} regression(s) detected!${NC}"
        return 1
    fi
}

# Main execution
if [ "${MODE}" = "baseline" ]; then
    run_benchmarks
    create_baseline
else
    run_benchmarks
    compare_baseline
fi
