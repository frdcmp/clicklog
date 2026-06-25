#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# Tenant provisioning for the centralised logging ClickHouse.
#
# Runs ONCE, the first time the server boots against an empty data directory
# (./clickhouse_data). The official clickhouse-server image executes every file
# in /docker-entrypoint-initdb.d/ after the server is up, connecting as the
# bootstrap admin (CLICKHOUSE_USER / CLICKHOUSE_PASSWORD from compose env).
#
# For each tenant we create:
#   • an isolated database  (e.g. `app_one`, `app_two`)
#   • a dedicated user restricted to that database
#   • grants so the tenant's own log worker can CREATE its tables and read/write
#
# Tenants are declared via the CH_TENANTS env var as space-separated triples:
#       db:user:password   db:user:password   ...
# Edit CH_TENANTS in .env to add a new system, then re-init (see README).
#
# NOTE: this script is idempotent (IF NOT EXISTS / OR REPLACE) but only RUNS on
# a fresh data dir. To add a tenant to an already-initialised instance, run the
# equivalent SQL by hand — see the README "Adding a new tenant" section.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

ADMIN_USER="${CLICKHOUSE_USER:-default}"
ADMIN_PASSWORD="${CLICKHOUSE_PASSWORD:-}"

ch() {
    clickhouse client --user "$ADMIN_USER" --password "$ADMIN_PASSWORD" -n "$@"
}

if [[ -z "${CH_TENANTS:-}" ]]; then
    echo "[init] CH_TENANTS is empty — no tenant databases created." >&2
    exit 0
fi

for tenant in $CH_TENANTS; do
    db="${tenant%%:*}"
    rest="${tenant#*:}"
    user="${rest%%:*}"
    password="${rest#*:}"

    if [[ -z "$db" || -z "$user" || -z "$password" || "$rest" == "$tenant" || "$password" == "$rest" ]]; then
        echo "[init] SKIP malformed tenant entry '$tenant' (expected db:user:password)" >&2
        continue
    fi

    echo "[init] provisioning tenant db='$db' user='$user'"
    ch <<-EOSQL
        CREATE DATABASE IF NOT EXISTS \`${db}\`;

        CREATE USER IF NOT EXISTS \`${user}\`
            IDENTIFIED WITH sha256_password BY '${password}'
            DEFAULT DATABASE \`${db}\`;

        -- Full control of its own database (CREATE/ALTER/INSERT/SELECT/DROP …).
        GRANT ALL ON \`${db}\`.* TO \`${user}\`;

        -- The tenant's logs-worker issues CREATE DATABASE IF NOT EXISTS on boot;
        -- the privilege is checked before the existence short-circuit, so grant
        -- it scoped to this database name only.
        GRANT CREATE DATABASE ON \`${db}\`.* TO \`${user}\`;
EOSQL
done

echo "[init] tenant provisioning complete."
