#!/usr/bin/env bash
# Stage 5 smoke gates — release-binary end-to-end verification for the
# multi-session pivot work (5-A through 0040 option A). Complements the
# unit/integration tests in `cargo test --workspace` by exercising the
# real bound port + the cookie auth flow + the binary wire codec, which
# the in-process router tests do not cover.
#
# Coverage (per docs/reports/0041-next-session-handover.md §2.3, §8.2):
#   5-2  POST /api/sessions/:name/terminals (5-D P2) — shape + default coords
#   5-3  cascade offset — second POST shifts (80,80) → (112,112)
#   5-4  implicit detach-on-reattach (5932d00) — cookie session switch
#   5-5  same-name reattach with same cookie → 409, lock preserved
#   5-1  WS handshake with cookie-only auth (D10 α)
#   5-6  WS catch-up 0x88 burst (0040 option A) — alive UUIDs re-emit
#   5-7  0x85 TERMINAL_DIED on POST /terminals/:id/kill
#
# Order matters: HTTP gates run first because WS close triggers the
# cookie-driven auto-release (ADR-0021 D6). WS gates run last so the
# attach state is not silently torn down mid-test. Between WS gates a
# re-attach is performed after polling `/api/sessions` for the previous
# release to land, avoiding the disconnect-vs-attach race.
#
# Run from repo root:
#   ./codebase/smoke/02_stage5.sh
# or with a custom port:
#   GTMUX_SMOKE_PORT=9998 ./codebase/smoke/02_stage5.sh

set -euo pipefail

PORT="${GTMUX_SMOKE_PORT:-9991}"
HOST="127.0.0.1:${PORT}"
SESSION_PRIMARY="${GTMUX_SMOKE_SESSION:-stage5}"
SESSION_OTHER="stage5other"
BIN="${GTMUX_SMOKE_BIN:-/Users/ws/Desktop/projects/gtmux/codebase/backend/target/release/gtmux}"
WORKDIR=$(mktemp -d -t gtmux-smoke-stage5-XXXX)

cleanup() {
  if [ -n "${SERVER_PID:-}" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

declare -a RESULTS=()
pass() { RESULTS+=("  PASS  $1"); echo "  PASS  $1"; }
fail() { RESULTS+=("  FAIL  $1"); echo "  FAIL  $1"; print_summary; exit 1; }

print_summary() {
  echo
  echo "=== Stage 5 smoke summary ==="
  for line in "${RESULTS[@]}"; do
    echo "$line"
  done
}

# Re-attach the primary session for the current cookie. The cookie's
# previous attach (if any) is released asynchronously by the WS
# disconnect consumer (ADR-0021 D6); poll `/api/sessions` for the
# release to land before issuing the new attach.
reattach_primary() {
  for _ in $(seq 1 40); do
    local active
    active=$(curl -fsS "http://$HOST/api/sessions" \
      -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
      | python3 -c "import sys,json; rs=json.load(sys.stdin); print([r for r in rs if r['name']=='$SESSION_PRIMARY'][0]['active'])" \
      2>/dev/null || echo "?")
    if [ "$active" = "False" ]; then
      break
    fi
    sleep 0.05
  done
  curl -fsS -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/attach" \
    -H "$HOSTH" -H "$COOKIEH" >/dev/null
}

if [ ! -x "$BIN" ]; then
  echo "[setup] release binary missing: $BIN"
  echo "[setup] build it with: (cd codebase/backend && cargo build --release --bin gtmux)"
  exit 2
fi

echo "[setup] workspace=$WORKDIR port=$PORT primary=$SESSION_PRIMARY"
# Isolate XDG_{CONFIG,STATE}_HOME inside the per-run tempdir so the
# file_open allowlist + audit log + token + pidfile don't collide with
# (or inherit from) the dev host's real `~/.config/gtmux` and
# `~/.local/state/gtmux`. Both the gtmux process and the rest of this
# smoke script read these vars, so export them for the shell as well.
export XDG_CONFIG_HOME="$WORKDIR/xdg-config"
export XDG_STATE_HOME="$WORKDIR/xdg-state"
env -u TMUX "$BIN" start \
  --session "$SESSION_PRIMARY" \
  --port "$PORT" \
  --workspace "$WORKDIR" \
  >"$WORKDIR/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 50); do
  if curl -sf "http://$HOST/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

STATE_DIR=${XDG_STATE_HOME:-$HOME/.local/state}
TOKEN_FILE="$STATE_DIR/gtmux/${SESSION_PRIMARY}.token"
[ -f "$TOKEN_FILE" ] || fail "setup: token file missing at $TOKEN_FILE"
TOKEN=$(cat "$TOKEN_FILE")
echo "[setup] token loaded (${#TOKEN} chars)"

HOSTH="Host: $HOST"
ORIGINH="Origin: http://$HOST"

# ── Pre-flight: create two sessions for the multi-session tests ──────────
for name in "$SESSION_PRIMARY" "$SESSION_OTHER"; do
  STATUS=$(curl -sfS -o /dev/null -w '%{http_code}' \
    -X POST "http://$HOST/api/sessions" \
    -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"$name\"}" || echo "ERR")
  case "$STATUS" in
    20[01]) ;;
    409) ;; # already exists — fine
    *) fail "setup: create session $name returned $STATUS" ;;
  esac
