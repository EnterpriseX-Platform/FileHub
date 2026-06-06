#!/usr/bin/env bash
# End-to-end test suite for the File Hub backend.
#
# Hits every endpoint with curl, asserts the responses with python3, and prints
# a colored PASS/FAIL line per check.  Requires:
#   - the backend running on $BASE (default http://localhost:8090)
#   - python3 (stdlib only) on $PATH
#
# Usage:
#   ./scripts/test-api.sh                     # default base
#   BASE=http://localhost:8090 ./scripts/test-api.sh

set -u

BASE="${BASE:-http://localhost:8090}"
# Backend mounts everything under /fh — fold that into BASE so $BASE$path
# patterns continue to work without per-call sed.
case "$BASE" in
    */fh) ;;
    *) BASE="$BASE/fh" ;;
esac
CURL="${CURL:-/usr/bin/curl}"
PY="${PY:-python3}"

# Login once and stash the cookie for all mutating requests. Default to the
# editor account; tests that need admin-only behaviour log in again.
COOKIE_JAR="$(mktemp -t filehub-cookies.XXXXXX)"
trap 'rm -f "$COOKIE_JAR"' EXIT

"$CURL" -s -o /dev/null -c "$COOKIE_JAR" -X POST "$BASE/api/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"email":"anong@acme.go.th","password":"anong123"}'

AUTH="-b $COOKIE_JAR"

# Seed UUIDs live in backend/migrations/0002_seed.sql. They are inlined as
# literals throughout this script so assertion failures point at the exact
# rows. Highlights:
#   sys_hr  HR System
#   sys_fin  Finance
#   org_phattana  org-phattana
#   00000000-0000-7000-8000-000000002001  contract-A12.pdf
#   00000000-0000-7000-8000-00000000200b  team-photo.jpg
#   00000000-0000-4000-8000-fffffffffff0  reserved-not-present  (404 sentinel)

# ANSI colors (disabled if not a TTY)
if [[ -t 1 ]]; then
  C_GREEN=$'\e[32m'; C_RED=$'\e[31m'; C_YELLOW=$'\e[33m'; C_DIM=$'\e[2m'; C_BOLD=$'\e[1m'; C_RESET=$'\e[0m'
else
  C_GREEN=""; C_RED=""; C_YELLOW=""; C_DIM=""; C_BOLD=""; C_RESET=""
fi

PASS=0
FAIL=0
TOTAL=0

# ----------------------------------------------------------------------------
# Test helper
# ----------------------------------------------------------------------------
# pass <label>
pass() { TOTAL=$((TOTAL+1)); PASS=$((PASS+1)); printf "  ${C_GREEN}✓${C_RESET} %s\n" "$1"; }
# fail <label> <detail>
fail() { TOTAL=$((TOTAL+1)); FAIL=$((FAIL+1)); printf "  ${C_RED}✗${C_RESET} %s\n    ${C_DIM}%s${C_RESET}\n" "$1" "${2:-}"; }
section() { printf "\n${C_BOLD}%s${C_RESET}\n" "$1"; }

# assert_json <label> <body> <python-expr-or-script>
# expr operates on variable `d` (parsed JSON). Should evaluate truthy.
assert_json() {
  local label="$1" body="$2" expr="$3"
  if [[ -z "$body" ]]; then fail "$label" "empty body"; return; fi
  if printf '%s' "$body" | "$PY" -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception as e:
    print('JSON parse error:', e, file=sys.stderr); sys.exit(2)
try:
    ok = bool($expr)
except Exception as e:
    print('assertion error:', e, file=sys.stderr); sys.exit(2)
sys.exit(0 if ok else 1)
" 2>/tmp/filehub-test.err; then
    pass "$label"
  else
    fail "$label" "$(cat /tmp/filehub-test.err 2>/dev/null) — body was: $(echo "$body" | head -c 200)"
  fi
}

# assert_status <label> <expected-status> <method> <path> [extra curl args...]
# Auth cookie is included by default; pass it explicitly if you want to test
# the unauthenticated path.
assert_status() {
  local label="$1" expected="$2" method="$3" path="$4"; shift 4
  local actual
  actual=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X "$method" "$BASE$path" "$@")
  if [[ "$actual" == "$expected" ]]; then
    pass "$label ($method $path → $actual)"
  else
    fail "$label" "expected $expected, got $actual"
  fi
}

# ----------------------------------------------------------------------------
# 1. Health
# ----------------------------------------------------------------------------
section "1. Health"
HEALTH=$("$CURL" -s "$BASE/api/health")
if [[ "$HEALTH" == "ok" ]]; then pass "GET /api/health returns 'ok'"
else fail "GET /api/health" "got: $HEALTH"; fi

# ----------------------------------------------------------------------------
# 2. Stats
# ----------------------------------------------------------------------------
section "2. Dashboard stats"
STATS=$("$CURL" -s $AUTH "$BASE/api/stats")
assert_json "stats has total_files (int)"        "$STATS" "isinstance(d.get('total_files'), int) and d['total_files'] >= 0"
assert_json "stats has total_size_bytes (int)"   "$STATS" "isinstance(d.get('total_size_bytes'), int) and d['total_size_bytes'] >= 0"
assert_json "stats has total_quota_bytes (int)"  "$STATS" "isinstance(d.get('total_quota_bytes'), int) and d['total_quota_bytes'] > 0"
assert_json "stats has active_orgs (int)"        "$STATS" "isinstance(d.get('active_orgs'), int) and d['active_orgs'] > 0"
assert_json "stats has awaiting_review (int)"    "$STATS" "isinstance(d.get('awaiting_review'), int) and d['awaiting_review'] >= 0"
assert_json "stats.storage_by_system is list"    "$STATS" "isinstance(d.get('storage_by_system'), list) and len(d['storage_by_system']) == 7"
assert_json "stats.connected_systems is list"    "$STATS" "isinstance(d.get('connected_systems'), list) and len(d['connected_systems']) == 7"

# ----------------------------------------------------------------------------
# 3. Systems
# ----------------------------------------------------------------------------
section "3. Systems"
SYSTEMS=$("$CURL" -s $AUTH "$BASE/api/systems")
assert_json "GET /api/systems returns 7 systems"    "$SYSTEMS" "isinstance(d, list) and len(d) == 7"
assert_json "every system has id/name/tone/bucket"  "$SYSTEMS" "all('id' in s and 'name' in s and 'tone' in s and 'bucket' in s for s in d)"
assert_json "HR System has tone='rose'"             "$SYSTEMS" "next(s for s in d if s['id']=='sys_hr')['tone'] == 'rose'"

# ----------------------------------------------------------------------------
# 4. Orgs
# ----------------------------------------------------------------------------
section "4. Orgs"
ORGS_ALL=$("$CURL" -s $AUTH "$BASE/api/orgs")
assert_json "GET /api/orgs returns array"     "$ORGS_ALL" "isinstance(d, list) and len(d) > 0"

