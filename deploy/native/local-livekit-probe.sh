#!/usr/bin/env bash
#
# Prova local do comportamento do LiveKit antes da fatia de admissao de midia
# do Server 005. Nao usa Azure e nao toca em producao.
#
# O que mede:
#   1. O servidor sobe com um segredo de API de 32+ caracteres.
#   2. Um token HS256 minotado em Rust entra na sala e publica microfone.
#   3. O webhook chega assinado por JWT e o corpo bruto fica preso pelo
#      claim sha256.
#   4. A API do servidor lista salas e participantes com um token admin
#      limitado a sala.
#   5. RemoveParticipant desconecta o cliente.
#   6. O MESMO token reentra depois da remocao e da exclusao da sala: e a
#      lacuna que o ADR-006 precisa medir.
#
# Como usar no LXC (usuario cordel, com sudo):
#
#   scp deploy/native/local-livekit-probe.sh \
#       deploy/native/livekit-webhook-capture.py cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   bash /tmp/local-livekit-probe.sh
#
# Rode com "bash <arquivo>": um arquivo copiado por scp nao tem bit de execucao.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_URL="${REPO_URL:-https://github.com/Antorsrj/Cordel.git}"
DEFAULT_REPO_DIR="${DEFAULT_REPO_DIR:-$HOME/Projects/Cordel}"
IMAGE="${CORDEL_LIVEKIT_IMAGE:-livekit/livekit-server:v1.13.10}"
CONTAINER="cordel-livekit-probe"
ROOM="voice:probe:1"
API_KEY="devkey"
API_SECRET="localtestsecretthatisatleast32charslong"
WEBHOOK_PORT="${CORDEL_LIVEKIT_WEBHOOK_PORT:-8098}"
SIGNAL_PORT="${CORDEL_LIVEKIT_SIGNAL_PORT:-7880}"
API_KEY="devkey"
API_SECRET="localtestsecretthatisatleast32charslong"
CAPTURE_PID=""

log() { printf '[livekit-probe] %s\n' "$*"; }
fail() { printf '[livekit-probe][ERRO] %s\n' "$*" >&2; exit 1; }

# Escolhe a primeira porta livre a partir da base, para o probe poder rodar
# ao lado de outro LiveKit local.
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

cleanup() {
  if [[ -n "$CAPTURE_PID" ]] && kill -0 "$CAPTURE_PID" 2>/dev/null; then
    kill "$CAPTURE_PID" 2>/dev/null || true
  fi
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  return 0
}
trap cleanup EXIT

is_checkout() { [[ -f "$1/server/identity/Cargo.toml" ]]; }

find_repo_dir() {
  local candidate target temp
  if [[ -n "${CORDEL_REPO_DIR:-}" ]] && is_checkout "$CORDEL_REPO_DIR"; then
    REPO_DIR="$CORDEL_REPO_DIR"
    return 0
  fi
  if is_checkout "$DEFAULT_REPO_DIR"; then
    REPO_DIR="$DEFAULT_REPO_DIR"
    return 0
  fi
  for candidate in "$HOME"/Cordel* "$HOME"/cordel* "$HOME"/Projects/Cordel*; do
    if is_checkout "$candidate"; then
      REPO_DIR="$candidate"
      return 0
    fi
  done
  for target in "$DEFAULT_REPO_DIR" "$DEFAULT_REPO_DIR-main" "$HOME/cordel-checkout"; do
    if [[ -e "$target" ]]; then
      continue
    fi
    git clone --quiet "$REPO_URL" "$target" || continue
    if is_checkout "$target"; then
      REPO_DIR="$target"
      return 0
    fi
  done
  temp="$(mktemp -d "$HOME/cordel-checkout-XXXXXX")" || return 1
  git clone --quiet "$REPO_URL" "$temp" || return 1
  REPO_DIR="$temp"
  return 0
}

mint_token() {
  local identity="$1" room="$2" admin="${3:-0}"
  CORDEL_LIVEKIT_API_KEY="$API_KEY" \
  CORDEL_LIVEKIT_API_SECRET="$API_SECRET" \
  CORDEL_LIVEKIT_IDENTITY="$identity" \
  CORDEL_LIVEKIT_ROOM="$room" \
  CORDEL_LIVEKIT_ADMIN="$admin" \
    "$REPO_DIR/server/identity/target/debug/examples/mint_livekit_token"
}

# -----------------------------------------------------------------------------
# 1. Checkout e binarios
# -----------------------------------------------------------------------------
find_repo_dir || fail "Nao foi possivel obter um checkout do Cordel."
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"

log "Compilando o minter de token e o probe de midia..."
cargo build --manifest-path server/identity/Cargo.toml --locked --example mint_livekit_token
cargo build --manifest-path client/media/Cargo.toml --locked --features sdk --bin probe

# -----------------------------------------------------------------------------
# 2. Servidor LiveKit local
# -----------------------------------------------------------------------------
command -v docker >/dev/null 2>&1 || fail "docker nao esta instalado."
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true

SIGNAL_PORT="$(pick_port "$SIGNAL_PORT")" || fail "Nenhuma porta livre para o LiveKit."
WEBHOOK_PORT="$(pick_port "$WEBHOOK_PORT")" || fail "Nenhuma porta livre para o webhook."
log "Usando as portas $SIGNAL_PORT (sinal) e $WEBHOOK_PORT (webhook)"

WORK_DIR="$(mktemp -d)"
cat >"$WORK_DIR/livekit.yaml" <<EOF
port: $SIGNAL_PORT
rtc:
  tcp_port: 7881
  port_range_start: 50000
  port_range_end: 50050
  use_external_ip: false
keys:
  $API_KEY: $API_SECRET
