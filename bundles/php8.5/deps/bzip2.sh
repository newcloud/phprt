#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=bzip2 version=BZIP2_VERSION needs= optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 名字本身以数字结尾（bzip2），upstream_norm_version 会在 bzip2- 处截断，
# 这里自己剥前缀
versions() {
  upstream_fetch listing-bzip2 https://sourceware.org/pub/bzip2/ |
    grep -oE 'bzip2-[0-9]+(\.[0-9]+)+\.tar\.gz' |
    sed -E 's/^bzip2-//; s/\.tar\.gz$//' | sort -u
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of bzip2)"
  SIG="bzip2=$BZIP2_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok bzip2 "$SIG" && [ -f "$D/lib/libbz2.so" ]; then
    ok "bzip2 已安装，跳过"; return 0
  fi

  ensure_clean_target bzip2 "$D"

  TAR="$(basename "$BZIP2_URL")"
  fetch "$BZIP2_URL" "$DOWNLOADS/$TAR" "${BZIP2_SHA256:-}"

  SRC="$SRCDIR/bzip2-$BZIP2_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  # bzip2 无 configure；共享库走 Makefile-libbz2_so（soname libbz2.so.1.0）
  log "make -f Makefile-libbz2_so -j$JOBS"
  make -f Makefile-libbz2_so -j"$JOBS" > "$LOGS/bzip2-make.log" 2>&1 \
    || { tail -20 "$LOGS/bzip2-make.log"; die "bzip2 make 失败"; }

  log "安装（install 目标不装库和头文件，手动 cp）"
  mkdir -p "$D/lib" "$D/include"
  cp -a libbz2.so.1.0.8 "$D/lib/"
  ln -sfn libbz2.so.1.0.8 "$D/lib/libbz2.so.1.0"
  ln -sfn libbz2.so.1.0.8 "$D/lib/libbz2.so"
  cp -a bzlib.h "$D/include/"

  stamp_write bzip2 "$SIG"
  ok "bzip2 $BZIP2_VERSION -> $D"
}

recipe_main "$@"