ORGS_HR=$("$CURL" -s $AUTH "$BASE/api/orgs?system_id=sys_hr")
assert_json "?system_id=sys_hr filter applies" "$ORGS_HR" "all(o['system_id']=='sys_hr' for o in d) and len(d) >= 5"
assert_json "Thai org names present"           "$ORGS_HR" "any('สำนัก พัฒนาฯ' in o['name'] for o in d)"

ORGS_BAD=$("$CURL" -s $AUTH "$BASE/api/orgs?system_id=sys_does_not_exist")
assert_json "?system_id=unknown UUID returns empty" "$ORGS_BAD" "isinstance(d, list) and len(d) == 0"

ORGS_MALFORMED=$("$CURL" -s $AUTH "$BASE/api/orgs?system_id=not-a-system" | "$PY" -c "import json,sys; print(len(json.load(sys.stdin)))")
if [[ "$ORGS_MALFORMED" == "0" ]]; then pass "?system_id=unknown text returns empty"
else fail "?system_id=unknown" "expected empty list, got len $ORGS_MALFORMED"; fi

# ----------------------------------------------------------------------------
# 5. Files — listing and filtering
# ----------------------------------------------------------------------------
section "5. Files — list + filter"
FILES=$("$CURL" -s $AUTH "$BASE/api/files")
assert_json "GET /api/files returns array"     "$FILES" "isinstance(d, list) and len(d) >= 12"
assert_json "files include contract-A12.pdf"   "$FILES" "any(f['name']=='contract-A12.pdf' for f in d)"
assert_json "files sorted modified DESC"       "$FILES" "all(d[i]['modified_at'] >= d[i+1]['modified_at'] for i in range(len(d)-1))"

FILES_HR=$("$CURL" -s $AUTH "$BASE/api/files?system_id=sys_hr")
assert_json "?system_id=sys_hr filter applies" "$FILES_HR" "all(f['system_id']=='sys_hr' for f in d) and len(d) >= 5"

FILES_REVIEW=$("$CURL" -s $AUTH "$BASE/api/files?status=Review")
assert_json "?status=Review filter applies"    "$FILES_REVIEW" "all(f['status']=='Review' for f in d) and len(d) >= 1"

FILES_Q1=$("$CURL" -s $AUTH "$BASE/api/files?project=Q1-2026")
assert_json "?project=Q1-2026 filter applies"  "$FILES_Q1" "all(f['project']=='Q1-2026' for f in d) and len(d) >= 1"

# ----------------------------------------------------------------------------
# 6. File detail
# ----------------------------------------------------------------------------
section "6. File detail"
FILE_001=$("$CURL" -s $AUTH "$BASE/api/files/00000000-0000-7000-8000-000000002001")
assert_json "GET /api/files/00000000-0000-7000-8000-000000002001"                "$FILE_001" "d['id']=='00000000-0000-7000-8000-000000002001' and d['name']=='contract-A12.pdf'"
assert_json "00000000-0000-7000-8000-000000002001 has expected metadata"         "$FILE_001" "d['file_type']=='pdf' and d['status']=='Review' and d['project']=='Q1-2026'"
assert_json "00000000-0000-7000-8000-000000002001 owner = Anong K."              "$FILE_001" "d['owner']=='Anong K.'"
assert_json "00000000-0000-7000-8000-000000002001 has tags as JSON string"       "$FILE_001" "'legal' in __import__('json').loads(d['tags'])"
assert_status "404 for unknown file" "404" GET "/api/files/00000000-0000-4000-8000-fffffffffff0"

# ----------------------------------------------------------------------------
# 7. Upload — full round-trip through filesystem
# ----------------------------------------------------------------------------
section "7. Upload → download → delete (filesystem round-trip)"
TMP_UP=$(mktemp -t filehub-upload.XXXXXX)
# Pad to ~2 KB so size_bytes is interesting
{ printf "File Hub end-to-end test\n"; printf "ลูกค้า ทดสอบ ภาษาไทย\n"; head -c 2000 /dev/urandom | base64; } > "$TMP_UP"
UP_SIZE=$(wc -c <"$TMP_UP" | tr -d ' ')

UPLOAD=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" \
  -F "file=@$TMP_UP;filename=test-roundtrip.txt" \
  -F "system_id=sys_hr" \
  -F "org_id=org_phattana" \
  -F "project=Q1-2026" \
  -F "status=Review" \
  -F "owner=Pat S." \
  -F 'tags=["roundtrip","auto-test"]')

assert_json "POST /api/files returns new file"       "$UPLOAD" "d['name']=='test-roundtrip.txt'"
assert_json "uploaded .txt detected as file_type='txt'" "$UPLOAD" "d['file_type']=='txt'"
assert_json "uploaded size_bytes matches local size" "$UPLOAD" "d['size_bytes']==$UP_SIZE"
assert_json "uploaded etag is 32-hex MD5"            "$UPLOAD" "isinstance(d['etag'], str) and len(d['etag'])==32 and all(c in '0123456789abcdef' for c in d['etag'])"
assert_json "uploaded tags preserved"                "$UPLOAD" "set(__import__('json').loads(d['tags'])) == {'roundtrip','auto-test'}"

NEW_ID=$(printf '%s' "$UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
OBJECT_KEY=$(printf '%s' "$UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['object_key'])")

# Verify the file actually lives on disk where the row says it does. When
# encryption is enabled the on-disk size will be ~28 bytes larger (12-byte
# nonce + 16-byte GCM tag), so we only assert that the file exists and is
# non-empty; the byte-for-byte check happens on download instead.
STORAGE_ROOT="${STORAGE_ROOT:-./backend/storage}"
DISK_PATH="$STORAGE_ROOT/$OBJECT_KEY"
if [[ -f "$DISK_PATH" ]]; then
  DISK_SIZE=$(wc -c <"$DISK_PATH" | tr -d ' ')
  if [[ "$DISK_SIZE" -gt 0 ]]; then
    pass "file exists on disk at $DISK_PATH (${DISK_SIZE} bytes; raw size $UP_SIZE)"
  else
    fail "disk file empty" "$DISK_PATH"
  fi
else
  fail "file not on disk" "expected at $DISK_PATH"
fi

# Download and compare bytes
TMP_DOWN=$(mktemp -t filehub-download.XXXXXX)
"$CURL" -s -o "$TMP_DOWN" $AUTH "$BASE/api/files/$NEW_ID/download"
if cmp -s "$TMP_UP" "$TMP_DOWN"; then
  pass "GET /api/files/$NEW_ID/download bytes match uploaded content"
else
  fail "download content mismatch" "diff between $TMP_UP and $TMP_DOWN"
fi

# Check Content-Disposition header
DISPO=$("$CURL" -s -D - -o /dev/null $AUTH "$BASE/api/files/$NEW_ID/download" | tr -d '\r' | grep -i '^content-disposition:')
case "$DISPO" in
  *test-roundtrip.txt*) pass "download Content-Disposition contains filename" ;;
  *) fail "download Content-Disposition" "$DISPO" ;;
esac

