#!/usr/bin/env bash
# =============================================================================
#  自检：安装根结构、依赖记录、动态依赖、扩展、php.ini、Composer、可搬迁性
#  用法: 在 phprt 仓库里跑  ./phprt verify   （或 ./build.sh --verify）
# =============================================================================
set -euo pipefail
export PHPRT_NO_LOGS=1
source "$(dirname "$(readlink -f "$0")")/common.sh"
load_config

[ -x "$PREFIX/bin/php" ] || die "PHP 未安装: $PREFIX/bin/php"

fail=0

# 不依赖 ./myphp 就能检查：这里自己按同样的规则拼环境
run_php() { # run_php <参数...>
  local d ld="" ext=""
  for d in "$PHPRT_HOME"/deps/*/lib; do [ -d "$d" ] && ld+="$d:"; done
  for d in "$PREFIX"/lib/php/extensions/*/; do [ -d "$d" ] && { ext="${d%/}"; break; }; done
  local ossl=""
  for d in "$PHPRT_HOME"/deps/openssl-*/etc/ssl/openssl.cnf; do
    [ -f "$d" ] && { ossl="$d"; break; }
  done
  local pargs=()
  [ -n "$ext" ] && pargs=(-d "extension_dir=$ext")
  PHPRC="$PREFIX/etc/php.ini" PHP_INI_SCAN_DIR="$PREFIX/etc/conf.d" \
  LD_LIBRARY_PATH="${ld%:}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  OPENSSL_CONF="${ossl:-}" \
  "$PREFIX/bin/php" ${pargs[@]+"${pargs[@]}"} "$@"
}

log "1) 安装根"
printf '    %s\n' "$PHPRT_HOME"
for f in "php-$PHP_VERSION/bin/php" config.conf ini/default.ini downloads composer.phar; do
  if [ -e "$PHPRT_HOME/$f" ]; then
    printf '    ✓ %s\n' "$f"
  else
    printf '    ✗ %s 缺失\n' "$f"; fail=1
  fi
done
run_php -v | head -1 | sed 's/^/    /' || fail=1

log "2) 依赖目录记录（php-$PHP_VERSION/.phprt-deps）"
if [ -f "$PREFIX/.phprt-deps" ]; then
  while IFS= read -r d; do
    [ -n "$d" ] || continue
    if [ -d "$d" ]; then
      printf '    ✓ %s\n' "$(short "$d")"
    else
      printf '    ✗ %s（目录不存在，重跑 ./install.sh 会重建）\n' "$d"; fail=1
    fi
  done < "$PREFIX/.phprt-deps"
else
  printf '    ✗ 缺少 .phprt-deps\n'; fail=1
fi

log "3) 动态依赖（除 glibc 外，库必须来自安装根）"
ldd_bad=0
check_ldd() { # check_ldd <文件>
  local f="$1" bad
  bad="$(LD_LIBRARY_PATH="$(printf '%s:' "$PHPRT_HOME"/deps/*/lib)" ldd "$f" 2>/dev/null \
        | awk '$3 ~ /^\// {print $3}' \
        | grep -Ev '/(libc|libm|libgcc_s|libpthread|libdl|librt|libresolv|libcrypt)\.so' \
        | grep -v "^$PHPRT_HOME/" || true)"
  if [ -n "$bad" ]; then
    warn "${f#"$PHPRT_HOME"/} 借用了安装根以外的库:"; printf '      %s\n' $bad; ldd_bad=1; fail=1
  fi
}
check_ldd "$PREFIX/bin/php"
for so in "$PREFIX"/lib/php/extensions/*/*.so; do
  [ -f "$so" ] && check_ldd "$so"
done
[ "$ldd_bad" = 0 ] && ok "php 与全部扩展都没有从系统借库"

log "4) 已加载扩展"
run_php -m | tr '\n' ' ' | sed 's/  */ /g' | fold -w 100 | sed 's/^/    /'; echo

log "5) 按 config.conf 核对开关对应的扩展"
check_ext() { # check_ext <ext> <开关值>
  local ext="$1" on="$2"
  [ "$on" = 1 ] || return 0
  if run_php -r "exit(extension_loaded('$ext')?0:1);"; then
    printf '    ✓ %-10s 已加载\n' "$ext"
  else
    printf '    ✗ %-10s 开关为 1 但没加载\n' "$ext"; fail=1
  fi
}
check_ext openssl    "${ENABLE_OPENSSL:-0}"
check_ext zlib       "${ENABLE_ZLIB:-0}"
check_ext libxml     "${ENABLE_LIBXML2:-0}"
check_ext dom        "${ENABLE_LIBXML2:-0}"
check_ext sqlite3    "${ENABLE_SQLITE:-0}"
check_ext pdo_sqlite "${ENABLE_SQLITE:-0}"
check_ext curl       "${ENABLE_LIBCURL:-0}"
check_ext mbstring   "${ENABLE_ONIGURUMA:-0}"
check_ext intl       "${ENABLE_ICU:-0}"
check_ext zip        "${ENABLE_LIBZIP:-0}"
check_ext redis      "${ENABLE_REDIS:-0}"

log "6) php.ini"
run_php --ini | sed -n '1,3p' | sed 's/^/    /'
for k in openssl.cafile curl.cainfo extension_dir; do
  v="$(run_php -r "echo ini_get('$k');")"
  [ -n "$v" ] && printf '    %s = %s\n' "$k" "$v"
done

log "7) Composer"
if [ -f "$PHPRT_HOME/composer.phar" ]; then
  printf '    %s\n' "$(run_php "$PHPRT_HOME/composer.phar" --version 2>&1 | head -1)"
else
  printf '    未安装（ENABLE_COMPOSER=%s）\n' "${ENABLE_COMPOSER:-0}"
fi

log "8) 搬迁测试（拷安装根到临时目录，现场渲染一个 ./myphp 再跑）"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/phprt.XXXXXX")"
mkdir -p "$tmp"
copy_tree() { # 同一文件系统用硬链接（快、省空间），跨设备就老实拷贝
  if [ "$(stat -c %d "$PHPRT_HOME")" = "$(stat -c %d "$tmp")" ]; then
    cp -al "$@" "$tmp/" 2>/dev/null || cp -a "$@" "$tmp/"
  else
    cp -a "$@" "$tmp/"
  fi
}
copy_tree "$PREFIX" "$DEPS" "$TOOLS"

tpl="$(cat "$HERE/myphp-project.tmpl")"
tpl="${tpl//@PHPRT_ROOT@/$tmp}"
tpl="${tpl//@PHP_VERSION@/$PHP_VERSION}"
tpl="${tpl//@PHP_INI@/}"
tpl="${tpl//@PHP_ARGS@/}"
printf '%s\n' "$tpl" > "$tmp/myphp"
chmod +x "$tmp/myphp"

if "$tmp/myphp" -r 'echo "    搬迁后 PHP ", PHP_VERSION, " / redis ", (phpversion("redis") ?: "-"), " / zip ", (phpversion("zip") ?: "-"), "\n";'; then
  ok "搬迁后可直接运行（也验证了项目入口模板）"
else
  warn "搬迁后运行失败"; fail=1
fi
rm -rf "$tmp"

log "9) 构建记录与磁盘占用"
if [ -f "$HISTORY" ]; then
  tail -5 "$HISTORY" | sed 's/^/    /'
  printf '    共 %s 次构建记录\n' "$(wc -l < "$HISTORY")"
fi
du -sh "$PHPRT_HOME"/* 2>/dev/null | sed 's/^/    /'

echo
if [ "$fail" = 0 ]; then
  ok "全部自检通过"
else
  die "自检有失败项"
fi
