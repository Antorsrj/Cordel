#!/usr/bin/env bash
#
# Prova local da admissao de midia do Server 005: dois clientes reais em uma
# sala de voz, com o token emitido pelo Axum. Nao usa Azure e nao toca em
# producao.
#
# O que mede:
#   1. O LiveKit local sobe e o Axum recebe a configuracao do servico.
#   2. Um membro de um canal de voz recebe uma admissao escopada.
#   3. Dois clientes entram na mesma sala com as suas adjudicacoes e cada um
#      recebe a faixa do outro.
#   4. Um membro moderado e recusado na admissao.
#
# Como usar no LXC (usuario cordel, com sudo):
#
#   scp deploy/native/local-lxc-media.sh deploy/native/local-lxc-common.sh \
#       deploy/native/media-admission.py cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   bash /tmp/local-lxc-media.sh
#
# Rode com "bash <arquivo>": um arquivo copiado por scp nao tem bit de execucao.

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
  printf '[media][ERRO] local-lxc-common.sh nao encontrado.\n' >&2
  exit 1
fi
# shellcheck source=local-lxc-common.sh
source "$COMMON_LIB"

IMAGE="${CORDEL_LIVEKIT_IMAGE:-livekit/livekit-server:v1.13.10}"
CONTAINER="cordel-livekit-media"
API_KEY="devkey"
API_SECRET="local-livekit-secret-with-32-bytes-minimum"
DB_NAME="cordel_identity_test_lxc_media_$(date +%s)"
ADMIN_LOGIN="media-admin"
MEMBER_LOGIN="media-member"
ADMIN_PASSWORD="disposable-media-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-media-member-$(openssl rand -hex 6)"
API_PORT=3000
SIGNAL_PORT=7880
WEBHOOK_PORT=8098
SERVER_PID=""
HELPER=""

log() { printf '[media] %s\n' "$*"; }
fail() { printf '[media][ERRO] %s\n' "$*" >&2; exit 1; }

cleanup() {
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
# 1. Checkout e binarios
# -----------------------------------------------------------------------------
find_repo_dir || fail "Nao foi possivel obter um checkout do Cordel."
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"
find_helper || fail "media-admission.py nao encontrado."

log "Compilando o servidor, o probe de midia e o exemplo de bootstrap..."
cargo build --manifest-path server/identity/Cargo.toml --locked --bin cordel-identity --example bootstrap_disposable_admin
cargo build --manifest-path client/media/Cargo.toml --locked --features sdk --bin probe

SIGNAL_PORT="$(pick_port "$SIGNAL_PORT")" || fail "Nenhuma porta livre para o LiveKit."
WEBHOOK_PORT="$(pick_port "$WEBHOOK_PORT")" || fail "Nenhuma porta livre para o webhook."
API_PORT="$(pick_port "$API_PORT")" || fail "Nenhuma porta livre para a API."
ORIGIN="http://127.0.0.1:$API_PORT"
log "Portas: api $API_PORT, sinal $SIGNAL_PORT, webhook $WEBHOOK_PORT"

# -----------------------------------------------------------------------------
# 2. LiveKit local
# -----------------------------------------------------------------------------
command -v docker >/dev/null 2>&1 || fail "docker nao esta instalado."
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
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
for _ in $(seq 1 30); do
  curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" && break
  sleep 1
done
curl -s -o /dev/null "http://127.0.0.1:$SIGNAL_PORT/" || fail "O LiveKit nao subiu."

# -----------------------------------------------------------------------------
# 3. Banco e servidor Axum
# -----------------------------------------------------------------------------
log "Garantindo que o PostgreSQL esteja rodando..."
sudo systemctl start postgresql 2>/dev/null || sudo service postgresql start 2>/dev/null || true
sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'postgres';" >/dev/null 2>&1 || true
log "Criando banco descartavel: $DB_NAME"
sudo -u postgres psql -tc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1 \
  || sudo -u postgres createdb "$DB_NAME"
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB_NAME"

log "Aplicando migrations e criando o primeiro admin..."
"$REPO_DIR/server/identity/target/debug/cordel-identity" migrate
CORDEL_DISPOSABLE_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
CORDEL_DISPOSABLE_ADMIN_LOGIN="$ADMIN_LOGIN" \
CORDEL_DISPOSABLE_COMMUNITY_NAME="Media Community" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

log "Subindo o Axum com a configuracao do LiveKit..."
DATABASE_URL="$DATABASE_URL" \
CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)" \
CORDEL_LISTEN_ADDR="127.0.0.1:$API_PORT" \
CORDEL_LIVEKIT_API_KEY="$API_KEY" \
CORDEL_LIVEKIT_API_SECRET="$API_SECRET" \
CORDEL_LIVEKIT_URL="ws://127.0.0.1:$SIGNAL_PORT" \
  "$REPO_DIR/server/identity/target/debug/cordel-identity" serve \
  >"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 30); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "$ORIGIN/v1/me" || true)"
  [[ "$code" == "401" ]] && break
  sleep 1