# Detail endpoint after upload
DETAIL=$("$CURL" -s $AUTH "$BASE/api/files/$NEW_ID")
assert_json "detail returns the newly uploaded row"  "$DETAIL" "d['id']=='$NEW_ID' and d['name']=='test-roundtrip.txt'"

# Delete
DEL_CODE=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X DELETE "$BASE/api/files/$NEW_ID")
if [[ "$DEL_CODE" == "204" ]]; then
  pass "DELETE /api/files/$NEW_ID → 204"
else
  fail "DELETE" "expected 204, got $DEL_CODE"
fi

# After soft delete: file is hidden from list / detail but bytes remain on disk
# (recoverable from /api/trash). Hard delete is admin-only and exercised
# separately later.
assert_status "GET soft-deleted file → 404"     "404" GET  "/api/files/$NEW_ID"
assert_status "GET soft-deleted download → 404" "404" GET  "/api/files/$NEW_ID/download"
if [[ -f "$DISK_PATH" ]]; then
  pass "disk file still present after soft delete (recoverable)"
else
  fail "disk file missing after soft delete" "$DISK_PATH should remain until purge"
fi
# Restore + verify
RESTORE_CODE=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X POST "$BASE/api/files/$NEW_ID/restore")
if [[ "$RESTORE_CODE" == "200" ]]; then pass "restore → 200"
else fail "restore" "got $RESTORE_CODE"; fi
RESTORED=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" "$BASE/api/files/$NEW_ID")
if [[ "$RESTORED" == "200" ]]; then pass "GET restored file → 200"
else fail "restored GET" "got $RESTORED"; fi
# Now hard delete via admin to clean up the disk
ADMIN_JAR="$(mktemp -t filehub-admin.XXXXXX)"
"$CURL" -s -o /dev/null -c "$ADMIN_JAR" -X POST "$BASE/api/auth/login" -H "Content-Type: application/json" -d '{"email":"admin@acme.go.th","password":"admin123"}'
"$CURL" -s -o /dev/null -b "$ADMIN_JAR" -X DELETE "$BASE/api/files/$NEW_ID?hard=true"
rm -f "$ADMIN_JAR"

rm -f "$TMP_UP" "$TMP_DOWN"

# ----------------------------------------------------------------------------
# 8. Upload — bad input
# ----------------------------------------------------------------------------
section "8. Upload — invalid input"
BAD1=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X POST "$BASE/api/files" -F "system_id=sys_hr")
if [[ "$BAD1" == "400" ]]; then pass "POST without file → 400"
else fail "POST without file" "expected 400, got $BAD1"; fi

TMP_BAD=$(mktemp -t filehub-bad.XXXXXX); echo "x" > "$TMP_BAD"
BAD2=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X POST "$BASE/api/files" \
  -F "file=@$TMP_BAD;filename=x.txt" -F "system_id=sys_does_not_exist")
if [[ "$BAD2" == "400" ]]; then pass "POST with unknown system_id UUID → 400"
else fail "POST unknown system" "expected 400, got $BAD2"; fi
rm -f "$TMP_BAD"

# ----------------------------------------------------------------------------
# 9. Activity feed
# ----------------------------------------------------------------------------
section "9. Activity"
ACTIVITY=$("$CURL" -s $AUTH "$BASE/api/activity?limit=20")
assert_json "GET /api/activity returns array"        "$ACTIVITY" "isinstance(d, list) and len(d) >= 6"
assert_json "activity rows have actor + action"      "$ACTIVITY" "all('actor' in a and 'action' in a for a in d)"
assert_json "activity sorted by created_at DESC"     "$ACTIVITY" "all(d[i]['created_at'] >= d[i+1]['created_at'] for i in range(len(d)-1))"

# Upload one more file to confirm activity is appended
TMP2=$(mktemp -t filehub-act.XXXXXX); echo "activity test" > "$TMP2"
ACTIVITY_FILE=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" \
  -F "file=@$TMP2;filename=activity-probe.txt" \
  -F "system_id=sys_fin" -F "owner=Auto Tester")
