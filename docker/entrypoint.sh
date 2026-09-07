#!/bin/sh
# RustSocks Docker Entrypoint Script
# Performs initialization before starting the server

set -e

echo "🚀 RustSocks Docker Entrypoint"
echo "================================"

# Configuration file path
CONFIG_FILE="${RUSTSOCKS_CONFIG:-/etc/rustsocks/rustsocks.toml}"

# Check if config file exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo "⚠️  Config file not found at $CONFIG_FILE"

    # Check if example exists
    if [ -f "${CONFIG_FILE}.example" ]; then
        echo "📋 Using example config from ${CONFIG_FILE}.example"
        cp "${CONFIG_FILE}.example" "$CONFIG_FILE"
    else
        echo "❌ No config file available. Please mount configuration:"
        echo "   docker run -v ./rustsocks.toml:/etc/rustsocks/rustsocks.toml rustsocks"
        exit 1
    fi
fi

echo "✓ Configuration file: $CONFIG_FILE"

# Database setup
DB_PATH="${RUSTSOCKS_DB_PATH:-/data/sessions.db}"
DB_DIR=$(dirname "$DB_PATH")

# Create data directory if it doesn't exist
if [ ! -d "$DB_DIR" ]; then
    echo "📁 Creating data directory: $DB_DIR"
    mkdir -p "$DB_DIR"
fi

# Check if database exists
if [ -f "$DB_PATH" ]; then
    echo "✓ Database found: $DB_PATH"
else
    echo "📊 Database will be created at: $DB_PATH"
fi

# Note: SQLite migrations are run automatically by RustSocks on startup
# when using sqlx with migrations/ directory embedded

# PAM configuration check
if [ -f /etc/pam.d/rustsocks ]; then
    echo "✓ PAM username auth configured"
fi

if [ -f /etc/pam.d/rustsocks-client ]; then
    echo "✓ PAM address auth configured"
fi

# ACL configuration check
ACL_FILE="/etc/rustsocks/acl.toml"
if [ -f "$ACL_FILE" ]; then
    echo "✓ ACL configuration found"
elif [ -f "${ACL_FILE}.example" ]; then
    echo "📋 Initializing ACL configuration from ${ACL_FILE}.example"
    cp "${ACL_FILE}.example" "$ACL_FILE"
else
    echo "❌ ACL configuration is enabled but no ACL file or example is available"
    exit 1
fi

# Dashboard check
if [ -d "/app/dashboard/dist" ]; then
    DASHBOARD_FILES=$(find /app/dashboard/dist -type f | wc -l)
    echo "✓ Dashboard ready ($DASHBOARD_FILES files)"
else
    echo "⚠️  Dashboard not found (build may have failed)"
fi

# Display environment info
echo ""
echo "Environment:"
echo "  Log level: ${RUST_LOG:-info}"
echo "  Config: $CONFIG_FILE"
echo "  Database: $DB_PATH"
echo ""

# Execute the provided command
echo "🎯 Starting RustSocks..."
echo "================================"
echo ""

exec "$@"
