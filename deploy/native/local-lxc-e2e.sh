#!/usr/bin/env bash
#
# Prova local de ponta a ponta do chat ADR-004 em um LXC/VM descartavel.
#
# O que faz:
#   1. Cria um banco PostgreSQL descartavel e aplica as migrations.
#   2. Cria o primeiro admin pela CLI do host (sem senha em argumentos).
#   3. Sobe o Axum localmente em 127.0.0.1:3000.
#   4. Cria um canal de texto e um segundo usuario via API.
#   5. Roda os testes opt-in do worker de chat do Phase 0 contra o servidor real,
#      incluindo a troca de mensagens entre dois clientes.
#   6. Sobe o binario do Phase 0 sob Xvfb para provar que a interface abre.
#   7. Encerra o servidor e imprime o relatorio.
#
# Nao usa Azure. Nao toca em producao. As senhas sao geradas aqui e valem
# apenas para este banco descartavel.
#
# Como usar no LXC (usuario cordel, com sudo):
#
#   scp deploy/native/local-lxc-e2e.sh deploy/native/local-lxc-common.sh \
#       deploy/native/local-lxc-e2e.py cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   bash /tmp/local-lxc-e2e.sh
#
# Rode com "bash <arquivo>": o arquivo copiado por scp nao tem bit de execucao.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=local-lxc-common.sh
source "$SCRIPT_DIR/local-lxc-common.sh"

DB_NAME="cordel_identity_test_lxc_e2e_$(date +%s)"
ADMIN_LOGIN="e2e-admin"
MEMBER_LOGIN="e2e-member"
ADMIN_PASSWORD="disposable-e2e-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-e2e-member-$(openssl rand -hex 6)"
ORIGIN="http://127.0.0.1:3000"
LISTEN_ADDR="127.0.0.1:3000"
SERVER_PID=""
SUMMARY_FILE=""
SERVER_BIN=""
PHASE0_BIN=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    log "Encerrando o servidor local (pid $SERVER_PID)..."
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  [[ -n "$SUMMARY_FILE" ]] && rm -f "$SUMMARY_FILE"
  return 0
}
trap cleanup EXIT

summary_value() {
  python3 -c "import json;print(json.load(open('$SUMMARY_FILE'))['$1'])"
}

# -----------------------------------------------------------------------------
# 1. Checkout
# -----------------------------------------------------------------------------
if ! find_repo_dir; then
  fail "Nao foi possivel obter um checkout do Cordel. Defina CORDEL_REPO_DIR."
fi
cd "$REPO_DIR"
log "Usando repositorio em: $REPO_DIR"

AVAIL_GB="$(free_gb "$REPO_DIR")"
log "Espaco livre: ${AVAIL_GB} GB"
if [[ "${AVAIL_GB:-0}" -lt 6 ]]; then
  fail "Espaco insuficiente (${AVAIL_GB} GB). Rode o provisionamento antes."
fi

export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_INCREMENTAL=0
export PATH="$HOME/.cargo/bin:$PATH"

# O libwebrtc embarcado do Phase 0 exige clang 21. O provisionamento local o
# instala; sem ele, o build do Phase 0 falha, mas o servidor e o client/state
# continuam compilando com o compilador padrao.
if command -v clang-21 >/dev/null 2>&1; then
  export CC="clang-21"
  export CXX="clang++-21"
elif [[ -z "${CC:-}" ]]; then
  log "AVISO: clang-21 nao encontrado. O build do client/phase0 pode falhar."
  log "       Rode deploy/native/local-lxc-provision.sh antes deste script."
fi

# A porta precisa estar livre: caso contrario o script testaria um servidor
# antigo, ligado a outro banco.
if command -v ss >/dev/null 2>&1 && ss -ltn 2>/dev/null | grep -q ":$LISTEN_ADDR"; then
  fail "A porta $LISTEN_ADDR ja esta em uso. Encerre o processo anterior e rode de novo."
fi

# -----------------------------------------------------------------------------
# 2. Banco descartavel e migrations
# -----------------------------------------------------------------------------
log "Garantindo que o PostgreSQL esteja rodando..."
sudo systemctl start postgresql 2>/dev/null || sudo service postgresql start 2>/dev/null || true
sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'postgres';" >/dev/null 2>&1 || true

log "Criando banco descartavel: $DB_NAME"
sudo -u postgres psql -tc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1 \
  || sudo -u postgres createdb "$DB_NAME"

export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB_NAME"
export CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)"

log "Compilando o servidor..."
cargo build --manifest-path server/identity/Cargo.toml --locked
SERVER_BIN="$REPO_DIR/server/identity/target/debug/cordel-identity"

log "Aplicando migrations..."
DATABASE_URL="$DATABASE_URL" "$SERVER_BIN" migrate