PROBE_ID=$(printf '%s' "$ACTIVITY_FILE" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
ACTIVITY2=$("$CURL" -s $AUTH "$BASE/api/activity?limit=5")
assert_json "new upload appears at top of activity"  "$ACTIVITY2" "d[0]['target']=='activity-probe.txt' and d[0]['action']=='uploaded'"
# Clean up
"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$PROBE_ID"
rm -f "$TMP2"

# ----------------------------------------------------------------------------
# 10. Views
# ----------------------------------------------------------------------------
section "10. Views"
VIEWS=$("$CURL" -s $AUTH "$BASE/api/views")
assert_json "GET /api/views returns seeded views"    "$VIEWS" "isinstance(d, list) and len(d) >= 5"
assert_json "pinned views come first"                "$VIEWS" "bool(d[0]['pinned']) is True"
assert_json "Q1-2026 Board exists with layout=board" "$VIEWS" "any(v['name']=='Q1 2026 Board' and v['layout']=='board' for v in d)"

NEW_VIEW=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/views" \
  -H "Content-Type: application/json" \
  -d '{"name":"Auto test view","layout":"table","color":"#16a34a","pinned":false}')
assert_json "POST /api/views creates row"            "$NEW_VIEW" "d['name']=='Auto test view' and d['layout']=='table' and d['color']=='#16a34a'"

# ----------------------------------------------------------------------------
# 11. Permissions
# ----------------------------------------------------------------------------
section "11. Permissions"
PERMS=$("$CURL" -s $AUTH "$BASE/api/permissions/00000000-0000-7000-8000-000000002001")
assert_json "GET /api/permissions/00000000-0000-7000-8000-000000002001 returns rows" "$PERMS" "isinstance(d, list) and len(d) >= 6"
assert_json "owner permission exists"                    "$PERMS" "any(p['role']=='owner' for p in d)"
assert_json "external link permission flagged"           "$PERMS" "any(bool(p['external']) is True for p in d)"

PERMS_EMPTY=$("$CURL" -s $AUTH "$BASE/api/permissions/00000000-0000-4000-8000-fffffffffff0")
assert_json "GET /api/permissions/<missing> → empty array" "$PERMS_EMPTY" "isinstance(d, list) and len(d)==0"

# ----------------------------------------------------------------------------
# 12. CORS
# ----------------------------------------------------------------------------
section "12. CORS"
CORS_H=$("$CURL" -s -D - -o /dev/null -H "Origin: http://localhost:3001" "$BASE/api/health" | tr -d '\r' | grep -i '^access-control-allow-origin:' | awk '{print $2}')
if [[ "$CORS_H" == "http://localhost:3001" ]]; then
  pass "CORS allow-origin matches request origin"
else
  fail "CORS header" "got: ${CORS_H:-<none>}"
fi

# ----------------------------------------------------------------------------
# 13. Search (TOR 4.15.14)
# ----------------------------------------------------------------------------
section "13. Search"
SEARCH_C=$("$CURL" -s $AUTH "$BASE/api/search?q=contract")
assert_json "search 'contract' matches PDF rows"     "$SEARCH_C" "isinstance(d, list) and len([f for f in d if 'contract' in f['name']]) >= 2"
assert_json "search results sorted modified DESC"    "$SEARCH_C" "all(d[i]['modified_at'] >= d[i+1]['modified_at'] for i in range(len(d)-1))"

SEARCH_BUDGET=$("$CURL" -s $AUTH "$BASE/api/search?q=budget")
assert_json "search 'budget' finds budget-q1"        "$SEARCH_BUDGET" "any(f['name']=='budget-q1-2026.xlsx' for f in d)"

SEARCH_TAG=$("$CURL" -s $AUTH "$BASE/api/search?q=legal")
assert_json "search 'legal' matches via tags"        "$SEARCH_TAG" "len(d) >= 2"

SEARCH_FILTER=$("$CURL" -s $AUTH "$BASE/api/search?q=contract&system_id=sys_hr")
assert_json "search + system_id filter applies"      "$SEARCH_FILTER" "all(f['system_id']=='sys_hr' for f in d)"

SEARCH_EMPTY=$("$CURL" -s -o /dev/null -w "%{http_code}" $AUTH "$BASE/api/search?q=")
if [[ "$SEARCH_EMPTY" == "400" ]]; then pass "empty q rejected with 400"
else fail "empty q expected 400" "got $SEARCH_EMPTY"; fi

# ----------------------------------------------------------------------------
# 14. Versions (TOR 4.15.7)
# ----------------------------------------------------------------------------
section "14. Versions"
TMP_V1=$(mktemp -t filehub-v1.XXXXXX)
TMP_V2=$(mktemp -t filehub-v2.XXXXXX)
printf "version 1 payload\n" > "$TMP_V1"
printf "version 2 payload — เพิ่มเติม\n" > "$TMP_V2"

V_UPLOAD=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" \
  -F "file=@$TMP_V1;filename=versioned.txt" \
  -F "system_id=sys_hr" -F "owner=Version Test")
V_ID=$(printf '%s' "$V_UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
assert_json "initial upload starts at version 1"     "$V_UPLOAD" "d['version']==1"

V_NEW=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files/$V_ID/versions" \
  -F "file=@$TMP_V2;filename=versioned.txt" \
  -F "uploaded_by=Reviewer A" \
  -F "note=fixed typo")
assert_json "POST new version bumps to v2"           "$V_NEW" "d['version']==2 and d['id']=='$V_ID'"

# Download now returns v2 bytes
TMP_VD=$(mktemp -t filehub-vdownload.XXXXXX)
"$CURL" -s -o "$TMP_VD" $AUTH "$BASE/api/files/$V_ID/download"
if cmp -s "$TMP_V2" "$TMP_VD"; then
  pass "download returns latest (v2) bytes"
else
  fail "download v2 content mismatch" "diff"
fi

# List versions should contain v1 snapshot
V_LIST=$("$CURL" -s $AUTH "$BASE/api/files/$V_ID/versions")
assert_json "version history contains v1"            "$V_LIST" "isinstance(d, list) and any(v['version']==1 for v in d)"
assert_json "v1 snapshot recorded note"              "$V_LIST" "any(v['note']=='fixed typo' for v in d)"
assert_json "v1 uploaded_by recorded"                "$V_LIST" "any(v['uploaded_by']=='Reviewer A' for v in d)"

# Versions endpoint 404s for unknown file
assert_status "versions for unknown file → 404" "404" GET "/api/files/00000000-0000-4000-8000-fffffffffff0/versions"

# Cleanup
"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$V_ID"
rm -f "$TMP_V1" "$TMP_V2" "$TMP_VD"

# ----------------------------------------------------------------------------
# 15. PATCH / Move + Rename (TOR 4.15.12)
# ----------------------------------------------------------------------------
section "15. PATCH /api/files/:id"
TMP_M=$(mktemp -t filehub-move.XXXXXX); printf "move me\n" > "$TMP_M"
M_UPLOAD=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" \
  -F "file=@$TMP_M;filename=movable.txt" \
  -F "system_id=sys_hr" -F "owner=Mover")
M_ID=$(printf '%s' "$M_UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
M_KEY=$(printf '%s' "$M_UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['object_key'])")

# Rename
M_RENAMED=$("$CURL" -s -b "$COOKIE_JAR" -X PATCH "$BASE/api/files/$M_ID" \
  -H "Content-Type: application/json" \
  -d '{"name":"renamed.txt"}')
assert_json "PATCH name renames file"                "$M_RENAMED" "d['name']=='renamed.txt'"
assert_json "PATCH name re-keys object_key"          "$M_RENAMED" "d['object_key'] != '$M_KEY' and d['object_key'].endswith('renamed.txt')"

# Move across system
M_MOVED=$("$CURL" -s -b "$COOKIE_JAR" -X PATCH "$BASE/api/files/$M_ID" \
  -H "Content-Type: application/json" \
  -d '{"system_id":"sys_fin","org_id":"org_fin_ap","project":"Q1-2026","status":"Review"}')
assert_json "PATCH system_id moves to new system"    "$M_MOVED" "d['system_id']=='sys_fin' and d['org_id']=='org_fin_ap'"
assert_json "moved file has new bucket"              "$M_MOVED" "d['bucket']=='fin-invoices'"
assert_json "moved object_key starts with new bucket" "$M_MOVED" "d['object_key'].startswith('fin-invoices/')"
assert_json "PATCH preserves other fields when omitted" "$M_MOVED" "d['name']=='renamed.txt'"

# Download still works after move
M_DD=$(mktemp -t filehub-mdownload.XXXXXX)
"$CURL" -s -o "$M_DD" $AUTH "$BASE/api/files/$M_ID/download"
if cmp -s "$TMP_M" "$M_DD"; then
  pass "download works after move"
else
  fail "download after move mismatch" ""
fi

# Unknown id (valid UUID, not in DB) → 404
PATCH_404=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X PATCH "$BASE/api/files/00000000-0000-4000-8000-fffffffffff0" \
  -H "Content-Type: application/json" -d '{"name":"x"}')
if [[ "$PATCH_404" == "404" ]]; then pass "PATCH unknown file → 404"
else fail "PATCH 404" "got $PATCH_404"; fi

# Malformed file id in path → still 400 (file id is UUID column type)
PATCH_BAD_PATH=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X PATCH "$BASE/api/files/file-nope" \
  -H "Content-Type: application/json" -d '{"name":"x"}')
if [[ "$PATCH_BAD_PATH" == "400" ]]; then pass "PATCH malformed file uuid → 400"
else fail "PATCH bad path" "got $PATCH_BAD_PATH"; fi

# Unknown system_id (valid UUID, not in DB) → 400
PATCH_400=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X PATCH "$BASE/api/files/$M_ID" \
  -H "Content-Type: application/json" -d '{"system_id":"sys_does_not_exist"}')
if [[ "$PATCH_400" == "400" ]]; then pass "PATCH unknown system_id UUID → 400"
else fail "PATCH 400" "got $PATCH_400"; fi

"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$M_ID"
rm -f "$TMP_M" "$M_DD"

