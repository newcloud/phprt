#!/usr/bin/env bash
# =============================================================================
#  phprt: switch=ENABLE_COMPOSER
#  Composer（单文件 phar，放到 <安装根>/composer.phar；由 ./myphp composer 调用）
#  phar 与 PHP 版本无关，所以和 myphp 放在一起共享
#  可单独执行: lib/modules/composer.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

SIG="composer=$COMPOSER_VERSION"
if [ "$FORCE" != 1 ] && stamp_ok composer "$SIG" && [ -f "$PHPRT_HOME/composer.phar" ]; then
  ok "Composer 已安装，跳过"; exit 0
fi

# --force 只清 composer 自己的文件：bin 里还放着 myphp，不能整个删
if [ "$FORCE" = 1 ]; then
  log "清空 $PHPRT_HOME/composer.phar（--force）"
  rm -f "$PHPRT_HOME/composer.phar" "$PHPRT_HOME/.phprt-stamp-composer"
fi

URL="${COMPOSER_URL:-}"
if [ -z "$URL" ]; then
  if [ "$COMPOSER_VERSION" = latest-stable ]; then
    URL="https://getcomposer.org/download/latest-stable/composer.phar"
  else
    URL="https://getcomposer.org/download/${COMPOSER_VERSION}/composer.phar"
  fi
fi

CACHED="$DOWNLOADS/composer-$COMPOSER_VERSION.phar"
if [ -s "$CACHED" ]; then
  ok "缓存命中 $(basename "$CACHED")"
else
  [ "$OFFLINE" = 1 ] && die "离线模式，但缺少缓存: $(basename "$CACHED")"
  log "下载 Composer ($COMPOSER_VERSION)"
  if ! _dl_once "$URL" "$CACHED.part" ""; then
    warn "直连失败，改用代理 $PROXY"
    _dl_once "$URL" "$CACHED.part" "$PROXY" || die "下载 Composer 失败: $URL"
  fi
  mv "$CACHED.part" "$CACHED"
fi

install -m 644 "$CACHED" "$PHPRT_HOME/composer.phar"

# 校验 phar 能被本地 PHP 正常加载
LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
LD_LIBRARY_PATH="$(dirname "$PHP")/../lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$PHP" "$PHPRT_HOME/composer.phar" --version > /dev/null 2>&1 \
  || die "Composer 装上了但跑不起来（检查 openssl / phar）"

stamp_write composer "$SIG"
ok "Composer $("$PHP" "$PHPRT_HOME/composer.phar" --version 2>/dev/null | head -1) -> $PHPRT_HOME/composer.phar"
