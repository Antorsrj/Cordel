#!/usr/bin/env bash
#
# Funcoes compartilhadas pelos scripts locais de LXC/VM.
# Use com:  source "$(dirname "$0")/local-lxc-common.sh"
#
# Nao contem segredos. As senhas dos testes sao geradas e descartadas em
# tempo de execucao.

REPO_URL="${REPO_URL:-https://github.com/Antorsrj/Cordel.git}"
DEFAULT_REPO_DIR="${DEFAULT_REPO_DIR:-$HOME/Projects/Cordel}"

# The Rust toolchain lives in the user's cargo home when rustup installed it,
# and a non-login shell does not have it on the path. When the script runs as
# root, the toolchain of the first user that has one is used instead, with its
# own rustup home, so the build finds the same toolchain.
if [[ -d "$HOME/.cargo/bin" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
elif ! command -v cargo >/dev/null 2>&1; then
  for candidate in /home/*/.cargo/bin; do
    if [[ -x "$candidate/cargo" ]]; then
      export PATH="$candidate:$PATH"
      export RUSTUP_HOME="$(dirname "$candidate")/../.rustup"
      break
    fi
  done
fi

# The libwebrtc that LiveKit ships requires clang 21, the same rule the Linux
# CI follows. Without it the media build fails with a clear panic.
if command -v clang-21 >/dev/null 2>&1; then
  export CC="clang-21"
  export CXX="clang++-21"
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

is_checkout() {
  [[ -f "$1/server/identity/Cargo.toml" ]]
}

clone_repo() {
  local target="$1"
  git clone "$REPO_URL" "$target" || return 1
}

# Um diretorio que ja tem .git e reaproveitado: busca e alinha com origin/main.
# Um checkout com alteracoes locais e preservado, com um aviso.
refresh_repo() {
  local target="$1"
  [[ -d "$target/.git" ]] || return 1
  if [[ -n "$(git -C "$target" status --porcelain 2>/dev/null)" ]]; then
    log "ATENCAO: $target tem alteracoes locais; mantendo como esta."
    return 0
  fi
  log "Atualizando $target para origin/main..."
  git -C "$target" fetch --quiet origin main || return 1
  git -C "$target" reset --quiet --hard origin/main || return 1
  return 0
}

# Define REPO_DIR com um checkout valido e o alinha com origin/main.
use_checkout() {
  local target="$1"
  is_checkout "$target" || return 1
  refresh_repo "$target" || log "AVISO: nao foi possivel atualizar $target; usando como esta."
  REPO_DIR="$target"
  return 0
}

# Define REPO_DIR com um checkout valido. Tenta, em ordem: caminho explicito,
# caminho padrao, qualquer checkout valido no home, reaproveitamento de clones
# parciais, e por fim um diretorio temporario.
find_repo_dir() {
  local candidate target temp

  if [[ -n "${CORDEL_REPO_DIR:-}" ]]; then
    if use_checkout "$CORDEL_REPO_DIR"; then
      return 0
    fi
  fi

  if use_checkout "$DEFAULT_REPO_DIR"; then
    return 0
  fi

  for candidate in "$HOME"/Cordel* "$HOME"/cordel* "$HOME"/Projects/Cordel*; do
    if use_checkout "$candidate"; then
      return 0
    fi
  done

  for target in "$DEFAULT_REPO_DIR" "$DEFAULT_REPO_DIR-main" "$HOME/cordel-checkout" "$HOME/Cordel"; do
    if [[ -d "$target/.git" ]]; then
      if refresh_repo "$target" && is_checkout "$target"; then
        REPO_DIR="$target"
        return 0
      fi
      log "Checkout em $target esta incompleto; escolhendo outro destino."
      continue
    fi
    if [[ -e "$target" ]]; then
      if [[ ! -w "$target" ]]; then
        log "Diretorio $target existe e nao e gravavel por este usuario; escolhendo outro destino."
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
      log "Diretorio $target ja tem conteudo e nao e um repositorio; escolhendo outro destino."
      continue
    fi
    log "Clonando em $target..."
    if clone_repo "$target"; then
      REPO_DIR="$target"
      return 0
    fi
  done

  temp="$(mktemp -d "$HOME/cordel-checkout-XXXXXX")" || return 1
  log "Clonando em $temp..."
  clone_repo "$temp" || return 1
  REPO_DIR="$temp"
  return 0
}
