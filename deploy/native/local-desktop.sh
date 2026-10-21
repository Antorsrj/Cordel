#!/usr/bin/env bash
#
# Prepara um servidor local para uma sessao interativa do Phase 0.
#
# Ele sobe o PostgreSQL num container, cria um banco descartavel, aplica as
# migrations, cria um admin e um segundo membro, e sobe o Axum em
# 127.0.0.1:3000. O cliente do Phase 0 aponta para ele.
#
# Como usar na maquina do desktop:
#
#   bash deploy/native/local-desktop.sh
#
# O script imprime o comando do cliente e as contas disposable. As senhas sao
# geradas aqui e valem apenas para este banco.

set -euo pipefail

DB_CONTAINER="cordel-desktop-pg"
DB_PORT="5432"
DB_NAME="cordel_identity_test_desktop_$(date +%s)"
ADMIN_LOGIN="desktop-admin"
MEMBER_LOGIN="desktop-member"
ADMIN_PASSWORD="disposable-desktop-admin-$(openssl rand -hex 6)"
MEMBER_PASSWORD="disposable-desktop-member-$(openssl rand -hex 6)"
API_PORT=3000
SERVER_PID=""
WORK_DIR=""

log() { printf '[desktop] %s\n' "$*"; }
fail() { printf '[desktop][ERRO] %s\n' "$*" >&2; exit 1; }

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
  return 0
}
trap cleanup EXIT

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_DIR"
log "Repositorio: $REPO_DIR"

command -v cargo >/dev/null 2>&1 || fail "cargo nao encontrado."
command -v docker >/dev/null 2>&1 || fail "docker nao encontrado."

# -----------------------------------------------------------------------------
# 1. PostgreSQL num container
# -----------------------------------------------------------------------------
log "Subindo o PostgreSQL num container..."
docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true
docker run -d --rm --name "$DB_CONTAINER" \
  -e POSTGRES_PASSWORD=postgres \
  -p "127.0.0.1:$DB_PORT:5432" \
  postgres:16-alpine >/dev/null
# A espera usa o proprio container, para nao depender do pg_isready do host.
for _ in $(seq 1 40); do
  docker exec "$DB_CONTAINER" pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
docker exec "$DB_CONTAINER" pg_isready -U postgres >/dev/null 2>&1 \
  || fail "O PostgreSQL nao subiu."

export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:$DB_PORT/$DB_NAME"
log "Criando o banco: $DB_NAME"
docker exec "$DB_CONTAINER" psql -U postgres -c "CREATE DATABASE $DB_NAME;" >/dev/null

# -----------------------------------------------------------------------------
# 2. Binarios
# -----------------------------------------------------------------------------
log "Compilando o servidor..."
cargo build --manifest-path server/identity/Cargo.toml --locked \
  --bin cordel-identity --example bootstrap_disposable_admin

log "Aplicando migrations e criando as contas..."
"$REPO_DIR/server/identity/target/debug/cordel-identity" migrate
CORDEL_DISPOSABLE_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
CORDEL_DISPOSABLE_ADMIN_LOGIN="$ADMIN_LOGIN" \
CORDEL_DISPOSABLE_COMMUNITY_NAME="Desktop Community" \
  "$REPO_DIR/server/identity/target/debug/examples/bootstrap_disposable_admin"

# -----------------------------------------------------------------------------
# 3. O servidor
# -----------------------------------------------------------------------------
log "Subindo o Axum em 127.0.0.1:$API_PORT..."
WORK_DIR="$(mktemp -d)"
DATABASE_URL="$DATABASE_URL" \
CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)" \
CORDEL_LISTEN_ADDR="127.0.0.1:$API_PORT" \
  "$REPO_DIR/server/identity/target/debug/cordel-identity" serve \
  >"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 30); do
  code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$API_PORT/v1/me" || true)"
  [[ "$code" == "401" ]] && break
  sleep 1
done
[[ "${code:-}" == "401" ]] || { tail -20 "$WORK_DIR/server.log"; fail "O Axum nao subiu."; }

# O segundo membro entra por um convite, como qualquer pessoa, e o helper
# cria o canal de texto.
log "Criando o segundo membro por convite e o canal de texto..."
SUMMARY="$(python3 "$REPO_DIR/deploy/native/local-lxc-e2e.py" \
  --origin "http://127.0.0.1:$API_PORT" \
  --admin-login "$ADMIN_LOGIN" \
  --admin-password "$ADMIN_PASSWORD" \
  --member-login "$MEMBER_LOGIN" \
  --member-password "$MEMBER_PASSWORD")" || fail "Nao foi possivel criar o membro."
CHANNEL_ID="$(printf '%s' "$SUMMARY" |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["channel_id"])')"

cat <<EOF

[desktop] Servidor pronto em http://127.0.0.1:$API_PORT
[desktop]
[desktop] Contas disposable (valem so para este banco):
[desktop]   admin  : $ADMIN_LOGIN / $ADMIN_PASSWORD
[desktop]   membro : $MEMBER_LOGIN / $MEMBER_PASSWORD
[desktop] Canal de texto: $CHANNEL_ID
[desktop]
[desktop] Para abrir o cliente do Phase 0:
[desktop]   CORDEL_API_ORIGIN="http://127.0.0.1:$API_PORT" \\
[desktop]   CORDEL_CHANNEL_ID="$CHANNEL_ID" \\
[desktop]   cargo run --manifest-path client/phase0/Cargo.toml --features local-smoke
[desktop]
[desktop] Para a voz: o LiveKit local e opcional. Sem ele, a entrada na sala de
[desktop] voz falha, e o resto do cliente funciona.
[desktop]
[desktop] Para encerrar o servidor: kill $SERVER_PID
[desktop] O banco fica no container $DB_CONTAINER.
EOF
