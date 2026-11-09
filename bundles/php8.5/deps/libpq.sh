#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libpq version=LIBPQ_VERSION needs= optional= tools=perlshim
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 只列 18.x；跨大版本（17→18）不属于安全更新
versions() {
  upstream_versions_listing https://www.postgresql.org/ftp/source/ \
    'v18\.[0-9]+(\.[0-9]+)?' | sed 's/^v//'
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC PERL_DIR
  D="$(dep_dir_of libpq)"
  SIG="libpq=$LIBPQ_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libpq "$SIG" && [ -f "$D/lib/libpq.so" ]; then
    ok "libpq 已安装，跳过"; return 0
  fi

  ensure_clean_target libpq "$D"

  TAR="$(basename "$LIBPQ_URL")"
  fetch "$LIBPQ_URL" "$DOWNLOADS/$TAR" "${LIBPQ_SHA256:-}"

  SRC="$SRCDIR/postgresql-$LIBPQ_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  # 生成 catalog 头的脚本要 FindBin 等 perl 核心模块；系统 perl 常缺
  PERL_DIR="$(dep_dir_of perlshim)"
  [ -f "$PERL_DIR/share/perl5core/FindBin.pm" ] || ensure_dep perlshim
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

  cd "$SRC"
  # 只要客户端库 libpq + pg_config，不编服务端
  log "configure（只装 libpq）"
  ./configure --prefix="$D" --without-readline --without-zlib \
    > "$LOGS/libpq-configure.log" 2>&1 \
    || { tail -30 "$LOGS/libpq-configure.log"; die "libpq configure 失败"; }

  log "make libpq + pg_config -j$JOBS"
  make -C src/interfaces/libpq -j"$JOBS" > "$LOGS/libpq-make.log" 2>&1 \
    || { tail -30 "$LOGS/libpq-make.log"; die "libpq make 失败"; }
  make -C src/interfaces/libpq install >> "$LOGS/libpq-make.log" 2>&1
  make -C src/bin/pg_config install >> "$LOGS/libpq-make.log" 2>&1 \
    || { tail -20 "$LOGS/libpq-make.log"; die "pg_config 安装失败"; }
  make -C src/include install >> "$LOGS/libpq-make.log" 2>&1

  [ -x "$D/bin/pg_config" ] || die "没装出 $D/bin/pg_config"
  [ -f "$D/lib/libpq.so" ] || die "没装出 $D/lib/libpq.so"

  stamp_write libpq "$SIG"
  ok "libpq $LIBPQ_VERSION -> $D"
}

recipe_main "$@"
