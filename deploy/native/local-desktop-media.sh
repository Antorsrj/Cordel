#!/usr/bin/env bash
#
# Prepara um servidor local COM midia para uma sessao interativa do Phase 0.
#
# Ele sobe o PostgreSQL, o LiveKit e o Caddy em containers, roda o Axum com o
# servico de midia e o gate de admissao, e cria um canal de voz. O cliente
# entra pelo Caddy, ou seja, pelo mesmo caminho que a producao usa: o gate
# responde antes de o LiveKit aceitar a entrada.
#
# Como usar na maquina do desktop:
#
#   bash deploy/native/local-desktop-media.sh
#
# O script imprime o comando do cliente e as contas disposable.

set -euo pipefail

DB_CONTAINER="cordel-desktop-media-pg"
LIVEKIT_CONTAINER="cordel-desktop-livekit"
CADDY_CONTAINER="cordel-desktop-caddy"
IMAGE="${CORDEL_LIVEKIT_IMAGE:-livekit/livekit-server:v1.13.10}"
API_KEY="devkey"
API_SECRET="local-livekit-secret-with-32-bytes-minimum"
DB_NAME="cordel_identity_test_desktop_media_$(date +%s)"
ADMIN_LOGIN="desktop-admin"
MEMBER_LOGIN="desktop-member"
ADMIN_PASSWORD="disposable-desktop-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-desktop-member-$(openssl rand -hex 6)"
API_PORT=3000
GATE_PORT=3001
PROXY_PORT=8080
SIGNAL_PORT=7880
SERVER_PID=""
WORK_DIR=""

log() { printf '[media] %s\n' "$*"; }
fail() { printf '[media][ERRO] %s\n' "$*" >&2; exit 1; }

KEEP=0
[[ "${1:-}" == "--keep" ]] && KEEP=1

cleanup() {
  if [[ "$KEEP" == "1" ]]; then
    return 0
  fi
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  docker rm -f "$LIVEKIT_CONTAINER" "$CADDY_CONTAINER" >/dev/null 2>&1 || true
  return 0
}
trap cleanup EXIT

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"

command -v cargo >/dev/null 2>&1 || fail "cargo nao encontrado."
command -v docker >/dev/null 2>&1 || fail "docker nao encontrado."

# -----------------------------------------------------------------------------
# 1. PostgreSQL
# -----------------------------------------------------------------------------
log "Subindo o PostgreSQL..."
docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$DB_CONTAINER" \
  -e POSTGRES_PASSWORD=postgres \
  -p "127.0.0.1:5432:5432" \
  postgres:16-alpine >/dev/null
for _ in $(seq 1 40); do
  docker exec "$DB_CONTAINER" pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB_NAME"
docker exec "$DB_CONTAINER" psql -U postgres -c "CREATE DATABASE $DB_NAME;" >/dev/null

# -----------------------------------------------------------------------------
# 2. LiveKit
# -----------------------------------------------------------------------------
log "Subindo o LiveKit..."
WORK_DIR="$(mktemp -d)"
cat >"$WORK_DIR/livekit.yaml" <<EOF
port: 7880
rtc:
  tcp_port: 7881
  port_range_start: 50000
  port_range_end: 50050
  use_external_ip: false
keys:
  $API_KEY: $API_SECRET
EOF
docker rm -f "$LIVEKIT_CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$LIVEKIT_CONTAINER" \
  -p "127.0.0.1:$SIGNAL_PORT:7880" \
  -p "127.0.0.1:7881:7881" \
  -v "$WORK_DIR/livekit.yaml:/etc/livekit.yaml" \
  "$IMAGE" --config /etc/livekit.yaml >/dev/null
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" && break
  sleep 1
done
curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" || fail "O LiveKit nao subiu."