# ----------------------------------------------------------------------------
# 16. Folders (TOR 4.15.4)
# ----------------------------------------------------------------------------
section "16. Folders"
FOLDERS=$("$CURL" -s $AUTH "$BASE/api/folders?system_id=sys_hr")
assert_json "GET /api/folders?system_id=sys_hr returns seeded rows" "$FOLDERS" "isinstance(d, list) and len(d) >= 2"
assert_json "seeded folder Contracts 2026 present"   "$FOLDERS" "any(f['name']=='Contracts 2026' for f in d)"

NEW_FOLDER=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/folders" \
  -H "Content-Type: application/json" \
  -d '{"name":"Auto test folder","system_id":"sys_hr","color":"#9333ea","owner":"Tester","encrypted":true}')
FOLDER_ID=$(printf '%s' "$NEW_FOLDER" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
assert_json "POST /api/folders creates row"          "$NEW_FOLDER" "d['name']=='Auto test folder' and d['system_id']=='sys_hr'"
assert_json "folder encrypted flag preserved"        "$NEW_FOLDER" "bool(d.get('encrypted')) is True"

# Unknown system_id (valid UUID, not in DB) → 400
BAD_FOLDER=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X POST "$BASE/api/folders" \
  -H "Content-Type: application/json" -d '{"name":"x","system_id":"sys_does_not_exist"}')
if [[ "$BAD_FOLDER" == "400" ]]; then pass "POST unknown system_id UUID → 400"
else fail "POST folder unknown" "expected 400, got $BAD_FOLDER"; fi

# Attach a file to the folder via PATCH
TMP_FF=$(mktemp -t filehub-folder.XXXXXX); echo "in folder" > "$TMP_FF"
FF_UPLOAD=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_FF;filename=in-folder.txt" -F "system_id=sys_hr")
FF_ID=$(printf '%s' "$FF_UPLOAD" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
FF_PATCHED=$("$CURL" -s -b "$COOKIE_JAR" -X PATCH "$BASE/api/files/$FF_ID" -H "Content-Type: application/json" -d "{\"folder_id\":\"$FOLDER_ID\"}")
assert_json "PATCH file folder_id assigns folder"    "$FF_PATCHED" "d['folder_id']=='$FOLDER_ID'"

FF_CLEARED=$("$CURL" -s -b "$COOKIE_JAR" -X PATCH "$BASE/api/files/$FF_ID" -H "Content-Type: application/json" -d '{"folder_id":null}')
assert_json "PATCH folder_id:null clears folder"     "$FF_CLEARED" "d['folder_id'] is None"

# Delete folder cascades → folder_id stays NULL on the file (we set ON DELETE SET NULL)
assert_status "DELETE /api/folders/$FOLDER_ID → 204"  "204" DELETE "/api/folders/$FOLDER_ID"
assert_status "DELETE missing folder → 404"            "404" DELETE "/api/folders/fld_does_not_exist"
"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$FF_ID"
rm -f "$TMP_FF"

# ----------------------------------------------------------------------------
# 17. Encryption at rest (TOR 4.15.5)
# ----------------------------------------------------------------------------
section "17. Encryption at rest"
TMP_E=$(mktemp -t filehub-enc.XXXXXX)
MAGIC="SECRET_PLAINTEXT_MARKER_ABCXYZ"
printf "%s\n" "$MAGIC" > "$TMP_E"
ENC_UP=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_E;filename=secret.txt" -F "system_id=sys_hr")
ENC_ID=$(printf '%s' "$ENC_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
ENC_KEY=$(printf '%s' "$ENC_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['object_key'])")
assert_json "uploaded with encrypted=true"           "$ENC_UP" "bool(d['encrypted']) is True"

# Disk bytes must NOT contain the plaintext marker
STORAGE_ROOT="${STORAGE_ROOT:-./backend/storage}"
DISK_PATH="$STORAGE_ROOT/$ENC_KEY"
if grep -q "$MAGIC" "$DISK_PATH" 2>/dev/null; then
  fail "ciphertext leaks plaintext" "found marker in $DISK_PATH"
else
  pass "disk bytes are encrypted (no plaintext marker)"
fi

# Download decrypts correctly
TMP_ED=$(mktemp -t filehub-encdl.XXXXXX)
"$CURL" -s -o "$TMP_ED" $AUTH "$BASE/api/files/$ENC_ID/download"
if grep -q "$MAGIC" "$TMP_ED"; then
  pass "download decrypts to plaintext"
else
  fail "download decryption" "marker missing in downloaded bytes"
fi

# Round-trip through versions: v2 must also encrypt + decrypt
TMP_E2=$(mktemp -t filehub-enc2.XXXXXX); echo "v2 content $MAGIC v2" > "$TMP_E2"
"$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files/$ENC_ID/versions" -F "file=@$TMP_E2;filename=secret.txt" -F "uploaded_by=Tester" > /dev/null
TMP_E2D=$(mktemp -t filehub-enc2dl.XXXXXX)
"$CURL" -s -o "$TMP_E2D" $AUTH "$BASE/api/files/$ENC_ID/download"
if cmp -s "$TMP_E2" "$TMP_E2D"; then
  pass "encrypted versions round-trip"
else
  fail "encrypted v2 download mismatch" ""
fi

"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$ENC_ID"
rm -f "$TMP_E" "$TMP_E2" "$TMP_ED" "$TMP_E2D"

# ----------------------------------------------------------------------------
# 18. Batch upload (TOR 4.15.9)
# ----------------------------------------------------------------------------
section "18. Batch upload"
TMP_B1=$(mktemp -t filehub-b1.XXXXXX); echo "file one"   > "$TMP_B1"
TMP_B2=$(mktemp -t filehub-b2.XXXXXX); echo "file two"   > "$TMP_B2"
TMP_B3=$(mktemp -t filehub-b3.XXXXXX); echo "file three" > "$TMP_B3"

BATCH=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files/batch" \
  -F "system_id=sys_hr" -F "project=Q1-2026" -F "owner=Batch Bot" \
  -F "file=@$TMP_B1;filename=batch-a.txt" \
  -F "file=@$TMP_B2;filename=batch-b.pdf" \
  -F "file=@$TMP_B3;filename=batch-c.docx")
assert_json "POST /api/files/batch returns 3 files"  "$BATCH" "isinstance(d, list) and len(d)==3"
assert_json "batch entries share owner='Batch Bot'"  "$BATCH" "all(f['owner']=='Batch Bot' for f in d)"
assert_json "batch entries share project"            "$BATCH" "all(f['project']=='Q1-2026' for f in d)"
assert_json "batch detects multiple file types"      "$BATCH" "{f['file_type'] for f in d} >= {'txt','pdf','docx'}"

# Empty batch → 400
EMPTY_BATCH=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w "%{http_code}" -X POST "$BASE/api/files/batch" -F "system_id=sys_hr")
if [[ "$EMPTY_BATCH" == "400" ]]; then pass "batch without files → 400"
else fail "empty batch" "expected 400, got $EMPTY_BATCH"; fi

# Cleanup
printf '%s' "$BATCH" | "$PY" -c "
import json, sys, subprocess
for f in json.load(sys.stdin):
    subprocess.run(['curl','-s','-o','/dev/null','-X','DELETE','$BASE/api/files/'+f['id']], check=False)
"
rm -f "$TMP_B1" "$TMP_B2" "$TMP_B3"

# ----------------------------------------------------------------------------
# 19. Share links (TOR 4.15.11)
# ----------------------------------------------------------------------------
# Seed data inserts file rows without bytes on disk, so we upload a real file
# first to exercise the download path end-to-end.
section "19. Share links"
TMP_S=$(mktemp -t filehub-share.XXXXXX); echo "share me" > "$TMP_S"
SHARE_UP=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_S;filename=shareable.txt" -F "system_id=sys_hr")
SHARE_FID=$(printf '%s' "$SHARE_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")

SHARE=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files/$SHARE_FID/share" \
  -H "Content-Type: application/json" \
  -d '{"created_by":"Anong K.","note":"for external review","expires_in_days":7}')
assert_json "POST /api/files/:id/share returns token"      "$SHARE" "isinstance(d.get('token'), str) and len(d['token']) >= 16"
assert_json "share response carries file metadata"         "$SHARE" "d['file']['id']=='$SHARE_FID'"
assert_json "share has expires_at"                         "$SHARE" "isinstance(d.get('expires_at'), str)"

TOKEN=$(printf '%s' "$SHARE" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['token'])")

META=$("$CURL" -s "$BASE/api/share/$TOKEN")
assert_json "GET /api/share/:token returns file"           "$META" "d['id']=='$SHARE_FID' and d['name']=='shareable.txt'"

DL_HEADERS=$("$CURL" -s -D - -o /dev/null "$BASE/api/share/$TOKEN/download" | tr -d '\r')
case "$DL_HEADERS" in
  *content-disposition:*shareable.txt*) pass "share download serves correct filename" ;;
  *) fail "share download filename" "$DL_HEADERS" ;;
