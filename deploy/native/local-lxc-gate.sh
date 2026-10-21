#!/usr/bin/env bash
#
# Prova local do gate de admissao de midia (ADR-006) com um proxy reverso
# real. Nao usa Azure e nao toca em producao.
#
# O que mede:
#   1. O Caddy fica na frente do caminho de sinalizacao do LiveKit.
#   2. Um cliente entra pela porta do Caddy com uma admissao valida.
#   3. Uma acao de moderacao avanca a geracao da sala.
#   4. O MESMO token, pela MESMA porta, e recusado com 403.
#   5. A sala da geracao anterior foi drenada.
#
# O passo 4 e o que fecha a lacuna do ADR-006: com o gate no caminho, um
# token ja emitido nao devolve a entrada a um membro moderado.
#
# Como usar no LXC (Ubuntu 24.04, como root ou com sudo):
#
#   scp deploy/native/local-lxc-gate.sh deploy/native/local-lxc-common.sh \
#       deploy/native/media-admission.py cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   sudo bash /tmp/local-lxc-gate.sh
#
# O script instala o Caddy se faltar, e para tudo o que ele iniciou ao
# terminar. Ele nao mexe no servico do sistema do Caddy.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_LIB=""
for candidate in \
  "$SCRIPT_DIR/local-lxc-common.sh" \
  "$HOME/local-lxc-common.sh" \
  "/tmp/local-lxc-common.sh"; do
  if [[ -f "$candidate" ]]; then
    COMMON_LIB="$candidate"
    break
  fi
done
if [[ -z "$COMMON_LIB" ]]; then
  printf '[gate][ERRO] local-lxc-common.sh nao encontrado.\n' >&2
  exit 1
fi
# shellcheck source=local-lxc-common.sh
source "$COMMON_LIB"

IMAGE="${CORDEL_LIVEKIT_IMAGE:-livekit/livekit-server:v1.13.10}"
CONTAINER="cordel-livekit-gate"
API_KEY="devkey"
API_SECRET="local-livekit-secret-with-32-bytes-minimum"
DB_NAME="cordel_identity_test_lxc_gate_$(date +%s)"
ADMIN_LOGIN="gate-admin"
MEMBER_LOGIN="gate-member"
ADMIN_PASSWORD="disposable-gate-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-gate-member-$(openssl rand -hex 6)"
API_PORT=3000
GATE_PORT=3001
PROXY_PORT=8080
SIGNAL_PORT=7880
WEBHOOK_PORT=8098
SERVER_PID=""
CADDY_PID=""
WORK_DIR=""

log() { printf '[gate] %s\n' "$*"; }
fail() { printf '[gate][ERRO] %s\n' "$*" >&2; exit 1; }

cleanup() {
  if [[ -n "$CADDY_PID" ]] && kill -0 "$CADDY_PID" 2>/dev/null; then
    kill "$CADDY_PID" 2>/dev/null || true
    wait "$CADDY_PID" 2>/dev/null || true
  fi
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  return 0
}
trap cleanup EXIT

pick_port() {
  local base="$1" port
  for port in $(seq "$base" $((base + 40))); do
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      echo "$port"
      return 0
    fi
    exec 3>&- 2>/dev/null || true
  done
  return 1
}

find_helper() {
  local candidate
  for candidate in \
    "$SCRIPT_DIR/media-admission.py" \
    "$HOME/media-admission.py" \
    "/tmp/media-admission.py" \
    "$REPO_DIR/deploy/native/media-admission.py"; do
    if [[ -f "$candidate" ]]; then
      HELPER="$candidate"
      return 0
    fi
  done
  return 1
}

# -----------------------------------------------------------------------------
# 1. Dependencias
# -----------------------------------------------------------------------------
if [[ $EUID -ne 0 ]]; then
  log "Este script precisa de root. Rode com sudo."
  exit 1
fi
export DEBIAN_FRONTEND=noninteractive
log "Instalando as dependencias do sistema..."
apt-get update -y >/dev/null
apt-get install -y --no-install-recommends \
  ca-certificates curl gnupg debian-keyring debian-archive-keyring apt-transport-https \
  postgresql >/dev/null

if ! command -v caddy >/dev/null 2>&1; then
  log "Instalando o Caddy pelo repositorio oficial..."
  install -d -m 0755 /usr/share/keyrings
  curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key |
    gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt \
    -o /etc/apt/sources.list.d/caddy-stable.list
  apt-get update -y >/dev/null
  apt-get install -y --no-install-recommends caddy >/dev/null
fi
log "Caddy: $(caddy version)"

# -----------------------------------------------------------------------------
# 2. Checkout e binarios
# -----------------------------------------------------------------------------
find_repo_dir || fail "Nao foi possivel obter um checkout do Cordel."
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"
find_helper || fail "media-admission.py nao encontrado."

log "Compilando o servidor e o probe de midia..."
cargo build --manifest-path server/identity/Cargo.toml --locked \
  --bin cordel-identity --example bootstrap_disposable_admin
