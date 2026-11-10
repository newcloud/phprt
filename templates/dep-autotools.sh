#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=__NAME__ version=__VERSION_VAR__ needs= optional= tools= upstream=none note=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC VAR VER URL_VAR URL SHA_VAR
  D="$(dep_dir_of __NAME__)"
  VAR=__VERSION_VAR__
  VER="${!VAR}"
  URL_VAR="${VAR%_VERSION}_URL"
  SHA_VAR="${VAR%_VERSION}_SHA256"
  URL="${!URL_VAR}"
  SIG="__NAME__=$VER"

  if [ "$FORCE" != 1 ] && stamp_ok __NAME__ "$SIG" && [ -d "$D" ]; then
    ok "__NAME__ 已安装，跳过"
    return 0
  fi

  ensure_clean_target __NAME__ "$D"
  TAR="$(basename "$URL")"
  fetch "$URL" "$DOWNLOADS/$TAR" "${!SHA_VAR:-}"

  SRC="$SRCDIR/__NAME__-$VER"
  rm -rf "$SRC"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"

  cd "$SRC"
  ./configure --prefix="$D" --enable-shared --enable-static
  make -j"${JOBS:-$(nproc)}"
  make install

  stamp_write __NAME__ "$SIG"
  ok "__NAME__ $VER -> $D"
}

recipe_main "$@"
