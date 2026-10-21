#!/usr/bin/env bash
#
# Funcoes compartilhadas pelos scripts locais de LXC/VM.
# Use com:  source "$(dirname "$0")/local-lxc-common.sh"
#
# Nao contem segredos. As senhas dos testes sao geradas e descartadas em
# tempo de execucao.

REPO_URL="${REPO_URL:-https://github.com/Antorsrj/Cordel.git}"
DEFAULT_REPO_DIR="${DEFAULT_REPO_DIR:-$HOME/Projects/Cordel}"

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
refresh_repo() {
  local target="$1"
  [[ -d "$target/.git" ]] || return 1
  log "Reaproveitando o checkout existente em $target..."
  git -C "$target" fetch --quiet origin main || return 1
  git -C "$target" reset --quiet --hard origin/main || return 1
  return 0
}

# Define REPO_DIR com um checkout valido. Tenta, em ordem: caminho explicito,
# caminho padrao, qualquer checkout valido no home, reaproveitamento de clones
# parciais, e por fim um diretorio temporario.
find_repo_dir() {
  local candidate target temp

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