esac

# Download bytes match upload
TMP_SD=$(mktemp -t filehub-sharedl.XXXXXX)
"$CURL" -s -o "$TMP_SD" "$BASE/api/share/$TOKEN/download"
if cmp -s "$TMP_S" "$TMP_SD"; then
  pass "share download bytes match upload (and decrypted)"
else
  fail "share download bytes" ""
fi

assert_status "GET /api/share/bogus → 404"          "404" GET "/api/share/bogus-token-xyz"
assert_status "GET /api/share/bogus/download → 404" "404" GET "/api/share/bogus-token-xyz/download"

"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$SHARE_FID"
rm -f "$TMP_S" "$TMP_SD"

# ----------------------------------------------------------------------------
# 20. Reports (TOR 4.15.15)
# ----------------------------------------------------------------------------
section "20. Reports"
R_CAT=$("$CURL" -s $AUTH "$BASE/api/reports/by-category")
assert_json "by-category returns array"               "$R_CAT" "isinstance(d, list) and len(d) >= 4"
assert_json "by-category rows have category/size"     "$R_CAT" "all('category' in r and 'file_count' in r and 'size_bytes' in r for r in d)"
assert_json "by-category sorted by size DESC"         "$R_CAT" "all(d[i]['size_bytes'] >= d[i+1]['size_bytes'] for i in range(len(d)-1))"

R_DAY=$("$CURL" -s $AUTH "$BASE/api/reports/by-time?bucket=day")
assert_json "by-time?bucket=day returns array"        "$R_DAY" "isinstance(d, list) and len(d) >= 1"
assert_json "by-time row has bucket+count"            "$R_DAY" "all('bucket' in r and isinstance(r['file_count'], int) for r in d)"

R_MONTH=$("$CURL" -s $AUTH "$BASE/api/reports/by-time?bucket=month")
assert_json "by-time?bucket=month works"              "$R_MONTH" "isinstance(d, list) and len(d) >= 1"

R_BAD=$("$CURL" -s -o /dev/null -w "%{http_code}" $AUTH "$BASE/api/reports/by-time?bucket=hour")
if [[ "$R_BAD" == "400" ]]; then pass "by-time invalid bucket → 400"
else fail "by-time bucket validation" "expected 400, got $R_BAD"; fi

# ----------------------------------------------------------------------------
# 21. Image caching headers + ETag 304 (TOR 4.15.16)
# ----------------------------------------------------------------------------
section "21. Cache headers"
# Upload a real image (PNG by extension only — bytes can be anything for the
# header test) and a non-image so we exercise both branches.
TMP_IMG=$(mktemp -t filehub-img.XXXXXX); echo "fake image bytes" > "$TMP_IMG"
TMP_PDF=$(mktemp -t filehub-pdf.XXXXXX); echo "fake pdf bytes"   > "$TMP_PDF"
IMG_UP=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_IMG;filename=photo.png"   -F "system_id=sys_hr")
PDF_UP=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_PDF;filename=memo.pdf"    -F "system_id=sys_hr")
IMG_ID=$(printf '%s' "$IMG_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
PDF_ID=$(printf '%s' "$PDF_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")

IMG_HEADERS=$("$CURL" -s -D - -o /dev/null $AUTH "$BASE/api/files/$IMG_ID/download" | tr -d '\r')
case "$IMG_HEADERS" in
  *cache-control:*public,*max-age=86400*) pass "image download has public+max-age cache header" ;;
  *) fail "image cache header" "$IMG_HEADERS" ;;
esac
case "$IMG_HEADERS" in
  *etag:*) pass "image download has ETag" ;;
  *) fail "image ETag missing" "$IMG_HEADERS" ;;
esac

NON_IMG=$("$CURL" -s -D - -o /dev/null $AUTH "$BASE/api/files/$PDF_ID/download" | tr -d '\r')
case "$NON_IMG" in
  *cache-control:*private,*no-store*) pass "non-image has private/no-store" ;;
  *) fail "non-image cache header" "$NON_IMG" ;;
esac

# ETag round-trip → 304
ETAG=$("$CURL" -s -D - -o /dev/null $AUTH "$BASE/api/files/$IMG_ID/download" | tr -d '\r' | awk -F': ' 'tolower($1)=="etag"{print $2}')
if [[ -n "$ETAG" ]]; then
  CODE304=$("$CURL" -s -o /dev/null -w "%{http_code}" -H "If-None-Match: $ETAG" $AUTH "$BASE/api/files/$IMG_ID/download")
  if [[ "$CODE304" == "304" ]]; then
    pass "If-None-Match returns 304"
  else
    fail "304 short-circuit" "got $CODE304 for etag $ETAG"
  fi
else
  fail "ETag captured" "no ETag header"
fi

"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$IMG_ID"
"$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$PDF_ID"
rm -f "$TMP_IMG" "$TMP_PDF"

