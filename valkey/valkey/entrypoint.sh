#!/bin/sh
# ─────────────────────────────────────────────────────────────────────────────
# Generate the Valkey ACL file from env, then launch valkey-server.
#
# Runs on EVERY boot (VK_TENANTS in .env is the source of truth). Creates:
#   • default  → admin: full access to all keys (~*) and channels (&*).
#   • one user per tenant → locked to its own prefix:
#         keys      ~<name>:*
#         channels  &<name>:*
#     with all commands EXCEPT @dangerous (no FLUSHALL/FLUSHDB/CONFIG/SHUTDOWN/…),
#     which matters because all tenants share one keyspace — a stray FLUSHDB
#     would wipe every service.
#
# Tenants are declared in VK_TENANTS as space-separated `name:password` pairs.
# The `name` is BOTH the ACL username and the mandatory key/channel prefix.
# ─────────────────────────────────────────────────────────────────────────────
set -eu

ACL=/tmp/users.acl
umask 077
: > "$ACL"

if [ -z "${VK_ADMIN_PASSWORD:-}" ]; then
    echo "[init] FATAL: VK_ADMIN_PASSWORD is empty" >&2
    exit 1
fi

# Admin = the built-in `default` user.
printf 'user default on >%s ~* &* +@all\n' "$VK_ADMIN_PASSWORD" >> "$ACL"

# Per-tenant users.
for t in ${VK_TENANTS:-}; do
    name=${t%%:*}
    pass=${t#*:}
    if [ -z "$name" ] || [ "$name" = "$t" ] || [ -z "$pass" ]; then
        echo "[init] SKIP malformed tenant '$t' (expected name:password)" >&2
        continue
    fi
    echo "[init] ACL user '$name' -> keys/channels '${name}:*'" >&2
    printf 'user %s on >%s resetchannels ~%s:* &%s:* +@all -@dangerous\n' \
        "$name" "$pass" "$name" "$name" >> "$ACL"
done

echo "[init] starting valkey-server with $(grep -c '^user ' "$ACL") ACL user(s)" >&2

# Note: runs as root (cosmetic warning only) to keep the data-dir/aclfile
# permission handling trivial; this is a private the private overlay-only service.
exec valkey-server /etc/valkey/valkey.conf \
    --aclfile "$ACL" \
    --maxmemory "${VK_MAXMEMORY:-768mb}"
