#!/usr/bin/env bash
#
# Prova de recuperacao de rede do Client 102: quanto tempo o chat leva para
# ficar utilizavel depois de uma interrupcao de rede.
#
# O que mede:
#   1. O cliente Phase 0 conecta e assina o canal.
#   2. A rede para de entregar os pacotes durante alguns segundos.
#   3. A rede volta.
#   4. Uma mensagem enviada depois da volta chega ao cliente.
#   5. O tempo decorrido entre a interrupcao e a mensagem recebida.
#
# Nao usa Azure, nao abre porta externa e nao toca em producao. A
# interrupcao e feita localmente com iptables, dentro do proprio host.
#
# Como usar no LXC:
#
#   scp deploy/native/local-lxc-network.sh deploy/native/local-lxc-common.sh \
#       deploy/native/media-admission.py cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   sudo bash /tmp/local-lxc-network.sh
#
# O script precisa de root porque o iptables e o namespace de rede exigem
# isso. Ele restaura as regras ao terminar.

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
  printf '[network][ERRO] local-lxc-common.sh nao encontrado.\n' >&2
  exit 1
fi
# shellcheck source=local-lxc-common.sh
source "$COMMON_LIB"

DB_NAME="cordel_identity_test_lxc_net_$(date +%s)"
ADMIN_LOGIN="net-admin"
MEMBER_LOGIN="net-member"
ADMIN_PASSWORD="disposable-net-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-net-member-$(openssl rand -hex 6)"
API_PORT=3000
READY_FILE="/tmp/cordel-network-ready"
OUTAGE_SECONDS="${CORDEL_OUTAGE_SECONDS:-8}"
SERVER_PID=""

log() { printf '[network] %s\n' "$*"; }
fail() { printf '[network][ERRO] %s\n' "$*" >&2; exit 1; }

cleanup() {
  # The rules are removed even when the proof fails halfway.
  iptables -D OUTPUT -p tcp --dport "$API_PORT" -j REJECT 2>/dev/null || true
  iptables -D INPUT -p tcp --dport "$API_PORT" -j REJECT 2>/dev/null || true
  rm -f "$READY_FILE" "$READY_FILE.outage"
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  return 0
}
trap cleanup EXIT

if [[ $EUID -ne 0 ]]; then
  fail "Este script precisa de root, porque a interrupcao usa iptables."
fi
command -v iptables >/dev/null 2>&1 || fail "iptables nao esta instalado."

# -----------------------------------------------------------------------------
# 1. Checkout e binarios
# -----------------------------------------------------------------------------
find_repo_dir || fail "Nao foi possivel obter um checkout do Cordel."
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"

# O store de credenciais do sistema usa o D-Bus, que precisa dos arquivos de
# desenvolvimento. O provisionamento ja os instala; aqui so se falta algo.
if ! pkg-config --exists dbus-1 2>/dev/null; then
  log "Instalando libdbus-1-dev..."
  apt-get install -y --no-install-recommends libdbus-1-dev >/dev/null
fi

log "Compilando o servidor e o cliente..."
cargo build --manifest-path server/identity/Cargo.toml --locked --bin cordel-identity \
  --example bootstrap_disposable_admin
cargo build --manifest-path client/phase0/Cargo.toml --locked --features local-smoke --tests

# -----------------------------------------------------------------------------
# 2. Banco e servidor
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
CORDEL_DISPOSABLE_COMMUNITY_NAME="Network Community" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

log "Subindo o Axum..."
DATABASE_URL="$DATABASE_URL" \
CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)" \
CORDEL_LISTEN_ADDR="127.0.0.1:$API_PORT" \
  "$REPO_DIR/server/identity/target/debug/cordel-identity" serve \
  >/tmp/cordel-network-server.log 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 30); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$API_PORT/v1/me" || true)"
  [[ "$code" == "401" ]] && break
  sleep 1
done
[[ "${code:-}" == "401" ]] || { tail -20 /tmp/cordel-network-server.log; fail "O Axum nao subiu."; }

# -----------------------------------------------------------------------------
# 3. Um canal e um membro
# -----------------------------------------------------------------------------
ORIGIN="http://127.0.0.1:$API_PORT"
SUMMARY="$(python3 "$REPO_DIR/deploy/native/local-lxc-e2e.py" \
  --origin "$ORIGIN" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD")"
read -r COMMUNITY_ID CHANNEL ADMIN_TOKEN MEMBER_TOKEN MEMBER_USER_ID < <(
  printf '%s' "$SUMMARY" |
    python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["community_id"], d["channel_id"], d["admin_token"], d["member_token"], d["member_user_id"])'
)
log "Canal de texto: $CHANNEL"

# -----------------------------------------------------------------------------
# 4. O cliente conecta e a rede para
# -----------------------------------------------------------------------------
rm -f "$READY_FILE"
log "O cliente conecta; a interrupcao comeca quando ele avisar que esta pronto."
CORDEL_PHASE0_TEST_ORIGIN="$ORIGIN" \
CORDEL_PHASE0_TEST_CHANNEL="$CHANNEL" \
CORDEL_PHASE0_TEST_TOKEN="$MEMBER_TOKEN" \
CORDEL_PHASE0_TEST_USER_ID="$MEMBER_USER_ID" \
CORDEL_PHASE0_TEST_LOGIN="$MEMBER_LOGIN" \
CORDEL_PHASE0_TEST_READY_FILE="$READY_FILE" \
  cargo test \
  --manifest-path client/phase0/Cargo.toml \
  --locked \
  --features local-smoke \
  -- \
  --ignored \
  --nocapture \
  the_worker_recovers_after_a_network_interruption \
  >/tmp/cordel-network-client.log 2>&1 &
CLIENT_PID=$!

for _ in $(seq 1 120); do
  [[ -f "$READY_FILE" ]] && break
  sleep 1
done
[[ -f "$READY_FILE" ]] || { cat /tmp/cordel-network-client.log; fail "O cliente nao avisou que conectou."; }
log "O cliente conectou. Cortando a rede por $OUTAGE_SECONDS segundos..."

iptables -I OUTPUT -p tcp --dport "$API_PORT" -j REJECT
iptables -I INPUT -p tcp --dport "$API_PORT" -j REJECT
# O instante da queda e o que o cliente usa para medir a recuperacao.
date +%s%3N >"$READY_FILE.outage"
sleep "$OUTAGE_SECONDS"
log "A rede voltou. Enviando a mensagem depois da interrupcao..."

# A mensagem vai pelo caminho normal: ela prova que o chat voltou a funcionar.
curl -s -o /dev/null -X POST "$ORIGIN/v1/channels/$CHANNEL/messages" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d "{\"client_message_id\":\"after-the-interruption\",\"body\":\"after the interruption\"}"

wait "$CLIENT_PID" || fail "O cliente nao recuperou a conexao."
cat /tmp/cordel-network-client.log | grep -E "recovery|test result" | sed 's/^/[network]   /'

log "------------------------------------------------------------"
log "PASS o chat voltou a funcionar depois da interrupcao de rede"
log "------------------------------------------------------------"
