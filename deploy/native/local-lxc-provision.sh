#!/usr/bin/env bash
#
# Provisionamento local para validação do Cordel em LXC/VM (Ubuntu 24.04).
# Não usa Azure. Não insere senhas, tokens ou dados de produção.
#
# O que faz:
#   1. Instala dependências do sistema (Rust via rustup, PostgreSQL, clang, etc.).
#   2. Cria um banco PostgreSQL descartável chamado cordel_identity_test_*.
#   3. Gera um CORDEL_JWT_SECRET_HEX aleatório (apenas para este teste).
#   4. Roda os testes ignorados do server/identity e os testes de client/state
#      e client/phase0 contra o banco local.
#
# Como usar no LXC (como o usuário cordel, com sudo):
#
#   scp local-lxc-provision.sh cordel@172.16.1.16:/tmp/
#   ssh cordel@172.16.1.16
#   bash /tmp/local-lxc-provision.sh
#
# Rode com "bash <arquivo>". O arquivo copiado por scp normalmente não tem
# permissão de execução, e "./script.sh" falharia com "Permission denied".
#
# Se o repositório ainda não estiver clonado no LXC, o script clona
# automaticamente a partir do GitHub em um diretório gravável. Para forçar um
# caminho, exporte CORDEL_REPO_DIR antes de rodar.

set -euo pipefail

# -----------------------------------------------------------------------------
# Configurações
# -----------------------------------------------------------------------------
REPO_URL="https://github.com/Antorsrj/Cordel.git"
DEFAULT_REPO_DIR="$HOME/Projects/Cordel"
REPO_DIR="${CORDEL_REPO_DIR:-$DEFAULT_REPO_DIR}"
RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-1.99.1}"
DB_NAME="cordel_identity_test_lxc_$(date +%s)"

log() {
  printf '[provision] %s\n' "$*"
}

fail() {
  printf '[provision][ERRO] %s\n' "$*" >&2
  exit 1
}

# -----------------------------------------------------------------------------
# 0. Garantir que estamos dentro de um checkout do Cordel
# -----------------------------------------------------------------------------
clone_repo() {
  local target="$1"
  git clone "$REPO_URL" "$target" || return 1
}

find_repo_dir() {
  # 1. Caminho configurado explicitamente.
  if [[ -n "${CORDEL_REPO_DIR:-}" ]]; then
    if [[ -f "$CORDEL_REPO_DIR/server/identity/Cargo.toml" ]]; then
      REPO_DIR="$CORDEL_REPO_DIR"
      return 0
    fi
  fi

  # 2. Caminho padrão, se for um checkout válido.
  if [[ -f "$DEFAULT_REPO_DIR/server/identity/Cargo.toml" ]]; then
    REPO_DIR="$DEFAULT_REPO_DIR"
    return 0
  fi

  # 3. Qualquer checkout válido já presente no diretório do usuário.
  local candidate
  for candidate in "$HOME"/Cordel* "$HOME"/cordel* "$HOME"/Projects/Cordel*; do
    if [[ -f "$candidate/server/identity/Cargo.toml" ]]; then
      REPO_DIR="$candidate"
      return 0
    fi
  done

  # 4. Clonar. Escolhe um destino gravável e não existente.
  local target
  for target in "$DEFAULT_REPO_DIR" "$DEFAULT_REPO_DIR-main" "$HOME/cordel-checkout" "$HOME/Cordel"; do
    if [[ -e "$target" ]]; then
      if [[ ! -w "$target" ]]; then
        log "Diretório $target existe e não é gravável por este usuário; escolhendo outro destino."
        continue
      fi
      if [[ -z "$(ls -A "$target" 2>/dev/null)" ]]; then
        log "Clonando em $target..."
        if clone_repo "$target"; then
          REPO_DIR="$target"
          return 0
        fi
        continue
      fi
      log "Diretório $target já tem conteúdo; escolhendo outro destino."
      continue
    fi
    log "Clonando em $target..."
    if clone_repo "$target"; then
      REPO_DIR="$target"
      return 0
    fi
  done

  # 5. Último recurso: um diretório temporário, sempre gravável.
  local temp
  temp="$(mktemp -d "$HOME/cordel-checkout-XXXXXX")" || return 1
  log "Clonando em $temp..."
  clone_repo "$temp" || return 1
  REPO_DIR="$temp"
  return 0
}

