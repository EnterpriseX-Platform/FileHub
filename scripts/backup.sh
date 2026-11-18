#!/usr/bin/env bash
# Daily backup of the File Hub workspace.
#
# Produces a single tarball:
#   <out>/filehub-backup-<YYYYMMDD-HHMMSS>.tar.gz
# containing:
#   - postgres.sql.gz   (pg_dump --format=custom --no-owner --no-privileges)
#   - storage/          (encrypted file payloads from STORAGE_ROOT)
#   - workspace.env     (subset of env: STORAGE_ROOT, quota, app version)
#
# To restore: `scripts/restore.sh <tarball>`
#
# Usage:
#   ./scripts/backup.sh [output-dir]
#
# Defaults to ./backups.  Tarballs older than RETENTION_DAYS (default 14) are
# pruned at the end.

set -euo pipefail

OUT_DIR="${1:-./backups}"
RETENTION_DAYS="${RETENTION_DAYS:-14}"
STAMP="$(date +%Y%m%d-%H%M%S)"

CONTAINER="${POSTGRES_CONTAINER:-filehub-postgres}"
DB_NAME="${POSTGRES_DB:-filehub}"
DB_USER="${POSTGRES_USER:-filehub}"
STORAGE_ROOT="${STORAGE_ROOT:-./backend/storage}"

mkdir -p "$OUT_DIR"
WORK="$(mktemp -d -t filehub-backup.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

echo "→ Dumping Postgres ($DB_NAME) from container $CONTAINER…"
docker exec "$CONTAINER" pg_dump --format=custom --no-owner --no-privileges \
    -U "$DB_USER" "$DB_NAME" > "$WORK/postgres.dump"
gzip -9 "$WORK/postgres.dump"

if [[ -d "$STORAGE_ROOT" ]]; then
    echo "→ Archiving storage tree at $STORAGE_ROOT…"
    tar -C "$(dirname "$STORAGE_ROOT")" -czf "$WORK/storage.tar.gz" "$(basename "$STORAGE_ROOT")"
else
    echo "⚠️  STORAGE_ROOT=$STORAGE_ROOT not found — skipping storage archive"
    echo "no storage dir" > "$WORK/storage-missing"
fi

echo "→ Capturing env snapshot…"
# IMPORTANT: STORAGE_ENC_KEY MUST be captured here or the storage archive is
# permanently unreadable on restore — the AES-256-GCM ciphertexts decrypt
# only with the original key.  We refuse to produce a backup that misses it
# so an operator can't accidentally lose the key.
if [[ -z "${STORAGE_ENC_KEY:-}" ]]; then
    # Try to read from backend/.env if not in the environment already.
    if [[ -f "$(dirname "$0")/../backend/.env" ]]; then
        STORAGE_ENC_KEY="$(grep -E '^STORAGE_ENC_KEY=' "$(dirname "$0")/../backend/.env" | head -1 | cut -d= -f2- || true)"
    fi
fi
if [[ -z "${STORAGE_ENC_KEY:-}" ]]; then
    cat <<MSG >&2
✗ STORAGE_ENC_KEY is empty.

If your deployment encrypts storage at rest (recommended), backups MUST capture
the key — without it the storage tarball is unreadable garbage.  If your
deployment is intentionally running with encryption OFF, re-run with
ALLOW_MISSING_ENC_KEY=1 to acknowledge.
MSG
    if [[ "${ALLOW_MISSING_ENC_KEY:-0}" != "1" ]]; then
        exit 2
    fi
fi
cat > "$WORK/workspace.env" <<EOF
# Snapshot taken $STAMP
STORAGE_ROOT=$STORAGE_ROOT
POSTGRES_DB=$DB_NAME
POSTGRES_USER=$DB_USER
STORAGE_ENC_KEY=${STORAGE_ENC_KEY:-}
APP_VERSION=$(git -C "$(dirname "$0")/.." rev-parse --short HEAD 2>/dev/null || echo unknown)
EOF
# workspace.env carries the encryption key — restrict its permissions inside
# the tarball so a stray `tar xf` doesn't expose it world-readable.
chmod 600 "$WORK/workspace.env"

TARBALL="$OUT_DIR/filehub-backup-$STAMP.tar.gz"
tar -C "$WORK" -czf "$TARBALL" .
SHA="$(shasum -a 256 "$TARBALL" | awk '{print $1}')"
echo "$SHA  $(basename "$TARBALL")" > "$TARBALL.sha256"

echo "✓ Backup → $TARBALL"
echo "  sha256: $SHA"
echo "  size:   $(du -h "$TARBALL" | awk '{print $1}')"

# Retention prune
echo "→ Pruning backups older than $RETENTION_DAYS days…"
find "$OUT_DIR" -name 'filehub-backup-*.tar.gz' -type f -mtime "+$RETENTION_DAYS" -print -delete || true
find "$OUT_DIR" -name 'filehub-backup-*.tar.gz.sha256' -type f -mtime "+$RETENTION_DAYS" -print -delete || true

echo "Done."