done

# ── Cookie login (D10 α path) — same cookie used throughout ─────────────
LOGIN_HDRS=$(curl -sS -D - -o /dev/null \
  -X POST "http://$HOST/auth/login" \
  -H "$HOSTH" -H "$ORIGINH" \
  -H "Content-Type: application/json" \
  -d "{\"token\":\"$TOKEN\"}")
COOKIE=$(echo "$LOGIN_HDRS" \
  | awk -F': ' '/^[Ss]et-[Cc]ookie: gtmux_auth=/ { sub(/;.*/,"",$2); print $2 }' \
  | head -1)
[ -n "$COOKIE" ] || fail "setup: cookie not issued from /auth/login"
echo "[setup] cookie=${COOKIE:0:40}…"
COOKIEH="Cookie: $COOKIE"

# Acquire the primary attach for the HTTP gates that follow.
curl -fsS -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/attach" \
  -H "$HOSTH" -H "$COOKIEH" >/dev/null

# ═════════════════════════════════════════════════════════════════════
#  HTTP-only gates (no WS — attach lock stays intact through these)
# ═════════════════════════════════════════════════════════════════════

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-2 — POST /api/sessions/:name/terminals (5-D P2 happy path)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-2: POST /terminals — first call returns default coords ────"
CREATE1=$(curl -fsS -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/terminals" \
  -H "$HOSTH" -H "$COOKIEH" -H "Content-Type: application/json" -d '{}')
echo "$CREATE1" | python3 -m json.tool
python3 - <<PY || fail "5-2: response shape mismatch"
import json, sys
d = json.loads("""$CREATE1""")
need = {"terminal_id", "pane_id", "x", "y", "w", "h"}
missing = need - d.keys()
assert not missing, f"missing keys: {missing}"
assert d["x"] == 80 and d["y"] == 80, f"first call must default to (80,80): {d}"
assert d["w"] == 720 and d["h"] == 420, f"default size must be 720x420: {d}"
assert isinstance(d["pane_id"], int) and d["pane_id"] > 0, d
PY
UUID1=$(python3 -c "import json; print(json.loads('''$CREATE1''')['terminal_id'])")
pass "5-2  POST /terminals returns {x:80,y:80,w:720,h:420} + pane_id"

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-3 — cascade offset on second POST (default + 32)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-3: PUT layout with first terminal, then POST → cascade (112,112) ────"
ETAG=$(curl -fsS -X GET "http://$HOST/api/sessions/$SESSION_PRIMARY/layout" \
  -H "$HOSTH" -H "$COOKIEH" -D - -o /dev/null \
  | awk '/^[Ee][Tt][Aa][Gg]:/ {print $2}' | tr -d '\r')
LAYOUT1=$(cat <<EOF
{
  "schema_version": 2,
  "groups": [],
  "items": [
    { "id": "$UUID1", "type": "terminal", "parent_id": null,
      "x": 80.0, "y": 80.0, "w": 720.0, "h": 420.0, "z": 0,
      "visibility": "visible", "locked": false, "minimized": false }
  ],
  "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 }
}
EOF
)
PUT_STATUS=$(curl -fsS -o /dev/null -w '%{http_code}' \
  -X PUT "http://$HOST/api/sessions/$SESSION_PRIMARY/layout" \
  -H "$HOSTH" -H "$COOKIEH" -H "Content-Type: application/json" \
  -H "If-Match: $ETAG" -d "$LAYOUT1")
[ "$PUT_STATUS" = "204" ] || fail "5-3: PUT layout returned $PUT_STATUS"

CREATE2=$(curl -fsS -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/terminals" \
  -H "$HOSTH" -H "$COOKIEH" -H "Content-Type: application/json" -d '{}')
echo "$CREATE2" | python3 -m json.tool
python3 - <<PY || fail "5-3: cascade offset mismatch"
import json
d = json.loads("""$CREATE2""")
assert d["x"] == 112 and d["y"] == 112, f"cascade must be max(x,y)+32 = (112,112) but got {d}"
PY
UUID2=$(python3 -c "import json; print(json.loads('''$CREATE2''')['terminal_id'])")
pass "5-3  Second POST applies cascade offset (80,80) → (112,112)"

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-4 — implicit detach-on-reattach (5932d00)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-4: same cookie attach $SESSION_OTHER → primary auto-detaches ────"
ATTACH_OTHER=$(curl -fsS -o /dev/null -w '%{http_code}' \
  -X POST "http://$HOST/api/sessions/$SESSION_OTHER/attach" \
  -H "$HOSTH" -H "$COOKIEH")
[ "$ATTACH_OTHER" = "200" ] || fail "5-4: attach $SESSION_OTHER returned $ATTACH_OTHER"
SESSIONS_BODY=$(curl -fsS -X GET "http://$HOST/api/sessions" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || fail "5-4: active-flag transition incorrect"
import json
rows = json.loads("""$SESSIONS_BODY""")
m = {r["name"]: r["active"] for r in rows if r["name"] in ("$SESSION_PRIMARY", "$SESSION_OTHER")}
assert m.get("$SESSION_PRIMARY") is False, f"primary must be inactive: {m}"
assert m.get("$SESSION_OTHER") is True, f"other must be active: {m}"
PY
pass "5-4  Cookie session switch auto-releases prior flock (active=false)"

# Restore primary attachment for the rest of the smoke.
curl -fsS -o /dev/null -X DELETE "http://$HOST/api/sessions/$SESSION_OTHER/attach" \
  -H "$HOSTH" -H "$COOKIEH"
curl -fsS -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/attach" \
  -H "$HOSTH" -H "$COOKIEH" >/dev/null

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-5 — same-name reattach with same cookie → 409
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-5: duplicate attach same cookie same session → 409 ────"
DUP_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  -X POST "http://$HOST/api/sessions/$SESSION_PRIMARY/attach" \
  -H "$HOSTH" -H "$COOKIEH")
[ "$DUP_STATUS" = "409" ] || fail "5-5: expected 409, got $DUP_STATUS"
# The lock is still held (idempotent no-op path). Verify via active flag.
ACTIVE=$(curl -fsS "http://$HOST/api/sessions" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
  | python3 -c "import sys,json; rs=json.load(sys.stdin); print([r for r in rs if r['name']=='$SESSION_PRIMARY'][0]['active'])")
[ "$ACTIVE" = "True" ] || fail "5-5: primary lost active after duplicate attach"
pass "5-5  Same-cookie same-session reattach returns 409 (no implicit release loop)"

# ═════════════════════════════════════════════════════════════════════
#  WS gates — each may trigger ADR-0021 D6 cookie-driven auto-release on
#  close. `reattach_primary` polls for the release to land, then
#  re-acquires the lock.
# ═════════════════════════════════════════════════════════════════════

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-1 — WS handshake with cookie-only auth (D10 α)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-1: WS handshake via cookie (no bearer) ────"
python3 - <<PY || fail "5-1: WS handshake failed (see /tmp/gtmux-smoke-stage5-ws1.log)"
import base64, os, socket, sys
key = base64.b64encode(os.urandom(16)).decode()
req = (
  "GET /ws HTTP/1.1\r\n"
  f"Host: $HOST\r\n"
  f"Origin: http://$HOST\r\n"
  "Upgrade: websocket\r\n"
  "Connection: Upgrade\r\n"
  f"Sec-WebSocket-Key: {key}\r\n"
  "Sec-WebSocket-Version: 13\r\n"
  "Sec-WebSocket-Protocol: gtmux.v1\r\n"
  f"Cookie: $COOKIE\r\n"
  "\r\n"
)
s = socket.create_connection(("127.0.0.1", $PORT))
s.settimeout(2)
s.sendall(req.encode())
buf = b""
while b"\r\n\r\n" not in buf:
    chunk = s.recv(4096)
    if not chunk: break
    buf += chunk
resp = buf.decode("latin1", "replace")
open("/tmp/gtmux-smoke-stage5-ws1.log", "w").write(resp)
low = resp.lower()
assert "101 switching protocols" in low, resp
assert "sec-websocket-protocol: gtmux.v1\r\n" in low, resp
sys.exit(0)
PY
pass "5-1  WS handshake accepts Cookie-only auth (D10 α)"
reattach_primary

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-6 — WS catch-up 0x88 burst for alive UUIDs (0040 option A)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-6: WS catch-up emits 0x88 for both alive UUIDs ────"
python3 - "$PORT" "$COOKIE" "$UUID1" "$UUID2" >/tmp/gtmux-smoke-stage5-ws6.log 2>&1 <<'PY' || { cat /tmp/gtmux-smoke-stage5-ws6.log; fail "5-6: catch-up frames missing 0x88"; }
import base64, json, os, socket, struct, sys, time

port = int(sys.argv[1]); cookie = sys.argv[2]
expect = set(sys.argv[3:])

key = base64.b64encode(os.urandom(16)).decode()
host = f"127.0.0.1:{port}"
req = (
  "GET /ws HTTP/1.1\r\n"
  f"Host: {host}\r\n"
  f"Origin: http://{host}\r\n"
  "Upgrade: websocket\r\n"
  "Connection: Upgrade\r\n"
  f"Sec-WebSocket-Key: {key}\r\n"
  "Sec-WebSocket-Version: 13\r\n"
  "Sec-WebSocket-Protocol: gtmux.v1\r\n"
  f"Cookie: {cookie}\r\n"
  "\r\n"
)
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(3)
s.sendall(req.encode())
buf = b""
while b"\r\n\r\n" not in buf:
    chunk = s.recv(4096)
    if not chunk:
        sys.stderr.write("handshake EOF\n"); sys.exit(1)
    buf += chunk
assert b" 101 " in buf.split(b"\r\n", 1)[0], buf[:120]
rest = buf.split(b"\r\n\r\n", 1)[1]

def read_frame(sock, carry):
    def need(n):
        nonlocal carry
        while len(carry) < n:
            chunk = sock.recv(65536)
            if not chunk: raise EOFError("socket closed")
            carry += chunk
    need(2)
    b0, b1 = carry[0], carry[1]
    opcode = b0 & 0x0F
    masked = (b1 & 0x80) != 0
    plen = b1 & 0x7F
    pos = 2
    if plen == 126:
        need(pos + 2); plen = struct.unpack(">H", carry[pos:pos+2])[0]; pos += 2
    elif plen == 127:
        need(pos + 8); plen = struct.unpack(">Q", carry[pos:pos+8])[0]; pos += 8
    if masked:
        need(pos + 4); mk = carry[pos:pos+4]; pos += 4
    need(pos + plen)
    payload = carry[pos:pos+plen]
    if masked:
        payload = bytes(b ^ mk[i % 4] for i, b in enumerate(payload))
    return opcode, payload, carry[pos+plen:]

seen_uuids = set()
saw_0x80 = False
deadline = time.monotonic() + 3.0
carry = rest
while time.monotonic() < deadline and seen_uuids != expect:
    try:
        opcode, payload, carry = read_frame(s, carry)
    except (socket.timeout, EOFError):
        break
    if opcode != 0x2 or len(payload) < 5:
        continue
    ftype = payload[0]
    inner_len = struct.unpack("<I", payload[1:5])[0]
    inner = payload[5:5+inner_len]
    if ftype == 0x80:
        saw_0x80 = True
    elif ftype == 0x88:
        try:
            body = inner[1:].decode("utf-8")
            obj = json.loads(body)
            seen_uuids.add(obj["terminal_id"])
        except Exception as e:
            sys.stderr.write(f"0x88 decode failed: {e!r} body={inner!r}\n")

sys.stderr.write(f"saw_0x80={saw_0x80} seen_uuids={seen_uuids} expected={expect}\n")
missing = expect - seen_uuids
if not saw_0x80:
    print("FAIL: 0x80 LAYOUT_CHANGED hello not seen")
    sys.exit(1)
if missing:
    print(f"FAIL: missing 0x88 for {missing}")
    sys.exit(1)
print("OK")
PY
pass "5-6  WS catch-up emits 0x80 + 0x88 burst for all alive UUIDs (0040 option A)"
reattach_primary

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-7 — 0x85 TERMINAL_DIED on POST /terminals/:id/kill
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-7: kill UUID2 → 0x85 TERMINAL_DIED arrives ────"
python3 - "$PORT" "$COOKIE" "$UUID2" "$HOST" "$TOKEN" >/tmp/gtmux-smoke-stage5-ws7.log 2>&1 || { cat /tmp/gtmux-smoke-stage5-ws7.log; fail "5-7: 0x85 TERMINAL_DIED not received"; } <<'PY'
import base64, http.client, json, os, socket, struct, sys, time, threading

port = int(sys.argv[1]); cookie = sys.argv[2]; target = sys.argv[3]
host = sys.argv[4]; token = sys.argv[5]

key = base64.b64encode(os.urandom(16)).decode()
req = (
  "GET /ws HTTP/1.1\r\n"
  f"Host: {host}\r\n"
  f"Origin: http://{host}\r\n"
  "Upgrade: websocket\r\n"
  "Connection: Upgrade\r\n"
  f"Sec-WebSocket-Key: {key}\r\n"
  "Sec-WebSocket-Version: 13\r\n"
  "Sec-WebSocket-Protocol: gtmux.v1\r\n"
  f"Cookie: {cookie}\r\n"
  "\r\n"
)
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(3)
s.sendall(req.encode())
buf = b""
while b"\r\n\r\n" not in buf:
    chunk = s.recv(4096)
    if not chunk: sys.exit(2)
    buf += chunk
rest = buf.split(b"\r\n\r\n", 1)[1]

def read_frame(sock, carry):
    def need(n):
        nonlocal carry
        while len(carry) < n:
            chunk = sock.recv(65536)
            if not chunk: raise EOFError()
            carry += chunk
    need(2)
    b0, b1 = carry[0], carry[1]
    opcode = b0 & 0x0F
    masked = (b1 & 0x80) != 0
    plen = b1 & 0x7F
    pos = 2
    if plen == 126:
        need(pos+2); plen = struct.unpack(">H", carry[pos:pos+2])[0]; pos += 2
    elif plen == 127:
        need(pos+8); plen = struct.unpack(">Q", carry[pos:pos+8])[0]; pos += 8
    if masked:
        need(pos+4); mk = carry[pos:pos+4]; pos += 4
    need(pos+plen)
    payload = carry[pos:pos+plen]
    if masked:
        payload = bytes(b ^ mk[i % 4] for i, b in enumerate(payload))
    return opcode, payload, carry[pos+plen:]

def issue_kill():
    time.sleep(0.3)
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
    conn.request("POST", f"/api/terminals/{target}/kill", body=b"",
                 headers={"Host": host, "Authorization": f"Bearer {token}"})
    r = conn.getresponse()
    sys.stderr.write(f"[kill] status={r.status}\n")
    r.read()
    conn.close()

threading.Thread(target=issue_kill, daemon=True).start()

deadline = time.monotonic() + 4.0
carry = rest
while time.monotonic() < deadline:
    try:
        opcode, payload, carry = read_frame(s, carry)
    except (socket.timeout, EOFError):
        break
    if opcode != 0x2 or len(payload) < 5:
        continue
    ftype = payload[0]
    inner_len = struct.unpack("<I", payload[1:5])[0]
    inner = payload[5:5+inner_len]
    if ftype != 0x85:
        continue
    try:
        body = inner[1:].decode("utf-8")
        obj = json.loads(body)
        if obj.get("terminal_id") == target and obj.get("reason") in ("killed", "exit"):
            print(f"OK reason={obj['reason']}")
            sys.exit(0)
    except Exception as e:
        sys.stderr.write(f"0x85 decode failed: {e!r}\n")

print("FAIL: no matching 0x85 within timeout")
sys.exit(1)
PY
pass "5-7  POST /terminals/:id/kill emits 0x85 TERMINAL_DIED with matching UUID"

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-8 — Slice D-1 Settings API (ADR-0020 D11 minimal)
# ─────────────────────────────────────────────────────────────────────
echo
echo "──── gate 5-8: GET /api/settings + PATCH behavior + boot-immutable 400 ────"
GET_BODY=$(curl -fsS "http://$HOST/api/settings" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || fail "5-8: GET shape mismatch"
import json
d = json.loads("""$GET_BODY""")
for section in ("build", "server", "behavior", "auth"):
    assert section in d, f"missing section {section}: {d}"
assert d["behavior"]["auto_kill_terminal_on_panel_close"] is False, d
assert d["server"]["bind"] == "127.0.0.1", d
assert d["server"]["port"] == $PORT, d
assert d["auth"]["argon2"]["t_cost"] == 3, d
PY

PATCH_BODY=$(curl -fsS -X PATCH "http://$HOST/api/settings" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"behavior":{"auto_kill_terminal_on_panel_close":true}}')
python3 - <<PY || fail "5-8: PATCH did not flip toggle"
import json
d = json.loads("""$PATCH_BODY""")
assert d["behavior"]["auto_kill_terminal_on_panel_close"] is True, d
PY

BAD_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  -X PATCH "http://$HOST/api/settings" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"server":{"port":1}}')
[ "$BAD_STATUS" = "400" ] || fail "5-8: boot-immutable PATCH expected 400, got $BAD_STATUS"

pass "5-8  GET/PATCH /api/settings + boot-immutable rejection (D-1 ship)"

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-9 — Slice D-2 file_path allowlist + check (ADR-0023)
# ─────────────────────────────────────────────────────────────────────
# Covers GET empty / POST canonicalize / GET non-empty / check allowed +
# denied / open denied without confirm. Skips the actual OS open spawn
# (5-7-equivalent end-to-end is not feasible on a smoke CI without
# launching a GUI handler). The wire validation here is the same code
# path the FE will trip in production.
echo
echo "──── gate 5-9: /api/file-path/* (allowlist + check + open denied) ────"

# GET empty.
FP_EMPTY=$(curl -fsS "http://$HOST/api/file-path/allowlist" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || fail "5-9: GET empty shape mismatch"
import json
d = json.loads("""$FP_EMPTY""")
assert isinstance(d["entries"], list) and d["entries"] == [], d
PY

# Prep a real directory + file to drive POST + check.
FP_DIR=$(mktemp -d -t gtmux-smoke-fp-XXXX)
FP_FILE_MD="$FP_DIR/spec.md"
FP_FILE_SH="$FP_DIR/payload.sh"
echo "# Spec" >"$FP_FILE_MD"
echo "#!/bin/sh" >"$FP_FILE_SH"

# POST allowlist entry (ext=md, prefix=$FP_DIR/).
POST_BODY=$(printf '{"ext":"md","prefix":"%s","label":"smoke"}' "$FP_DIR/")
FP_CREATED=$(curl -fsS -X POST "http://$HOST/api/file-path/allowlist" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d "$POST_BODY")
python3 - <<PY || { rm -rf "$FP_DIR"; fail "5-9: POST response shape mismatch"; }
import json
d = json.loads("""$FP_CREATED""")
assert d["ext"] == "md", d
assert d["prefix"].endswith("/"), d
assert d["label"] == "smoke", d
PY
# Re-capture the canonical prefix that the BE stored — macOS resolves
# `/tmp` to `/private/tmp` during canonicalize, so the original
# `$FP_DIR/` won't match for DELETE.
FP_PREFIX_CANON=$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['prefix'])" "$FP_CREATED")

# GET after POST → 1 entry.
FP_LIST=$(curl -fsS "http://$HOST/api/file-path/allowlist" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || { rm -rf "$FP_DIR"; fail "5-9: GET after POST mismatch"; }
import json
d = json.loads("""$FP_LIST""")
assert len(d["entries"]) == 1, d
PY

# allowlist-check on .md → allowed.
CHECK_OK=$(curl -fsS "http://$HOST/api/file-path/allowlist-check?path=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$FP_FILE_MD")" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || { rm -rf "$FP_DIR"; fail "5-9: .md check should be allowed"; }
import json
d = json.loads("""$CHECK_OK""")
assert d["allowed"] is True, d
assert d["matched_entry"]["ext"] == "md", d
PY

# allowlist-check on .sh (same prefix) → DENIED — the core ADR-0023 D2
# security invariant: the `*.sh` modal-bypass attack is prevented.
CHECK_SH=$(curl -fsS "http://$HOST/api/file-path/allowlist-check?path=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$FP_FILE_SH")" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
python3 - <<PY || { rm -rf "$FP_DIR"; fail "5-9: .sh check must be denied (ADR-0023 D2 guard)"; }
import json
d = json.loads("""$CHECK_SH""")
assert d["allowed"] is False, d
assert d["reason"] == "not_in_allowlist", d
PY

# POST /open without user_confirmed on the unmatched .sh → 403.
OPEN_BODY=$(printf '{"path":"%s","user_confirmed":false}' "$FP_FILE_SH")
OPEN_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  -X POST "http://$HOST/api/file-path/open" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d "$OPEN_BODY")
[ "$OPEN_STATUS" = "403" ] || { rm -rf "$FP_DIR"; fail "5-9: open unmatched without confirm expected 403, got $OPEN_STATUS"; }

# DELETE round-trip. Use the canonical prefix the BE returned, not the
# raw `$FP_DIR/` — see the comment after the POST above.
DEL_PREFIX=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$FP_PREFIX_CANON")
DEL_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  -X DELETE "http://$HOST/api/file-path/allowlist?ext=md&prefix=$DEL_PREFIX" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
[ "$DEL_STATUS" = "204" ] || { rm -rf "$FP_DIR"; fail "5-9: DELETE expected 204, got $DEL_STATUS"; }
DEL_STATUS_2=$(curl -sS -o /dev/null -w '%{http_code}' \
  -X DELETE "http://$HOST/api/file-path/allowlist?ext=md&prefix=$DEL_PREFIX" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
[ "$DEL_STATUS_2" = "404" ] || { rm -rf "$FP_DIR"; fail "5-9: double-DELETE expected 404, got $DEL_STATUS_2"; }

rm -rf "$FP_DIR"
pass "5-9  /api/file-path/* allowlist + check + open denial (ADR-0023 D2/D5)"

# ─────────────────────────────────────────────────────────────────────
#  Gate 5-10 — Slice D-3 Auth Stage 7 (ADR-0020 D4 + D12)
# ─────────────────────────────────────────────────────────────────────
# Covers:
#   * password rotation 의 503 (password_not_set — token mode 의 정상 분기)
#   * logout-all 의 caller-cookie 보존 + 다른 session revoke + 403 (bearer-only)
# Skip: real password rotation 의 happy path 는 password mode 진입 (config 변경
# + 재기동) 필요 — release-binary 가 token mode 로 시작했으므로 happy path 는
# unit test 로 보장. smoke 는 endpoint wire + auth invariant 만 검증.
echo
echo "──── gate 5-10: /api/settings/password (503 token mode) + /logout-all ────"

# Token mode 의 password 변경 시도 → 503 password_not_set
PW_STATUS=$(curl -sS -o /tmp/gtmux-smoke-stage5-pw.json -w '%{http_code}' \
  -X POST "http://$HOST/api/settings/password" \
  -H "$HOSTH" -H "$COOKIEH" -H 'Content-Type: application/json' \
  -d '{"current_password":"x","new_password":"newpw123"}')
[ "$PW_STATUS" = "503" ] || fail "5-10: password rotation in token mode expected 503, got $PW_STATUS"
python3 - <<PY || fail "5-10: password 503 body shape mismatch"
import json
d = json.loads(open("/tmp/gtmux-smoke-stage5-pw.json").read())
assert d["error"] == "password_not_set", d
PY

# Logout-all 의 happy path — caller 는 보존되어야 함. 따라서 후속 GET 가
# 통과해야. 검증: revoked_count + caller cookie 살아있음.
LA_BODY=$(curl -fsS -X POST "http://$HOST/api/settings/logout-all" \
  -H "$HOSTH" -H "$COOKIEH")
python3 - <<PY || fail "5-10: logout-all body shape mismatch"
import json
d = json.loads("""$LA_BODY""")
assert isinstance(d["revoked_count"], int), d
assert d["revoked_count"] >= 0, d
PY

# Caller cookie 가 여전히 valid — GET /api/settings 가 통과해야.
POST_LA_GET=$(curl -sS -o /dev/null -w '%{http_code}' \
  "http://$HOST/api/settings" -H "$HOSTH" -H "$COOKIEH")
[ "$POST_LA_GET" = "200" ] || fail "5-10: caller cookie revoked after logout-all (got $POST_LA_GET)"

# Bearer-only request → 403 session_cookie_required
LA_BEARER_STATUS=$(curl -sS -o /tmp/gtmux-smoke-stage5-la-bearer.json -w '%{http_code}' \
  -X POST "http://$HOST/api/settings/logout-all" \
  -H "$HOSTH" -H "Authorization: Bearer $TOKEN")
[ "$LA_BEARER_STATUS" = "403" ] || fail "5-10: logout-all bearer-only expected 403, got $LA_BEARER_STATUS"
python3 - <<PY || fail "5-10: 403 body shape mismatch"
import json
d = json.loads(open("/tmp/gtmux-smoke-stage5-la-bearer.json").read())
assert d["error"] == "session_cookie_required", d
PY

pass "5-10 /api/settings/password 503 + /logout-all caller-preserved + 403 bearer (ADR-0020 D12)"

# ─────────────────────────────────────────────────────────────────────
print_summary
echo
echo "──── ALL STAGE 5 GATES PASSED ────"
