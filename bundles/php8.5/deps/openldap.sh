#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=openldap version=OPENLDAP_VERSION needs= optional=openssl tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

versions() {
  upstream_versions_listing https://www.openldap.org/software/download/OpenLDAP/openldap-release/ \
    'openldap-[0-9]+(\.[0-9]+)+\.tgz'
}

# TLS 只在 openssl 启用时编进去；关掉只是少了 ldaps://
needs() {
  [ "${ENABLE_OPENSSL:-0}" = 1 ] && printf '%s\n' openssl
  return 0
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  local -a OPTS
  D="$(dep_dir_of openldap)"
  SIG="openldap=$OPENLDAP_VERSION;openssl=${ENABLE_OPENSSL:-0}:${OPENSSL_VERSION:-none}"

  if [ "$FORCE" != 1 ] && stamp_ok openldap "$SIG" && [ -f "$D/lib/libldap.so" ]; then
    ok "OpenLDAP 已安装，跳过"; return 0
  fi

  ensure_needs openldap
  dep_env openldap
  ensure_clean_target openldap "$D"

  TAR="$(basename "$OPENLDAP_URL")"
  fetch "$OPENLDAP_URL" "$DOWNLOADS/$TAR" "${OPENLDAP_SHA256:-}"

  SRC="$SRCDIR/openldap-$OPENLDAP_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"

  # 只要客户端库 libldap/liblber：slapd 和所有 backend 全关。
  OPTS=(--disable-slapd --enable-backends=no --without-cyrus-sasl --with-threads=posix)
  if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then
    OPTS+=(--with-tls=openssl)
  else
    OPTS+=(--without-tls)
  fi

  log "configure + make depend + make -j$JOBS"
  ./configure --prefix="$D" "${OPTS[@]}" > "$LOGS/openldap-configure.log" 2>&1 \
    || { tail -20 "$LOGS/openldap-configure.log"; die "openldap configure 失败"; }
  make depend >> "$LOGS/openldap-configure.log" 2>&1 || true
  make -j"$JOBS" > "$LOGS/openldap-make.log" 2>&1 \
    || { tail -20 "$LOGS/openldap-make.log"; die "openldap make 失败"; }
  make install >> "$LOGS/openldap-make.log" 2>&1

  [ -f "$D/lib/libldap.so" ] || die "没找到 $D/lib/libldap.so"

  stamp_write openldap "$SIG"
  ok "OpenLDAP $OPENLDAP_VERSION -> $D"
}

recipe_main "$@"
