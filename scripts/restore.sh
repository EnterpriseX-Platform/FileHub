#!/usr/bin/env bash
# Restore a backup tarball produced by scripts/backup.sh.
#
#   ./scripts/restore.sh <tarball.tar.gz>
#
# This is destructive: it wipes the current `filehub` database + storage dir
# and replaces them with the snapshot.  The script refuses to run unless the
# user types `yes` interactively.

set -euo pipefail

TARBALL="${1:-}"
if [[ -z "$TARBALL" || ! -f "$TARBALL" ]]; then
    echo "Usage: $0 <tarball.tar.gz>" >&2
    exit 2
fi

CONTAINER="${POSTGRES_CONTAINER:-filehub-postgres}"
DB_NAME="${POSTGRES_DB:-filehub}"
DB_USER="${POSTGRES_USER:-filehub}"
STORAGE_ROOT="${STORAGE_ROOT:-./backend/storage}"

echo "About to restore $TARBALL into:"
echo "  Postgres: $CONTAINER / $DB_NAME (existing data will be dropped)"
echo "  Storage:  $STORAGE_ROOT      (existing files will be replaced)"
read -p "Type 'yes' to confirm: " ans
if [[ "$ans" != "yes" ]]; then
    echo "Aborted." >&2
    exit 1
fi

WORK="$(mktemp -d -t filehub-restore.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
tar -C "$WORK" -xzf "$TARBALL"

echo "→ Restoring Postgres…"
docker exec -i "$CONTAINER" psql -U "$DB_USER" -d postgres -c "DROP DATABASE IF EXISTS \"$DB_NAME\";"
docker exec -i "$CONTAINER" psql -U "$DB_USER" -d postgres -c "CREATE DATABASE \"$DB_NAME\" OWNER \"$DB_USER\";"
gunzip -c "$WORK/postgres.dump.gz" | docker exec -i "$CONTAINER" pg_restore --no-owner --no-privileges -U "$DB_USER" -d "$DB_NAME"

if [[ -f "$WORK/storage.tar.gz" ]]; then
    echo "→ Restoring storage tree…"
    rm -rf "$STORAGE_ROOT"
    mkdir -p "$(dirname "$STORAGE_ROOT")"
    tar -C "$(dirname "$STORAGE_ROOT")" -xzf "$WORK/storage.tar.gz"
fi

echo "Done.  Restart the backend so it picks up the restored state."