# ----------------------------------------------------------------------------
# 22. Expanded file types (TOR 4.15.6)
# ----------------------------------------------------------------------------
section "22. File-type detection"
declare -a TYPECASES=("test.pptx pptx" "test.html html" "report.xml xml" "audio.wav wav" "note.txt txt")
for case in "${TYPECASES[@]}"; do
  fname=$(echo "$case" | awk '{print $1}')
  want=$(echo "$case" | awk '{print $2}')
  TMP_T=$(mktemp -t filehub-tp.XXXXXX); echo "type test" > "$TMP_T"
  TP_UP=$("$CURL" -s -b "$COOKIE_JAR" -X POST "$BASE/api/files" -F "file=@$TMP_T;filename=$fname" -F "system_id=sys_hr")
  TP_ID=$(printf '%s' "$TP_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['id'])")
  GOT=$(printf '%s' "$TP_UP" | "$PY" -c "import json,sys; print(json.load(sys.stdin)['file_type'])")
  if [[ "$GOT" == "$want" ]]; then
    pass "filename $fname → file_type=$want"
  else
    fail "filename $fname" "expected $want, got $GOT"
  fi
  "$CURL" -s -b "$COOKIE_JAR" -o /dev/null -X DELETE "$BASE/api/files/$TP_ID"
  rm -f "$TMP_T"
done

# ----------------------------------------------------------------------------
# 23. Phase H — bucket CRUD
# ----------------------------------------------------------------------------
section "23. Bucket CRUD (POST/PATCH/DELETE /api/systems)"

# Log in as admin — bucket CRUD is admin-only.  We swap the cookie jar back at
# the end so the rest of the script keeps using the editor account.
ADMIN_JAR="$(mktemp -t filehub-admin.XXXXXX)"
"$CURL" -s -o /dev/null -c "$ADMIN_JAR" -X POST "$BASE/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@acme.go.th","password":"admin123"}'

RAND="$(printf '%s' "$RANDOM-$$" | md5sum 2>/dev/null | head -c 6 || printf '%s' $$)"
NEW_BUCKET="qa-bucket-$RAND"
SYS_CREATE=$("$CURL" -s -b "$ADMIN_JAR" -X POST "$BASE/api/systems" \
  -H 'Content-Type: application/json' \
  -d "{\"name\":\"QA Bucket $RAND\",\"bucket\":\"$NEW_BUCKET\",\"tone\":\"emerald\",\"quota_bytes\":1073741824}")
NEW_SYS_ID=$(printf '%s' "$SYS_CREATE" | "$PY" -c "import json,sys;print(json.load(sys.stdin).get('id',''))")
assert_json "POST /api/systems returns new system" "$SYS_CREATE" "d.get('system_type') == 'shared' and d.get('quota_bytes') == 1073741824"

# editor cannot create
ED_CREATE_STATUS=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/systems" \
  -H 'Content-Type: application/json' -d '{"name":"nope"}')
if [[ "$ED_CREATE_STATUS" == "403" ]]; then pass "editor POST /api/systems → 403"; else fail "editor POST" "expected 403 got $ED_CREATE_STATUS"; fi

# duplicate bucket
DUP_STATUS=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/systems" \
  -H 'Content-Type: application/json' -d "{\"name\":\"dup\",\"bucket\":\"$NEW_BUCKET\"}")
if [[ "$DUP_STATUS" == "400" ]]; then pass "duplicate bucket → 400"; else fail "duplicate bucket" "expected 400 got $DUP_STATUS"; fi

# patch
SYS_PATCH=$("$CURL" -s -b "$ADMIN_JAR" -X PATCH "$BASE/api/systems/$NEW_SYS_ID" \
  -H 'Content-Type: application/json' -d '{"name":"QA Bucket renamed","quota_bytes":2147483648}')
assert_json "PATCH renames + raises quota" "$SYS_PATCH" "d.get('name') == 'QA Bucket renamed' and d.get('quota_bytes') == 2147483648"

# delete (allowed — bucket is empty)
DEL_STATUS=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X DELETE "$BASE/api/systems/$NEW_SYS_ID")
if [[ "$DEL_STATUS" == "204" ]]; then pass "DELETE empty system → 204"; else fail "DELETE empty system" "expected 204 got $DEL_STATUS"; fi

# delete on non-empty system → 400 (sys_hr has files seeded)
DEL_FULL_STATUS=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X DELETE "$BASE/api/systems/sys_hr")
if [[ "$DEL_FULL_STATUS" == "400" ]]; then pass "DELETE non-empty system → 400"; else fail "DELETE non-empty" "expected 400 got $DEL_FULL_STATUS"; fi

# ----------------------------------------------------------------------------
# 24. Phase I — personal drive
# ----------------------------------------------------------------------------
section "24. Personal drive (My Drive)"

PERS=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/personal-drive")
assert_json "personal drive owned by caller" "$PERS" "d.get('system_type') == 'personal' and d.get('owner_user_id') == 'usr_anong'"
assert_json "personal drive id is sys_personal_<uid>" "$PERS" "d.get('id') == 'sys_personal_usr_anong'"

SYS_LIST=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/systems")
assert_json "shared list excludes personal" "$SYS_LIST" "not any(s.get('system_type')=='personal' for s in d)"

SYS_LIST_WITH=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/systems?include_personal=true")
assert_json "include_personal=true reveals My Drive" "$SYS_LIST_WITH" "any(s.get('id')=='sys_personal_usr_anong' for s in d)"

# ----------------------------------------------------------------------------
# 25. Phase J — quota report + per-org/user caps
# ----------------------------------------------------------------------------
section "25. Per-scope quota"

QR=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/quota?system_id=sys_hr")
assert_json "quota report has workspace+system+user" "$QR" \
  "{s['scope'] for s in d['scopes']} >= {'workspace','system','user'}"

ORG_ID=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/orgs?system_id=sys_hr" | "$PY" -c "import json,sys;print(json.load(sys.stdin)[0]['id'])")
SET_ORG=$("$CURL" -s -b "$ADMIN_JAR" -X PATCH "$BASE/api/orgs/$ORG_ID/quota" \
  -H 'Content-Type: application/json' -d '{"quota_bytes":100}')
assert_json "PATCH org quota persisted"  "$SET_ORG" "d.get('quota_bytes') == 100"

TMP_BIG=$(mktemp -t filehub-big.XXXXXX); head -c 1024 /dev/urandom > "$TMP_BIG"
QC=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/files" \
  -F "file=@$TMP_BIG;filename=big.bin" -F "system_id=sys_hr" -F "org_id=$ORG_ID")
rm -f "$TMP_BIG"
if [[ "$QC" == "413" ]]; then pass "org quota blocks upload → 413"; else fail "org quota block" "expected 413 got $QC"; fi

"$CURL" -s -b "$ADMIN_JAR" -o /dev/null -X PATCH "$BASE/api/orgs/$ORG_ID/quota" \
  -H 'Content-Type: application/json' -d '{"quota_bytes":0}'

ED_PATCH_STATUS=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X PATCH "$BASE/api/orgs/$ORG_ID/quota" \
  -H 'Content-Type: application/json' -d '{"quota_bytes":1000}')
if [[ "$ED_PATCH_STATUS" == "403" ]]; then pass "editor PATCH org quota → 403"; else fail "editor quota patch" "expected 403 got $ED_PATCH_STATUS"; fi