# -----------------------------------------------------------------------------
# 3. Primeiro admin
# -----------------------------------------------------------------------------
# A CLI de producao le a senha no terminal de proposito, entao nao roda sem
# TTY. O example e o equivalente descartavel para um ambiente local efemero.
log "Criando o primeiro admin..."
cargo build --manifest-path server/identity/Cargo.toml --locked --example bootstrap_disposable_admin
CORDEL_DISPOSABLE_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
CORDEL_DISPOSABLE_ADMIN_LOGIN="$ADMIN_LOGIN" \
CORDEL_DISPOSABLE_COMMUNITY_NAME="E2E Community" \
DATABASE_URL="$DATABASE_URL" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

# -----------------------------------------------------------------------------
# 4. Servidor local
# -----------------------------------------------------------------------------
log "Subindo o servidor Axum local..."
CORDEL_LISTEN_ADDR="$LISTEN_ADDR" "$SERVER_BIN" serve >/tmp/cordel-e2e-server.log 2>&1 &
SERVER_PID=$!

ready=0
for _ in $(seq 1 60); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "$ORIGIN/v1/me" || true)"
  if [[ "$code" == "401" ]]; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != "1" ]]; then
  log "Servidor nao respondeu. Log:"
  tail -40 /tmp/cordel-e2e-server.log || true
  fail "O servidor local nao subiu."
fi
log "Servidor respondendo em $ORIGIN"

# -----------------------------------------------------------------------------
# 5. Canal e segundo usuario
# -----------------------------------------------------------------------------
log "Criando o canal de texto e o segundo usuario..."
SUMMARY_FILE="$(mktemp)"
python3 "$SCRIPT_DIR/local-lxc-e2e.py" \
  --origin "$ORIGIN" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD" >"$SUMMARY_FILE"

CHANNEL_ID="$(summary_value channel_id)"
ADMIN_TOKEN="$(summary_value admin_token)"
ADMIN_USER_ID="$(summary_value admin_user_id)"
MEMBER_TOKEN="$(summary_value member_token)"
log "Canal de texto: $CHANNEL_ID"

# -----------------------------------------------------------------------------
# 6. Testes opt-in do worker de chat
# -----------------------------------------------------------------------------
export CORDEL_PHASE0_TEST_ORIGIN="$ORIGIN"
export CORDEL_PHASE0_TEST_CHANNEL="$CHANNEL_ID"
export CORDEL_PHASE0_TEST_TOKEN="$ADMIN_TOKEN"
export CORDEL_PHASE0_TEST_USER_ID="$ADMIN_USER_ID"
export CORDEL_PHASE0_TEST_LOGIN="$ADMIN_LOGIN"
export CORDEL_PHASE0_TEST_PEER_TOKEN="$MEMBER_TOKEN"

log "Rodando o teste do worker de chat (conectar, enviar, receber, reconectar)..."
xvfb-run -a cargo test \
  --manifest-path client/phase0/Cargo.toml \
  --locked \
  --features local-smoke \
  -- \
  --ignored \
  --nocapture \
  the_worker_connects_sends_and_receives_a_message

log "Rodando o teste de troca entre dois clientes..."
xvfb-run -a cargo test \
  --manifest-path client/phase0/Cargo.toml \
  --locked \
  --features local-smoke \
  -- \
  --ignored \
  --nocapture \
  two_clients_exchange_messages_over_the_live_session

# -----------------------------------------------------------------------------
# 7. A interface do Phase 0 abre com o chat configurado
# -----------------------------------------------------------------------------
log "Verificando se a interface do Phase 0 abre..."
PHASE0_BIN="$REPO_DIR/client/phase0/target/release/cordel-phase0"
if [[ ! -x "$PHASE0_BIN" ]]; then
  log "Binario de release ausente; compilando..."
  cargo build --manifest-path client/phase0/Cargo.toml --locked --release
fi
set +e
CORDEL_API_ORIGIN="$ORIGIN" \
CORDEL_CHANNEL_ID="$CHANNEL_ID" \
  timeout 15 xvfb-run -a "$PHASE0_BIN" --fake-media >/tmp/cordel-e2e-ui.log 2>&1
ui_status=$?
set -e
if [[ "$ui_status" == "124" || "$ui_status" == "0" ]]; then
  log "PASS a interface do Phase 0 abriu e permaneceu em execucao"
else
  log "FALHA a interface do Phase 0 saiu com codigo $ui_status"
  tail -30 /tmp/cordel-e2e-ui.log || true
  fail "A interface do Phase 0 nao permaneceu em execucao."
fi

# -----------------------------------------------------------------------------
# 8. Relatorio
# -----------------------------------------------------------------------------
log "------------------------------------------------------------"
log "PASS prova local de ponta a ponta do chat ADR-004"
log "Banco: $DB_NAME"
log "Servidor: $ORIGIN"
log "Canal: $CHANNEL_ID"
log "------------------------------------------------------------"