done
[[ "${code:-}" == "401" ]] || { tail -20 "$WORK_DIR/server.log"; fail "O Axum nao subiu."; }
log "Axum respondendo em $ORIGIN"

# -----------------------------------------------------------------------------
# 4. Um canal de voz e dois membros
# -----------------------------------------------------------------------------
SUMMARY="$(python3 "$REPO_DIR/deploy/native/local-lxc-e2e.py" \
  --origin "$ORIGIN" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD")"
# The LiveKit identity is the account id, so the proof compares account ids.
read -r COMMUNITY_ID ADMIN_TOKEN MEMBER_TOKEN ADMIN_USER_ID MEMBER_USER_ID < <(
  printf '%s' "$SUMMARY" |
    python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["community_id"], d["admin_token"], d["member_token"], d["admin_user_id"], d["member_user_id"])'
)

VOICE_RAW="$(curl -s -X POST "$ORIGIN/v1/communities/$COMMUNITY_ID/channels" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"kind":"voice","name":"team"}')"
VOICE_CHANNEL="$(printf '%s' "$VOICE_RAW" |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])' 2>/dev/null)" \
  || fail "Nao foi possivel criar o canal de voz: $VOICE_RAW"
[[ -n "$VOICE_CHANNEL" ]] || fail "Nao foi possivel criar o canal de voz: $VOICE_RAW"
log "Canal de voz: $VOICE_CHANNEL"

# -----------------------------------------------------------------------------
# 5. Admissoes
# -----------------------------------------------------------------------------
log "Pedindo uma admissao para cada membro..."
ADMIN_ADMISSION="$(python3 "$HELPER" --origin "$ORIGIN" --channel "$VOICE_CHANNEL" \
  --login "$ADMIN_LOGIN" --password "$ADMIN_PASSWORD")" \
  || fail "O admin nao recebeu admissao."
MEMBER_ADMISSION="$(python3 "$HELPER" --origin "$ORIGIN" --channel "$VOICE_CHANNEL" \
  --login "$MEMBER_LOGIN" --password "$MEMBER_PASSWORD")" \
  || fail "O membro nao recebeu admissao."

ADMIN_URL="$(cut -f1 <<<"$ADMIN_ADMISSION")"
ADMIN_TOKEN_ROOM="$(cut -f2 <<<"$ADMIN_ADMISSION")"
ADMIN_ROOM="$(cut -f3 <<<"$ADMIN_ADMISSION")"
MEMBER_URL="$(cut -f1 <<<"$MEMBER_ADMISSION")"
MEMBER_TOKEN_ROOM="$(cut -f2 <<<"$MEMBER_ADMISSION")"
MEMBER_ROOM="$(cut -f3 <<<"$MEMBER_ADMISSION")"
log "Admin entrou em $ADMIN_ROOM; membro em $MEMBER_ROOM"
[[ "$ADMIN_ROOM" == "$MEMBER_ROOM" ]] || fail "As duas admissoes apontam para salas diferentes."

# -----------------------------------------------------------------------------
# 6. Os dois clientes trocam midia
# -----------------------------------------------------------------------------
log "Dois clientes entram na sala..."
PROBE="$REPO_DIR/client/media/target/debug/probe"
setsid "$PROBE" join --url "$ADMIN_URL" --token "$ADMIN_TOKEN_ROOM" --seconds 40 \
  --no-audio --share --synthetic-screen \
  >"$WORK_DIR/admin.log" 2>&1 &