# ----------------------------------------------------------------------------
# 26. Phase K — rotation policies + run
# ----------------------------------------------------------------------------
section "26. Rotation policies + manual run"

POLS=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/rotation/policies")
assert_json "rotation policies has workspace default" "$POLS" \
  "any(p.get('scope_type')=='workspace' for p in d)"

UPS=$("$CURL" -s -b "$ADMIN_JAR" -X POST "$BASE/api/rotation/policies" \
  -H 'Content-Type: application/json' \
  -d '{"scope_type":"system","scope_id":"sys_hr","keep_last_n_versions":2,"archive_after_days":0,"delete_after_days":0}')
assert_json "upsert system policy returns row" "$UPS" "d.get('scope_type') == 'system' and d.get('keep_last_n_versions') == 2"

BAD=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/rotation/policies" \
  -H 'Content-Type: application/json' \
  -d '{"scope_type":"workspace","scope_id":"sys_hr","keep_last_n_versions":0,"archive_after_days":0,"delete_after_days":0}')
if [[ "$BAD" == "400" ]]; then pass "workspace + scope_id → 400"; else fail "workspace + scope_id" "expected 400 got $BAD"; fi

ED_UPS=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/rotation/policies" \
  -H 'Content-Type: application/json' \
  -d '{"scope_type":"system","scope_id":"sys_hr","keep_last_n_versions":1,"archive_after_days":0,"delete_after_days":0}')
if [[ "$ED_UPS" == "403" ]]; then pass "editor POST policy → 403"; else fail "editor upsert" "expected 403 got $ED_UPS"; fi

RUN=$("$CURL" -s -b "$ADMIN_JAR" -X POST "$BASE/api/rotation/run")
assert_json "manual rotation run returns stats keys" "$RUN" \
  "set(d.keys()) == {'versions_pruned','files_archived','files_hard_deleted'}"

RUNS=$("$CURL" -s -b "$COOKIE_JAR" "$BASE/api/rotation/runs")
assert_json "rotation runs is array" "$RUNS" "isinstance(d, list) and len(d) >= 1"
assert_json "latest run has finished_at" "$RUNS" "d[0].get('finished_at') is not None"

# ----------------------------------------------------------------------------
# 27. Q1 — PATCH /api/workspace (workspace_config k/v)
# ----------------------------------------------------------------------------
section "27. Workspace config (PATCH /api/workspace)"

WS_INITIAL=$("$CURL" -s -b "$ADMIN_JAR" "$BASE/api/workspace")
assert_json "GET /api/workspace returns k/v map"  "$WS_INITIAL" "isinstance(d, dict) and 'storage_quota_bytes' in d"

WS_PATCHED=$("$CURL" -s -b "$ADMIN_JAR" -X PATCH "$BASE/api/workspace" \
  -H 'Content-Type: application/json' \
  -d '{"values":{"require_2fa_admins":"true","watermark_external":"false"}}')
assert_json "PATCH /api/workspace persisted new keys" "$WS_PATCHED" \
  "d.get('require_2fa_admins') == 'true' and d.get('watermark_external') == 'false'"

ED_PATCH_WS=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X PATCH "$BASE/api/workspace" \
  -H 'Content-Type: application/json' -d '{"values":{"foo":"bar"}}')
if [[ "$ED_PATCH_WS" == "403" ]]; then pass "editor PATCH /api/workspace → 403"; else fail "editor PATCH workspace" "expected 403 got $ED_PATCH_WS"; fi

# ----------------------------------------------------------------------------
# 28. Q2 — Members CRUD (GET/POST/PATCH /api/users)
# ----------------------------------------------------------------------------
section "28. Members CRUD"

MEMBERS=$("$CURL" -s -b "$ADMIN_JAR" "$BASE/api/users")
assert_json "GET /api/users includes admin/anong/viewer" "$MEMBERS" \
  "{m['id'] for m in d} >= {'usr_admin','usr_anong','usr_viewer'}"
assert_json "members rows expose used_bytes" "$MEMBERS" \
  "all('used_bytes' in m and 'quota_bytes' in m for m in d)"

RAND_USER="qa-$(printf '%s' "$RANDOM-$$" | md5sum 2>/dev/null | head -c 6 || printf '%s' $$)"
INVITE=$("$CURL" -s -b "$ADMIN_JAR" -X POST "$BASE/api/users" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$RAND_USER@acme.go.th\",\"display_name\":\"QA $RAND_USER\",\"password\":\"qa-pass-12345\",\"role\":\"viewer\",\"quota_bytes\":536870912}")
NEW_UID=$(printf '%s' "$INVITE" | "$PY" -c "import json,sys;print(json.load(sys.stdin).get('id',''))")
assert_json "POST /api/users creates viewer with 512MB cap" "$INVITE" \
  "d.get('role') == 'viewer' and d.get('quota_bytes') == 536870912"

DUP=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/users" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$RAND_USER@acme.go.th\",\"display_name\":\"dup\",\"password\":\"qa-pass-12345\"}")
if [[ "$DUP" == "400" ]]; then pass "duplicate email → 400"; else fail "duplicate email" "expected 400 got $DUP"; fi

PATCH_MEMBER=$("$CURL" -s -b "$ADMIN_JAR" -X PATCH "$BASE/api/users/$NEW_UID" \
  -H 'Content-Type: application/json' \
  -d '{"role":"editor","quota_bytes":1073741824}')
assert_json "PATCH promotes to editor + raises quota" "$PATCH_MEMBER" \
  "d.get('role') == 'editor' and d.get('quota_bytes') == 1073741824"

# Cannot downgrade the only active admin
DEMOTE=$("$CURL" -s -b "$ADMIN_JAR" -o /dev/null -w '%{http_code}' -X PATCH "$BASE/api/users/usr_admin" \
  -H 'Content-Type: application/json' -d '{"role":"viewer"}')
if [[ "$DEMOTE" == "400" ]]; then pass "demote last admin → 400"; else fail "demote last admin" "expected 400 got $DEMOTE"; fi

ED_INVITE=$("$CURL" -s -b "$COOKIE_JAR" -o /dev/null -w '%{http_code}' -X POST "$BASE/api/users" \
  -H 'Content-Type: application/json' \
  -d '{"email":"x@example.com","display_name":"x","password":"qa-pass-12345"}')
if [[ "$ED_INVITE" == "403" ]]; then pass "editor POST /api/users → 403"; else fail "editor POST users" "expected 403 got $ED_INVITE"; fi

rm -f "$ADMIN_JAR"

# ----------------------------------------------------------------------------
# Summary
# ----------------------------------------------------------------------------
echo
printf "${C_BOLD}Summary:${C_RESET} %d/%d passed" "$PASS" "$TOTAL"
if [[ "$FAIL" -gt 0 ]]; then
  printf "  ${C_RED}(%d failed)${C_RESET}\n" "$FAIL"
  exit 1
else
  printf "  ${C_GREEN}✓ all green${C_RESET}\n"
fi
