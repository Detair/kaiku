#!/bin/bash
# Grant permissions for sqlx test fixtures
# This runs as postgres superuser during container initialization

set -e

# POSTGRESQL_POSTGRES_PASSWORD must be set to enable postgres superuser
if [ -n "$POSTGRESQL_POSTGRES_PASSWORD" ]; then
    echo "Granting test permissions to voicechat user..."

    PGPASSWORD="$POSTGRESQL_POSTGRES_PASSWORD" psql -U postgres -d voicechat <<-EOSQL
        -- Grant CREATEDB for creating test databases
        ALTER USER voicechat CREATEDB;

        -- Grant SUPERUSER for sqlx::test to query system catalogs
        -- This is safe for development environments
        ALTER USER voicechat WITH SUPERUSER;

        -- Verify
        SELECT rolname, rolsuper, rolcreatedb FROM pg_roles WHERE rolname = 'voicechat';
EOSQL

    echo "Test permissions granted successfully."
else
    echo "POSTGRESQL_POSTGRES_PASSWORD not set, skipping superuser grant."
fi
