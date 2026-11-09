#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=icu version=ICU_VERSION needs= optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# ICU 的 GitHub release tag 形如 release-79.1
versions() {
  upstream_fetch github-releases-unicode-org-icu \
    "https://api.github.com/repos/unicode-org/icu/releases?per_page=100" |
    grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"release-[0-9]+\.[0-9]+"' |
    sed -E 's/.*release-([0-9]+\.[0-9]+)".*/\1/' | sort -u
}

build() {
  need_cmds gcc g++ make tar

  local D SIG TAR SRC
  D="$(dep_dir_of icu)"
  SIG="icu=$ICU_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok icu "$SIG" && [ -e "$D/lib/libicuuc.so" ]; then
    ok "ICU 已安装，跳过"; return 0
  fi

  ensure_clean_target icu "$D"

  TAR="$(basename "$ICU_URL")"
  fetch "$ICU_URL" "$DOWNLOADS/$TAR" "${ICU_SHA256:-}"

  SRC="$SRCDIR/icu-$ICU_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC/source" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC/source" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    # sources 包顶层是 icu/，去掉这一层，$SRC 直接就是源码根
    extract "$DOWNLOADS/$TAR" "$SRC" 1
  fi

  cd "$SRC/source" 2>/dev/null || die "ICU 源码里找不到 source/ 目录"

  log "configure + make -j$JOBS（ICU 编译较久）"
  ./configure --prefix="$D" \
    --disable-samples --disable-tests --disable-extras \
    --enable-shared --disable-static \
    --with-data-packaging=library \
    > "$LOGS/icu-configure.log" 2>&1 \
    || { tail -30 "$LOGS/icu-configure.log"; die "ICU configure 失败"; }
  make -j"$JOBS" > "$LOGS/icu-make.log" 2>&1 \
    || { tail -40 "$LOGS/icu-make.log"; die "ICU make 失败"; }
  make install >> "$LOGS/icu-make.log" 2>&1

  stamp_write icu "$SIG"
  ok "ICU $ICU_VERSION -> $D"
}

recipe_main "$@"