cargo build --manifest-path client/media/Cargo.toml --locked --features sdk --bin probe

API_PORT="$(pick_port "$API_PORT")" || fail "Nenhuma porta livre para a API."
GATE_PORT="$(pick_port "$GATE_PORT")" || fail "Nenhuma porta livre para o gate."
PROXY_PORT="$(pick_port "$PROXY_PORT")" || fail "Nenhuma porta livre para o proxy."
SIGNAL_PORT="$(pick_port "$SIGNAL_PORT")" || fail "Nenhuma porta livre para o LiveKit."
WEBHOOK_PORT="$(pick_port "$WEBHOOK_PORT")" || fail "Nenhuma porta livre para o webhook."
log "Portas: api $API_PORT, gate $GATE_PORT, proxy $PROXY_PORT, sinal $SIGNAL_PORT"

# -----------------------------------------------------------------------------
# 3. LiveKit local
# -----------------------------------------------------------------------------
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
log "Subindo $IMAGE..."
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
# O cliente roda no proprio host, então ele alcanca os candidatos ICE do
# container diretamente: a prova nao precisa publicar a porta de midia.
docker run -d --rm --name "$CONTAINER" \
  -p "127.0.0.1:$SIGNAL_PORT:7880" \
  -v "$WORK_DIR/livekit.yaml:/etc/livekit.yaml" \
  "$IMAGE" --config /etc/livekit.yaml >/dev/null
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" && break
  sleep 1
done
curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" || fail "O LiveKit nao subiu."

# -----------------------------------------------------------------------------
# 4. Banco e servidor
# -----------------------------------------------------------------------------
log "Garantindo que o PostgreSQL esteja rodando..."
systemctl start postgresql 2>/dev/null || service postgresql start 2>/dev/null || true
su - postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres';\"" >/dev/null 2>&1 || true
log "Criando o banco descartavel: $DB_NAME"
su - postgres -c "psql -tc \"SELECT 1 FROM pg_database WHERE datname='$DB_NAME'\"" | grep -q 1 \
  || su - postgres -c "createdb $DB_NAME"
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB_NAME"

log "Aplicando migrations e criando o primeiro admin..."
"$REPO_DIR/server/identity/target/debug/cordel-identity" migrate
CORDEL_DISPOSABLE_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
CORDEL_DISPOSABLE_ADMIN_LOGIN="$ADMIN_LOGIN" \
CORDEL_DISPOSABLE_COMMUNITY_NAME="Gate Community" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

log "Subindo o Axum com o servico de midia e o gate..."
DATABASE_URL="$DATABASE_URL" \
CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)" \
CORDEL_LISTEN_ADDR="127.0.0.1:$API_PORT" \
CORDEL_MEDIA_GATE_ADDR="127.0.0.1:$GATE_PORT" \
CORDEL_LIVEKIT_API_KEY="$API_KEY" \
CORDEL_LIVEKIT_API_SECRET="$API_SECRET" \
CORDEL_LIVEKIT_URL="ws://127.0.0.1:$SIGNAL_PORT" \
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
for _ in $(seq 1 20); do
  curl -s -o /dev/null "http://127.0.0.1:$GATE_PORT/v1/livekit/authorize" && break
  curl -s -o /dev/null "http://127.0.0.1:$GATE_PORT/" && break
  sleep 1
done
log "Axum em $API_PORT e gate em $GATE_PORT"