ADMIN_PID=$!
sleep 4
setsid "$PROBE" join --url "$MEMBER_URL" --token "$MEMBER_TOKEN_ROOM" --seconds 35 \
  --no-audio --share --synthetic-screen \
  >"$WORK_DIR/member.log" 2>&1 &
MEMBER_PID=$!
sleep 25

admin_saw_member=0
member_saw_admin=0
grep -q "participant joined: $MEMBER_USER_ID" "$WORK_DIR/admin.log" && admin_saw_member=1
# The client that joins later finds the others in the room itself, not in a
# ParticipantConnected event.
grep -q "participant already here: $ADMIN_USER_ID" "$WORK_DIR/member.log" && member_saw_admin=1

if [[ "$admin_saw_member" == "1" && "$member_saw_admin" == "1" ]]; then
  log "PASS os dois clientes se viram na mesma sala"
else
  log "FALHA os clientes nao se viram"
  sed 's/^/[media]   admin: /' "$WORK_DIR/admin.log" | tail -20
  sed 's/^/[media]   member: /' "$WORK_DIR/member.log" | tail -20
  fail "A prova de dois clientes nao passou."
fi

admin_track=0
member_track=0
grep -q "subscribed to a track from $MEMBER_USER_ID" "$WORK_DIR/admin.log" && admin_track=1
grep -q "subscribed to a track from $ADMIN_USER_ID" "$WORK_DIR/member.log" && member_track=1
if [[ "$admin_track" == "1" && "$member_track" == "1" ]]; then
  log "PASS os dois clientes receberam a faixa de tela do outro"
else
  log "FALHA uma faixa remota nao chegou"
  sed 's/^/[media]   admin: /' "$WORK_DIR/admin.log" | tail -20
  sed 's/^/[media]   member: /' "$WORK_DIR/member.log" | tail -20
  fail "A prova de faixa remota nao passou."
fi

admin_frame=0
member_frame=0
grep -q "received .* from $MEMBER_USER_ID" "$WORK_DIR/admin.log" && admin_frame=1
grep -q "received .* from $ADMIN_USER_ID" "$WORK_DIR/member.log" && member_frame=1
if [[ "$admin_frame" == "1" && "$member_frame" == "1" ]]; then
  log "PASS os dois clientes receberam o quadro de tela do outro"
else
  log "FALHA um quadro remoto nao chegou"
  sed 's/^/[media]   admin: /' "$WORK_DIR/admin.log" | tail -20
  sed 's/^/[media]   member: /' "$WORK_DIR/member.log" | tail -20
  fail "A prova de quadro remoto nao passou."
fi

kill "$ADMIN_PID" "$MEMBER_PID" 2>/dev/null || true

# -----------------------------------------------------------------------------
# 7. Um membro moderado e recusado
# -----------------------------------------------------------------------------
log "Moderando o membro e pedindo outra admissao..."
# A moderation route needs the account-bound admin grant, not the app token.
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

set +e
python3 "$HELPER" --origin "$ORIGIN" --channel "$VOICE_CHANNEL" \
  --login "$MEMBER_LOGIN" --password "$MEMBER_PASSWORD" >/dev/null 2>"$WORK_DIR/refused.log"
refused=$?
set -e
# The session check comes first, so a moderated member is usually refused at
# the login and never reaches the admission route.
if [[ "$refused" == "1" ]] && grep -q "refused login status=403" "$WORK_DIR/refused.log"; then
  log "PASS um membro moderado nem consegue uma sessao"
elif [[ "$refused" == "2" ]] && grep -q "code=forbidden" "$WORK_DIR/refused.log"; then
  log "PASS um membro moderado nao recebe admissao"
else
  cat "$WORK_DIR/refused.log"
  fail "Um membro moderado recebeu admissao."
fi

log "------------------------------------------------------------"
log "PASS prova local de admissao de midia do Server 005"
log "Sala: $ADMIN_ROOM"
log "------------------------------------------------------------"
