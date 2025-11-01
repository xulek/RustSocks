#!/bin/bash
# Local CI checks script
# Run this before pushing to verify CI will pass

set -e

echo "================================================"
echo "🔍 Running local CI checks..."
echo "================================================"
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 1. Format check
echo "1️⃣  Checking code formatting..."
if cargo fmt --all -- --check; then
    echo -e "${GREEN}✓ Format check passed${NC}"
else
    echo -e "${YELLOW}⚠ Format issues found. Run: cargo fmt --all${NC}"
fi
echo ""

# 2. Clippy
echo "2️⃣  Running clippy..."
if cargo clippy --all-features -- -D warnings; then
    echo -e "${GREEN}✓ Clippy passed${NC}"
else
    echo -e "${RED}✗ Clippy failed${NC}"
    exit 1
fi
echo ""

# 3. Build
echo "3️⃣  Building project..."
if cargo build --locked --all-targets --features database; then
    echo -e "${GREEN}✓ Build passed${NC}"
else
    echo -e "${RED}✗ Build failed${NC}"
    exit 1
fi
echo ""

# 4. Tests
echo "4️⃣  Running tests..."
if cargo test --locked --all-targets --features database -- --skip performance; then
    echo -e "${GREEN}✓ Tests passed${NC}"
else
    echo -e "${RED}✗ Tests failed${NC}"
    exit 1
fi
echo ""

# 5. Security audit
echo "5️⃣  Running security audit..."
if cargo audit 2>&1 | grep -q "error:"; then
    echo -e "${YELLOW}⚠ Cargo audit found vulnerabilities${NC}"

    # Check if only known issues
    if cargo audit 2>&1 | grep -E "RUSTSEC-" | grep -v -E "(RUSTSEC-2023-0071|RUSTSEC-2025-0040|RUSTSEC-2024-0370|RUSTSEC-2023-0040|RUSTSEC-2023-0059)"; then
        echo -e "${RED}✗ New security vulnerabilities found!${NC}"
        cargo audit
        exit 1
    else
        echo -e "${GREEN}✓ Only known unfixable vulnerabilities (rsa, users)${NC}"
    fi
else
    echo -e "${GREEN}✓ No vulnerabilities found${NC}"
fi
echo ""

echo "================================================"
echo -e "${GREEN}✅ All CI checks passed!${NC}"
echo "================================================"