# -----------------------------------------------------------------------------
# 5. O proxy reverso com o gate
# -----------------------------------------------------------------------------
cat >"$WORK_DIR/Caddyfile" <<EOF
# A prova local e em HTTP simples: o que importa e a ordem dos estratos, e nao
# o TLS. A implantacao de producao termina TLS no Caddy.
http://127.0.0.1:$PROXY_PORT {
    @livekit path /rtc /rtc/*
    handle @livekit {
        forward_auth 127.0.0.1:$GATE_PORT {
            uri /v1/livekit/authorize
            method GET
            header_up X-Cordel-Livekit-Token {query.access_token}
        }
        reverse_proxy 127.0.0.1:$SIGNAL_PORT
    }
    handle {
        reverse_proxy 127.0.0.1:$SIGNAL_PORT
    }
}
EOF
caddy validate --config "$WORK_DIR/Caddyfile" --adapter caddyfile >/dev/null \
  || fail "A configuracao do Caddy nao e valida."
log "Subindo o Caddy com o gate na frente do caminho de sinalizacao..."
caddy run --config "$WORK_DIR/Caddyfile" --adapter caddyfile >"$WORK_DIR/caddy.log" 2>&1 &
CADDY_PID=$!
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$PROXY_PORT/" && break
  sleep 1
done
curl -s -o /dev/null "http://127.0.0.1:$PROXY_PORT/" || { tail -20 "$WORK_DIR/caddy.log"; fail "O Caddy nao subiu."; }

# -----------------------------------------------------------------------------
# 6. Um membro, um canal de voz e uma admissao
# -----------------------------------------------------------------------------
ORIGIN="http://127.0.0.1:$API_PORT"
SUMMARY="$(python3 "$REPO_DIR/deploy/native/local-lxc-e2e.py" \
  --origin "$ORIGIN" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD")"
read -r COMMUNITY_ID ADMIN_TOKEN MEMBER_TOKEN MEMBER_USER_ID < <(
  printf '%s' "$SUMMARY" |
    python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["community_id"], d["admin_token"], d["member_token"], d["member_user_id"])'
)
VOICE_CHANNEL="$(curl -s -X POST "$ORIGIN/v1/communities/$COMMUNITY_ID/channels" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"kind":"voice","name":"team"}' |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])')"
log "Canal de voz: $VOICE_CHANNEL"

ADMISSION="$(python3 "$HELPER" --origin "$ORIGIN" --channel "$VOICE_CHANNEL" \
  --login "$MEMBER_LOGIN" --password "$MEMBER_PASSWORD")" \
  || fail "O membro nao recebeu admissao."
ROOM_TOKEN="$(cut -f2 <<<"$ADMISSION")"
ROOM="$(cut -f3 <<<"$ADMISSION")"
log "Admissao emitida para $ROOM"

# -----------------------------------------------------------------------------
# 7. O cliente entra pelo Caddy
# -----------------------------------------------------------------------------
PROBE="$REPO_DIR/client/media/target/debug/probe"
log "O cliente entra pelo proxy (porta $PROXY_PORT)..."
set +e
timeout 25 "$PROBE" join --url "ws://127.0.0.1:$PROXY_PORT" --token "$ROOM_TOKEN" \
  --seconds 6 --no-audio --share --synthetic-screen >"$WORK_DIR/join.log" 2>&1
join_status=$?
set -e
if grep -q "connected to ws://127.0.0.1:$PROXY_PORT" "$WORK_DIR/join.log"; then
  log "PASS o cliente entrou pelo proxy com uma admissao valida"
else
  sed 's/^/[gate]   /' "$WORK_DIR/join.log" | tail -10
  fail "O cliente nao entrou pelo proxy."
fi

# -----------------------------------------------------------------------------
# 8. A moderacao avanca a geracao
# -----------------------------------------------------------------------------
log "Aplicando uma acao de moderacao..."
GRANT_RAW="$(curl -s -X POST "$ORIGIN/v1/sessions/admin-grant" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d "{\"password\":\"$ADMIN_PASSWORD\"}")"
ADMIN_GRANT="$(printf '%s' "$GRANT_RAW" |
  python3 -c 'import json,sys; print(json.load(sys.stdin).get("admin_token",""))')"
[[ -n "$ADMIN_GRANT" ]] || fail "Nao foi possivel obter o grant de admin: $GRANT_RAW"
KICK_RAW="$(curl -s -X POST \
  "$ORIGIN/v1/communities/$COMMUNITY_ID/members/$MEMBER_USER_ID/kick" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_GRANT" \
  -d '{}')"
printf '%s' "$KICK_RAW" | grep -q "authorization_revision" \
  || fail "O kick nao foi aplicado: $KICK_RAW"
printf '%s' "$KICK_RAW" | grep -q '"media_drained":true' \
  || fail "A sala nao foi drenada: $KICK_RAW"
log "PASS a acao de moderacao avanca a geracao e drena a sala"

# -----------------------------------------------------------------------------
# 9. O mesmo token, pela mesma porta, e recusado
# -----------------------------------------------------------------------------
log "O mesmo token tenta entrar de novo pelo proxy..."
set +e
timeout 25 "$PROBE" join --url "ws://127.0.0.1:$PROXY_PORT" --token "$ROOM_TOKEN" \
  --seconds 4 --no-audio --share --synthetic-screen >"$WORK_DIR/refused.log" 2>&1
refused_status=$?
set -e
if grep -q "connected to ws://127.0.0.1:$PROXY_PORT" "$WORK_DIR/refused.log"; then
  sed 's/^/[gate]   /' "$WORK_DIR/refused.log" | tail -10
  fail "O token guardado ainda entrou: o gate nao esta no caminho."
fi
log "PASS o token guardado foi recusado pelo gate"
sed 's/^/[gate]   /' "$WORK_DIR/refused.log" | tail -3

# -----------------------------------------------------------------------------
# 10. Uma admissao nova entra
# -----------------------------------------------------------------------------
NEW_ADMISSION="$(python3 "$HELPER" --origin "$ORIGIN" --channel "$VOICE_CHANNEL" \
  --login "$MEMBER_LOGIN" --password "$MEMBER_PASSWORD" 2>/dev/null || true)"
if [[ -z "$NEW_ADMISSION" ]]; then
  log "PASS o membro moderado nao recebe mais admissao"
else
  log "AVISO o membro moderado ainda recebeu uma admissao"
fi

log "------------------------------------------------------------"
log "PASS prova local do gate de admissao de midia (ADR-006)"
log "O token guardado foi recusado depois da moderacao, pelo mesmo proxy."
log "------------------------------------------------------------"