# -----------------------------------------------------------------------------
# 3. O proxy com o gate na frente do caminho de sinalizacao
# -----------------------------------------------------------------------------
cat >"$WORK_DIR/Caddyfile" <<EOF
{
    admin off
}
http://127.0.0.1:$PROXY_PORT {
    @livekit path /rtc /rtc/*
    handle @livekit {
        forward_auth 127.0.0.1:$GATE_PORT {
            uri /v1/livekit/authorize
            method GET
        }
        reverse_proxy 127.0.0.1:$SIGNAL_PORT
    }
    handle {
        reverse_proxy 127.0.0.1:$SIGNAL_PORT
    }
}
EOF
log "Subindo o Caddy com o gate..."
docker rm -f "$CADDY_CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$CADDY_CONTAINER" --network host \
  -v "$WORK_DIR/Caddyfile:/etc/caddy/Caddyfile" \
  caddy:2-alpine caddy run --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$PROXY_PORT/" && break
  sleep 1
done
curl -s -o /dev/null "http://127.0.0.1:$PROXY_PORT/" || fail "O Caddy nao subiu."

# -----------------------------------------------------------------------------
# 4. Binarios, migrations e contas
# -----------------------------------------------------------------------------
log "Compilando o servidor..."
cargo build --manifest-path server/identity/Cargo.toml --locked \
  --bin cordel-identity --example bootstrap_disposable_admin

log "Aplicando migrations e criando as contas..."
"$REPO_DIR/server/identity/target/debug/cordel-identity" migrate
CORDEL_DISPOSABLE_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
CORDEL_DISPOSABLE_ADMIN_LOGIN="$ADMIN_LOGIN" \
CORDEL_DISPOSABLE_COMMUNITY_NAME="Desktop Media Community" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

# -----------------------------------------------------------------------------
# 5. O Axum com o servico de midia e o gate
# -----------------------------------------------------------------------------
log "Subindo o Axum com a midia e o gate..."
DATABASE_URL="$DATABASE_URL" \
CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)" \
CORDEL_LISTEN_ADDR="127.0.0.1:$API_PORT" \
CORDEL_MEDIA_GATE_ADDR="127.0.0.1:$GATE_PORT" \
CORDEL_LIVEKIT_API_KEY="$API_KEY" \
CORDEL_LIVEKIT_API_SECRET="$API_SECRET" \
CORDEL_LIVEKIT_URL="ws://127.0.0.1:$PROXY_PORT" \
CORDEL_LIVEKIT_API_URL="http://127.0.0.1:$SIGNAL_PORT" \
  "$REPO_DIR/server/identity/target/debug/cordel-identity" serve \
  >"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 30); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$API_PORT/v1/me" || true)"
  [[ "$code" == "401" ]] && break
  sleep 1
done
[[ "${code:-}" == "401" ]] || { tail -20 "$WORK_DIR/server.log"; fail "O Axum nao subiu."; }

# -----------------------------------------------------------------------------
# 6. Um canal de texto e um canal de voz
# -----------------------------------------------------------------------------
SUMMARY="$(python3 "$REPO_DIR/deploy/native/local-lxc-e2e.py" \
  --origin "http://127.0.0.1:$API_PORT" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD")" || fail "Nao foi possivel criar o membro."
read -r COMMUNITY_ID TEXT_CHANNEL ADMIN_TOKEN < <(
  printf '%s' "$SUMMARY" |
    python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["community_id"], d["channel_id"], d["admin_token"])'
)
VOICE_CHANNEL="$(curl -s -X POST "http://127.0.0.1:$API_PORT/v1/communities/$COMMUNITY_ID/channels" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"kind":"voice","name":"team"}' |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])')"
[[ "$VOICE_CHANNEL" != "None" ]] || fail "Nao foi possivel criar o canal de voz."

cat <<EOF

[media] Servidor pronto em http://127.0.0.1:$API_PORT
[media] O cliente entra pelo Caddy em http://127.0.0.1:$PROXY_PORT, e o gate
[media] responde antes de o LiveKit aceitar a entrada.
[media]
[media] Contas disposable (valem so para este banco):
[media]   admin  : $ADMIN_LOGIN / $ADMIN_PASSWORD
[media]   membro : $MEMBER_LOGIN / $MEMBER_PASSWORD
[media] Canal de texto: $TEXT_CHANNEL
[media] Canal de voz : $VOICE_CHANNEL
[media]
[media] Para abrir o cliente do Phase 0:
[media]   CORDEL_API_ORIGIN="http://127.0.0.1:$API_PORT" \\
[media]   CORDEL_CHANNEL_ID="$TEXT_CHANNEL" \\
[media]   CORDEL_VOICE_CHANNEL_ID="$VOICE_CHANNEL" \\
[media]   cargo run --manifest-path client/phase0/Cargo.toml --features local-smoke
[media]
[media] Para encerrar: kill $SERVER_PID
[media] Os containers ficam: $DB_CONTAINER, $LIVEKIT_CONTAINER, $CADDY_CONTAINER
EOF
