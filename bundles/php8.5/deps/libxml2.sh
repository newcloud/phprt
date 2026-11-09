#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libxml2 version=LIBXML2_VERSION needs= optional=zlib tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 名字以数字结尾（libxml2），upstream_norm_version 会截成 "2"，这里自己剥前缀
versions() {
  upstream_fetch listing-libxml2-2.15 https://download.gnome.org/sources/libxml2/2.15/ |
    grep -oE 'libxml2-[0-9]+(\.[0-9]+)+\.tar\.xz' |
    sed -E 's/^libxml2-//; s/\.tar\.xz$//' | sort -u
}

# zlib 只在 builtin zlib 启用时编进 libxml2
needs() {
  [ "${ENABLE_ZLIB:-0}" = 1 ] && printf '%s\n' zlib
  return 0
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC CONFIGURE_HELP
  local -a ZLIB_ARG XML_OPTS
  D="$(dep_dir_of libxml2)"
  SIG="libxml2=$LIBXML2_VERSION;zlib=${ENABLE_ZLIB:-0}:${ZLIB_VERSION:-none}"

  if [ "$FORCE" != 1 ] && stamp_ok libxml2 "$SIG" && [ -f "$D/lib/libxml2.so" ]; then
    ok "libxml2 已安装，跳过"; return 0
  fi

  ensure_needs libxml2
  ensure_clean_target libxml2 "$D"

  TAR="$(basename "$LIBXML2_URL")"
  fetch "$LIBXML2_URL" "$DOWNLOADS/$TAR" "${LIBXML2_SHA256:-}"

  SRC="$SRCDIR/libxml2-$LIBXML2_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  ZLIB_ARG=(--without-zlib)
  [ "${ENABLE_ZLIB:-0}" = 1 ] && ZLIB_ARG=(--with-zlib="$(dep_dir_of zlib)")

  cd "$SRC"
  export PKG_CONFIG_PATH="$(dep_pc_path zlib)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  # libxml2 2.15 起删除了 LZMA 与 FTP 支持，参数也随之消失；按 configure 实际支持的传
  XML_OPTS=(--without-python --without-http)
  CONFIGURE_HELP="$(./configure --help 2>/dev/null || true)"
  case "$CONFIGURE_HELP" in *--with-lzma*) XML_OPTS+=(--without-lzma) ;; esac
  case "$CONFIGURE_HELP" in *--with-ftp*)  XML_OPTS+=(--without-ftp) ;; esac

  log "configure + make -j$JOBS"
  ./configure --prefix="$D" "${ZLIB_ARG[@]}" "${XML_OPTS[@]}" \
      > "$LOGS/libxml2-configure.log" 2>&1 \
    || { tail -30 "$LOGS/libxml2-configure.log"; die "libxml2 configure 失败"; }
  make -j"$JOBS" > "$LOGS/libxml2-make.log" 2>&1 \
    || { tail -20 "$LOGS/libxml2-make.log"; die "libxml2 make 失败"; }
  make install >> "$LOGS/libxml2-make.log" 2>&1

  stamp_write libxml2 "$SIG"
  ok "libxml2 $LIBXML2_VERSION -> $D"
}

recipe_main "$@"
