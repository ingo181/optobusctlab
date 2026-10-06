#!/usr/bin/env bash
# Headless-Screenshot des Frontends gegen fake_xport (siehe README.md).
#
# Startet fake_xport, octlab-server (--connection tcp) und den Hilfsserver,
# macht mit Headless-Firefox einen Screenshot in der gewünschten Größe und
# beendet danach GENAU die selbst gestarteten Prozesse - über ihre gemerkten
# PIDs, nie über Namensmuster (kein pkill -f: das kann fremde Prozesse oder
# die eigene Shell treffen).
set -euo pipefail

usage() {
    cat <<'EOF'
Aufruf: tools/screenshot/shot.sh [Optionen]

  --size BxH        Fenstergröße in px (Default 800x480)
  --mute ADR        Modul ADR in fake_xport stummschalten (mehrfach möglich)
  --tab NAME        URL-Fragment für die App, z.B. uebersicht -> /#uebersicht
                    (Tab-Auswahl beim Laden, Spec 0006)
  --out DATEI       Ziel-PNG (Default target/screenshots/frontend-BxH[-NAME].png)
  --wait-ms MS      Wartezeit, bis Firefox auslöst (Default 8000)
  --settle-ms MS    Wartezeit nach dem Serverstart, bevor Firefox startet,
                    damit der erste Poll-Zyklus durch ist (Default 3000)
  --no-build        cargo- und trunk-Build überspringen
  -h, --help        diese Hilfe

Ports: octlab-server 3000 (fest), fake_xport FAKE_PORT (Default 15077),
Hilfsserver HELPER_PORT (Default 18080). Alle drei müssen frei sein.
EOF
}

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tool_dir="$repo/tools/screenshot"
size="800x480"
mutes=()
tab=""
out=""
wait_ms=8000
settle_ms=3000
build=1
fake_port="${FAKE_PORT:-15077}"
helper_port="${HELPER_PORT:-18080}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --size) size="$2"; shift 2 ;;
        --mute) mutes+=(--mute "$2"); shift 2 ;;
        --tab) tab="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --wait-ms) wait_ms="$2"; shift 2 ;;
        --settle-ms) settle_ms="$2"; shift 2 ;;
        --no-build) build=0; shift ;;
        -h | --help) usage; exit 0 ;;
        *) echo "Unbekannte Option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ ! "$size" =~ ^([0-9]+)x([0-9]+)$ ]]; then
    echo "--size erwartet BxH, z.B. 800x480" >&2
    exit 2
fi
width="${BASH_REMATCH[1]}"
height="${BASH_REMATCH[2]}"
if [[ -n "$tab" && ! "$tab" =~ ^[a-z0-9-]+$ ]]; then
    echo "--tab erwartet Kleinbuchstaben, Ziffern, '-' (z.B. uebersicht)" >&2
    exit 2
fi
out="${out:-$repo/target/screenshots/frontend-${width}x${height}${tab:+-$tab}.png}"

for tool in firefox python3 curl cargo; do
    command -v "$tool" >/dev/null || { echo "$tool nicht gefunden" >&2; exit 1; }
done
for port in 3000 "$fake_port" "$helper_port"; do
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
        echo "Port $port ist belegt - läuft schon ein Server/Simulator?" >&2
        exit 1
    fi
done

# --- Aufräumen: nur die hier gestarteten PIDs -------------------------------
pids=()
work="$(mktemp -d)"
cleanup() {
    for pid in "${pids[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$work"
}
trap cleanup EXIT

start() { # start <logname> <befehl...>: im Hintergrund starten, PID merken
    local log="$work/$1.log"
    shift
    "$@" >"$log" 2>&1 &
    pids+=("$!")
}

# --- Bauen -------------------------------------------------------------------
cd "$repo"
if [[ "$build" == 1 ]]; then
    command -v trunk >/dev/null || { echo "trunk nicht gefunden (oder --no-build)" >&2; exit 1; }
    cargo build -q --example fake_xport -p octlab-transport
    cargo build -q -p octlab-server
    (cd apps/web && trunk build >"$work/trunk.log" 2>&1) || {
        cat "$work/trunk.log" >&2
        exit 1
    }
fi

# --- Starten -----------------------------------------------------------------
start fake_xport "$repo/target/debug/examples/fake_xport" "127.0.0.1:$fake_port" "${mutes[@]}"
sleep 0.5
start server "$repo/target/debug/octlab-server" --connection tcp --addr "127.0.0.1:$fake_port"
start helper python3 -I "$tool_dir/helper_server.py" "$helper_port"

for _ in $(seq 1 50); do
    curl -sf http://127.0.0.1:3000/health >/dev/null && break
    sleep 0.2
done
curl -sf http://127.0.0.1:3000/health >/dev/null || {
    echo "octlab-server antwortet nicht:" >&2
    cat "$work/server.log" >&2
    exit 1
}
sleep "$(awk "BEGIN { print $settle_ms / 1000 }")"

# --- Screenshot --------------------------------------------------------------
mkdir -p "$(dirname "$out")"
mkdir -p "$work/profile"
# Das Fragment der App muss in der URL der Hilfsseite als %23 kodiert sein -
# ein rohes '#' wäre das Fragment der Hilfsseite selbst.
app="http://localhost:3000/${tab:+%23$tab}"
url="http://127.0.0.1:$helper_port/?w=$width&h=$height&wait=$wait_ms&app=$app"
timeout 120 firefox --headless --no-remote --profile "$work/profile" \
    --window-size="$width,$height" --screenshot "$out" "$url" >"$work/firefox.log" 2>&1 || {
    echo "Firefox-Screenshot fehlgeschlagen:" >&2
    cat "$work/firefox.log" >&2
    exit 1
}

echo "Screenshot: $out (${width}x${height}, Tab-Fragment: ${tab:-keins}, stumm: ${mutes[*]:-keins})"
echo "Poll-Statistik: $(curl -s http://127.0.0.1:3000/api/poll)"
