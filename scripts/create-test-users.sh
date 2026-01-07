#!/usr/bin/env bash
#
# Create test users for development via the API
#
# This script creates test users using the registration endpoint,
# ensuring passwords are properly hashed with Argon2id.
#
# Prerequisites:
#   - Server must be running (make dev or cargo run -p vc-server)
#   - curl must be installed
#
# Usage: ./scripts/create-test-users.sh [SERVER_URL]
#

set -euo pipefail

SERVER_URL="${1:-http://localhost:8080}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if server is running
log_info "Checking server at ${SERVER_URL}..."
if ! curl -s "${SERVER_URL}/health" > /dev/null 2>&1; then
    log_error "Server not reachable at ${SERVER_URL}"
    echo "Make sure the server is running: make dev"
    exit 1
fi
log_success "Server is running"

echo ""
log_info "Creating test users..."
echo ""

# Function to create a user
create_user() {
    local username=$1
    local display_name=$2
    local email=$3
    local password=$4

    echo -n "  Creating user '${username}'... "

    response=$(curl -s -w "\n%{http_code}" -X POST "${SERVER_URL}/auth/register" \
        -H "Content-Type: application/json" \
        -d "{
            \"username\": \"${username}\",
            \"display_name\": \"${display_name}\",
            \"email\": \"${email}\",
            \"password\": \"${password}\"
        }")

    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    case $http_code in
        200|201)
            echo -e "${GREEN}created${NC}"
            ;;
        409)
            echo -e "${YELLOW}already exists${NC}"
            ;;
        *)
            echo -e "${RED}failed (HTTP ${http_code})${NC}"
            echo "    Response: ${body}"
            ;;
    esac
}

# Create test users
create_user "admin" "Admin User" "admin@localhost" "admin123"
create_user "alice" "Alice Developer" "alice@localhost" "password123"
create_user "bob" "Bob Tester" "bob@localhost" "password123"
create_user "charlie" "Charlie QA" "charlie@localhost" "password123"

echo ""
log_success "Test users created!"
echo ""
echo "Test credentials:"
echo "  admin   / admin123"
echo "  alice   / password123"
echo "  bob     / password123"
echo "  charlie / password123"
echo ""

# Test login
log_info "Testing login with admin user..."
login_response=$(curl -s -X POST "${SERVER_URL}/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"username": "admin", "password": "admin123"}')

if echo "$login_response" | grep -q "access_token"; then
    log_success "Login successful!"
    echo ""
    echo "Sample access token (expires in 15 min):"
    echo "$login_response" | grep -oP '"access_token"\s*:\s*"\K[^"]+' | head -c 50
    echo "..."
else
    log_error "Login failed"
    echo "Response: $login_response"
fi