if ! find_repo_dir; then
  fail "Não foi possível obter um checkout do Cordel. Defina CORDEL_REPO_DIR ou clone manualmente."
fi

cd "$REPO_DIR"
log "Usando repositório em: $REPO_DIR"
git -C "$REPO_DIR" rev-parse --is-inside-work-tree >/dev/null || fail "$REPO_DIR não é um repositório git válido."

# -----------------------------------------------------------------------------
# 1. Dependências do sistema (Ubuntu 24.04)
# -----------------------------------------------------------------------------
log "Atualizando índices do apt..."
sudo apt-get update -y

log "Instalando pacotes de sistema..."
sudo apt-get install -y \
  build-essential \
  curl \
  git \
  pkg-config \
  libssl-dev \
  libpq-dev \
  postgresql \
  postgresql-contrib \
  clang \
  llvm \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libxkbcommon-dev \
  libfontconfig1-dev \
  libfreetype6-dev \
  libpng-dev

# -----------------------------------------------------------------------------
# 2. Rust toolchain (alinhado com o CI/local: 1.99.1)
# -----------------------------------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
  log "Instalando Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUST_TOOLCHAIN"
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

rustup toolchain install "$RUST_TOOLCHAIN" 2>/dev/null || true
rustup default "$RUST_TOOLCHAIN"
export PATH="$HOME/.cargo/bin:$PATH"

log "Versão do Rust: $(rustc --version)"
log "Versão do Cargo: $(cargo --version)"

# -----------------------------------------------------------------------------
# 3. PostgreSQL: garantir que está rodando e criar banco de teste
# -----------------------------------------------------------------------------
log "Garantindo que o PostgreSQL esteja rodando..."
sudo systemctl start postgresql 2>/dev/null || sudo service postgresql start 2>/dev/null || true

log "Criando banco de teste descartável: $DB_NAME"
sudo -u postgres psql -tc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1 \
  || sudo -u postgres createdb "$DB_NAME"

export CORDEL_TEST_DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB_NAME"
export CORDEL_JWT_SECRET_HEX="$(openssl rand -hex 32)"
export CORDEL_LISTEN_ADDR="127.0.0.1:3000"

log "Definindo senha do usuário postgres para conexões locais de teste..."
sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'postgres';" >/dev/null

log "CORDEL_TEST_DATABASE_URL=$CORDEL_TEST_DATABASE_URL"

# -----------------------------------------------------------------------------
# 4. Testes do servidor (identity) — fluxos ignorados que precisam do banco
# -----------------------------------------------------------------------------
log "Rodando testes ignorados do server/identity (banco descartável)..."
cargo test \
  --manifest-path server/identity/Cargo.toml \
  --locked \
  --lib \
  -- \
  --ignored \
  --test-threads=1

# -----------------------------------------------------------------------------
# 5. Testes de client/state
# -----------------------------------------------------------------------------
log "Rodando testes de client/state..."
cargo test \
  --manifest-path client/state/Cargo.toml \
  --locked

# -----------------------------------------------------------------------------
# 6. Testes de client/phase0 (sem local-smoke por padrão; build rápido)
# -----------------------------------------------------------------------------
log "Rodando testes de client/phase0..."
cargo test \
  --manifest-path client/phase0/Cargo.toml \
  --locked

# -----------------------------------------------------------------------------
# 7. Build de release do Phase 0 (sem local-smoke)
# -----------------------------------------------------------------------------
log "Fazendo build de release do client/phase0..."
cargo build \
  --manifest-path client/phase0/Cargo.toml \
  --locked \
  --release

log "------------------------------------------------------------"
log "Provisionamento concluído com sucesso."
log "Banco de teste: $DB_NAME"
log "URL: $CORDEL_TEST_DATABASE_URL"
log "Para subir o servidor localmente:"
log "  cd $REPO_DIR/server/identity"
log "  DATABASE_URL=\$CORDEL_TEST_DATABASE_URL cargo run --locked -- migrate"
log "  DATABASE_URL=\$CORDEL_TEST_DATABASE_URL CORDEL_JWT_SECRET_HEX=\$CORDEL_JWT_SECRET_HEX CORDEL_LISTEN_ADDR=127.0.0.1:3000 cargo run --locked -- serve"
log "------------------------------------------------------------"
