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

# O build do Phase 0 (LiveKit + WebRTC + Slint) é o maior consumidor de disco.
# Sem símbolos de debug e sem incremental, o target dir cai de vários GB para
# cerca de 1-2 GB. Define CORDEL_FULL_DEBUG=1 se quiser o build com debug.
if [[ "${CORDEL_FULL_DEBUG:-0}" == "1" ]]; then
  export CARGO_PROFILE_DEV_DEBUG=2
  export CARGO_PROFILE_RELEASE_DEBUG=2
  export CARGO_INCREMENTAL=1
else
  export CARGO_PROFILE_DEV_DEBUG=0
  export CARGO_PROFILE_RELEASE_DEBUG=0
  export CARGO_INCREMENTAL=0
fi

log() {
  printf '[provision] %s\n' "$*"
}

fail() {
  printf '[provision][ERRO] %s\n' "$*" >&2
  exit 1
}

free_gb() {
  df -BG --output=avail "$1" 2>/dev/null | tail -1 | tr -dc '0-9'
}

# -----------------------------------------------------------------------------
# 0. Garantir que estamos dentro de um checkout do Cordel
# -----------------------------------------------------------------------------
clone_repo() {
  local target="$1"
  git clone "$REPO_URL" "$target" || return 1
}

# Um diretório que já tem .git é reaproveitado: busca e alinha com origin/main.
refresh_repo() {
  local target="$1"
  [[ -d "$target/.git" ]] || return 1
  log "Reaproveitando o checkout existente em $target..."
  git -C "$target" fetch --quiet origin main || return 1
  git -C "$target" reset --quiet --hard origin/main || return 1
  return 0
}

is_checkout() {
  [[ -f "$1/server/identity/Cargo.toml" ]]
}

find_repo_dir() {
  # 1. Caminho configurado explicitamente.
  if [[ -n "${CORDEL_REPO_DIR:-}" ]]; then
    if is_checkout "$CORDEL_REPO_DIR"; then
      REPO_DIR="$CORDEL_REPO_DIR"
      return 0
    fi
    if [[ -d "$CORDEL_REPO_DIR/.git" ]]; then
      if refresh_repo "$CORDEL_REPO_DIR" && is_checkout "$CORDEL_REPO_DIR"; then
        REPO_DIR="$CORDEL_REPO_DIR"
        return 0
      fi
    fi
  fi

  # 2. Caminho padrão, se for um checkout válido.
  if is_checkout "$DEFAULT_REPO_DIR"; then
    REPO_DIR="$DEFAULT_REPO_DIR"
    return 0
  fi

  # 3. Qualquer checkout válido já presente no diretório do usuário.
  local candidate
  for candidate in "$HOME"/Cordel* "$HOME"/cordel* "$HOME"/Projects/Cordel*; do
    if is_checkout "$candidate"; then
      REPO_DIR="$candidate"
      return 0
    fi
  done

  # 4. Clone parcial existente: reaproveitar ou escolher outro destino.
  local target
  for target in "$DEFAULT_REPO_DIR" "$DEFAULT_REPO_DIR-main" "$HOME/cordel-checkout" "$HOME/Cordel"; do
    if [[ -d "$target/.git" ]]; then
      if refresh_repo "$target" && is_checkout "$target"; then
        REPO_DIR="$target"
        return 0
      fi
      log "Checkout em $target está incompleto; escolhendo outro destino."
      continue
    fi
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
      log "Diretório $target já tem conteúdo e não é um repositório; escolhendo outro destino."
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

AVAIL_GB="$(free_gb "$REPO_DIR")"
log "Espaço livre em $REPO_DIR: ${AVAIL_GB} GB"
if [[ "${AVAIL_GB:-0}" -lt 8 ]]; then
  log "AVISO: menos de 8 GB livres. O build do Phase 0 (LiveKit + WebRTC + Slint) pode falhar."
  log "       Aumente o disco do LXC ou exporte CORDEL_SKIP_PHASE0=1 para pular essa etapa."
fi

# -----------------------------------------------------------------------------
# 1. Dependências do sistema (Ubuntu 24.04)
# -----------------------------------------------------------------------------
log "Atualizando índices do apt..."
sudo apt-get update -y

log "Instalando pacotes de sistema..."
# A lista espelha .github/workflows/phase0-linux.yml, que é a referência
# autoritativa para o build Linux do Phase 0.
sudo apt-get install -y --no-install-recommends \
  build-essential \
  curl \
  git \
  pkg-config \
  libclang-dev \
  libssl-dev \
  libpq-dev \
  postgresql \
  postgresql-contrib \
  clang \
  libglib2.0-dev libpipewire-0.3-dev libgbm-dev libdrm-dev \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libfontconfig1-dev libfreetype6-dev \
  libgl1-mesa-dev libegl1-mesa-dev \
  libasound2-dev libpulse-dev \
  libx11-dev libxrandr-dev libxinerama-dev libxcursor-dev \
  libxi-dev libxext-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  xvfb

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
# 6. Testes de client/phase0 (build pesado: LiveKit + WebRTC + Slint)
# -----------------------------------------------------------------------------
PHASE0_GB="$(free_gb "$REPO_DIR")"
if [[ "${CORDEL_SKIP_PHASE0:-0}" == "1" ]]; then
  log "CORDEL_SKIP_PHASE0=1: pulando os testes e o build do client/phase0."
elif [[ "${PHASE0_GB:-0}" -lt 8 ]]; then
  log "AVISO: apenas ${PHASE0_GB} GB livres. Pulando client/phase0 para não esgotar o disco."
  log "       Rode depois com mais espaço, ou com CORDEL_SKIP_PHASE0=0."
else
  log "Rodando testes de client/phase0..."
  cargo test \
    --manifest-path client/phase0/Cargo.toml \
    --locked

  log "Fazendo build de release do client/phase0..."
  cargo build \
    --manifest-path client/phase0/Cargo.toml \
    --locked \
    --release
fi

# -----------------------------------------------------------------------------
# 7. Relatório de espaço
# -----------------------------------------------------------------------------
log "------------------------------------------------------------"
log "Espaço livre em $REPO_DIR: $(free_gb "$REPO_DIR") GB"
if [[ -d "$REPO_DIR/client/phase0/target" ]]; then
  log "Target do phase0 ocupa: $(du -sh "$REPO_DIR/client/phase0/target" 2>/dev/null | cut -f1)"
  log "Para liberar: rm -rf $REPO_DIR/client/phase0/target"
fi
log "Provisionamento concluído com sucesso."
log "Banco de teste: $DB_NAME"
log "URL: $CORDEL_TEST_DATABASE_URL"
log "Para subir o servidor localmente:"
log "  cd $REPO_DIR/server/identity"
log "  DATABASE_URL=\$CORDEL_TEST_DATABASE_URL cargo run --locked -- migrate"
log "  DATABASE_URL=\$CORDEL_TEST_DATABASE_URL CORDEL_JWT_SECRET_HEX=\$CORDEL_JWT_SECRET_HEX CORDEL_LISTEN_ADDR=127.0.0.1:3000 cargo run --locked -- serve"
log "------------------------------------------------------------"