webhook:
  api_key: $API_KEY
  urls:
    - http://host.docker.internal:$WEBHOOK_PORT/webhook
EOF

log "Subindo $IMAGE..."
docker run -d --rm --name "$CONTAINER" \
  --add-host host.docker.internal:host-gateway \
  -p "127.0.0.1:$SIGNAL_PORT:7880" \
  -p "127.0.0.1:7881:7881" \
  -v "$WORK_DIR/livekit.yaml:/etc/livekit.yaml" \
  "$IMAGE" --config /etc/livekit.yaml >/dev/null
ready=0
for _ in $(seq 1 30); do
  if curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/"; then
    ready=1
    break
  fi
  sleep 1
done
[[ "$ready" == "1" ]] || { docker logs "$CONTAINER" | tail -20; fail "O LiveKit nao subiu."; }
log "LiveKit respondendo em 127.0.0.1:$SIGNAL_PORT"

# -----------------------------------------------------------------------------
# 3. Captura de webhook
# -----------------------------------------------------------------------------
CAPTURE="$SCRIPT_DIR/livekit-webhook-capture.py"
if [[ ! -f "$CAPTURE" ]]; then
  CAPTURE="$REPO_DIR/deploy/native/livekit-webhook-capture.py"
fi
[[ -f "$CAPTURE" ]] || fail "livekit-webhook-capture.py nao encontrado."
setsid python3 "$CAPTURE" "$WEBHOOK_PORT" >"$WORK_DIR/webhook.log" 2>&1 &
CAPTURE_PID=$!
sleep 2
log "Capturador de webhook ativo na porta $WEBHOOK_PORT"

# -----------------------------------------------------------------------------
# 4. Um cliente entra e publica
# -----------------------------------------------------------------------------
PARTICIPANT_TOKEN="$(mint_token "probe-user" "$ROOM")"
log "Entrando na sala $ROOM com um token minotado em Rust..."
JOIN_LOG="$WORK_DIR/join.log"
setsid "$REPO_DIR/client/media/target/debug/probe" join \
  --url "ws://127.0.0.1:$SIGNAL_PORT" \
  --token "$PARTICIPANT_TOKEN" \
  --seconds 20 >"$JOIN_LOG" 2>&1 &
JOIN_PID=$!
sleep 8

grep -q "published microphone" "$JOIN_LOG" \
  || { cat "$JOIN_LOG"; fail "O cliente nao publicou microfone."; }
log "PASS o cliente publicou uma faixa de microfone"

# -----------------------------------------------------------------------------
# 5. Assinatura do webhook
# -----------------------------------------------------------------------------
sleep 3
if grep -q "body_pinned=True signature_ok=True issuer_ok=True" "$WORK_DIR/webhook.log"; then
  log "PASS o webhook chegou assinado por JWT e o corpo ficou preso pelo sha256"
  grep "webhook:" "$WORK_DIR/webhook.log" | sed 's/^/[livekit-probe]   /'
else
  log "FALHA a verificacao do webhook"
  cat "$WORK_DIR/webhook.log"
  fail "O webhook nao passou na verificacao."
fi

# -----------------------------------------------------------------------------
# 6. API do servidor
# -----------------------------------------------------------------------------
ADMIN_TOKEN="$(mint_token "cordel-axum" "$ROOM" 1)"
api() {
  curl -s -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    "http://127.0.0.1:$SIGNAL_PORT/twirp/livekit.RoomService/$1" -d "$2"
}

LIST="$(api ListRooms '{}')"
echo "$LIST" | grep -q "\"name\":\"$ROOM\"" \
  || { echo "$LIST"; fail "ListRooms nao enxergou a sala."; }
log "PASS ListRooms enxerga a sala $ROOM"

PARTICIPANTS="$(api ListParticipants "{\"room\":\"$ROOM\"}")"
echo "$PARTICIPANTS" | grep -q "probe-user" \
  || { echo "$PARTICIPANTS"; fail "ListParticipants nao enxergou o cliente."; }
log "PASS ListParticipants enxerga o cliente"

# -----------------------------------------------------------------------------
# 7. Remocao e a lacuna do ADR-006
# -----------------------------------------------------------------------------
REMOVE="$(api RemoveParticipant "{\"room\":\"$ROOM\",\"identity\":\"probe-user\"}")"
sleep 2
grep -q "ParticipantRemoved" "$JOIN_LOG" \
  || { cat "$JOIN_LOG"; fail "O cliente nao foi desconectado pela remocao."; }
log "PASS RemoveParticipant desconectou o cliente"

DELETE_ROOM="$(api DeleteRoom "{\"room\":\"$ROOM\"}")"
log "PASS DeleteRoom removeu a sala"

log "MEDINDO a lacuna do ADR-006: o mesmo token reentra?"
REJOIN_LOG="$WORK_DIR/rejoin.log"
timeout 20 "$REPO_DIR/client/media/target/debug/probe" join \
  --url "ws://127.0.0.1:$SIGNAL_PORT" \
  --token "$PARTICIPANT_TOKEN" \
  --seconds 4 >"$REJOIN_LOG" 2>&1 || true
if grep -q "published microphone" "$REJOIN_LOG"; then
  log "RESULTADO a lacuna do ADR-006 e total: o mesmo token reentra e publica"
  log "            apos RemoveParticipant e DeleteRoom. Um token valido nao pode"
  log "            ser negado por nenhuma acao do servidor em LiveKit self-hosted."
else
  cat "$REJOIN_LOG"
  log "RESULTADO o token antigo NAO reentrou. Reverificar antes de assumir."
fi

log "------------------------------------------------------------"
log "Probe concluido. Config e logs temporarios em $WORK_DIR"
log "------------------------------------------------------------"
