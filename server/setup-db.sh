#!/bin/bash
# Database Setup Script for VoiceChat Server

set -e

echo "Setting up VoiceChat database..."

# Create user and database
sudo -u postgres psql <<EOF
-- Create user if not exists
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_user WHERE usename = 'voicechat') THEN
    CREATE USER voicechat WITH PASSWORD 'voicechat_dev_pass';
  END IF;
END
\$\$;

-- Create database if not exists
SELECT 'CREATE DATABASE voicechat OWNER voicechat'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'voicechat')\gexec

-- Grant privileges
GRANT ALL PRIVILEGES ON DATABASE voicechat TO voicechat;
EOF

echo "✓ Database user and database created"

# Run migrations
echo "Running database migrations..."
cd "$(dirname "$0")"
sqlx migrate run

echo "✓ Database setup complete!"
echo ""
echo "You can now start the server with: cargo run"
