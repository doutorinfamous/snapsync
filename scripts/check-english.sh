#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

readonly pattern='[áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ]|\b(não|nenhuma|informe|configurações|sincronizar|impressora|histórico|salvar|falhou|baixado|abrir|sair|pausar|sincronização|destino|arquivo|pasta|válido|erro|horas|dias|buscar|conectar|remover|limpar)\b'
readonly targets=(
  "src"
  "src-tauri/src"
  "src-tauri/tauri.conf.json"
  "src-tauri/Cargo.toml"
  "index.html"
)

if rg --ignore-case --line-number --glob '*.{html,json,rs,ts,vue,toml}' \
  "${pattern}" "${targets[@]}"; then
  echo "Portuguese text was found in English-only application sources." >&2
  exit 1
fi

echo "English-only source check passed."
