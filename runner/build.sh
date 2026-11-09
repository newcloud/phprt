#!/usr/bin/env bash
# phprt：解析选择 -> deps -> php -> ext，每步按签名跳过
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
# 默认复用已解压的依赖源码，只有 --clean-src 才清空重解压。
# 必须在 source common.sh 之前设：common.sh 的兼容层会把未设的 REBUILD 置 0。
export REBUILD="${REBUILD:-1}"
# shellcheck disable=SC1091
. "$HERE/common.sh"

BUNDLE=""
TREE=""
EXTS=""
PLAN="${PHPRT_PLAN:-0}"
while [ $# -gt 0 ]; do
  case "$1" in
    --bundle) BUNDLE="${2:-}"; shift 2 ;;
    --tree)   TREE="${2:-}"; shift 2 ;;
    --exts)   EXTS="${2:-}"; shift 2 ;;
    --plan)   PLAN=1; shift ;;
    *) die "未知参数: $1" ;;
  esac
done
[ -n "$BUNDLE" ] && [ -d "$BUNDLE" ] || die "缺少 bundle"
[ -n "$TREE" ] || die "缺少 tree"

# shellcheck disable=SC1090
. "$BUNDLE/bundle.conf"
apply_php_override

# 构建目录按 PHP 版本分（build-php-<版本>）。commands.sh 已经设好就沿用；
# 单独执行 build.sh 时在这里补上。
if [ -z "${BUILD_ROOT:-}" ]; then
  phprt_set_build_root "$PHP_VERSION"
fi

export BUNDLE TREE
export PREFIX="$TREE"
export SELECTED_EXTS="$EXTS"
export DEPS="$TREE/deps"
export JOBS="${JOBS:-$(nproc 2>/dev/null || echo 4)}"
export VENDOR="$PHPRT_RUNNER/vendor"
[ -d "$VENDOR" ] || die "缺少 $VENDOR（runner/vendor）"

# 默认复用已解压源码；只有 --clean-src 才清空
if [ "${PHPRT_CLEAN_SRC:-0}" = 1 ]; then
  export REBUILD=0
  rm -rf "$SRCDIR"
  rm -rf "$TREE/.phprt-state"
fi
# 先设置 ENABLE_* 开关：builtin 的 --flags/--needs 和条件依赖都读它们
builtin_apply_switches

# ---------------------------------------------------------------- 解析 ----
BUILTINS=""
PECL=""
for e in $EXTS; do
  if [ -f "$BUNDLE/builtin/$e.sh" ]; then
    BUILTINS+=" $e"
  elif [ -f "$BUNDLE/ext/$e.sh" ]; then
    PECL+=" $e"
  else
    die "未知扩展: $e（既不是 builtin 也不是 ext）"
  fi
done

ALL_DEPS=""
PHP_DEPS=""
append_dep() {
  local list="$1" n="$2"
  case " $list " in *" $n "*) printf '%s' "$list" ;; *) printf '%s %s' "$list" "$n" ;; esac
}
add_dep() {
  local n="$1" x
  for x in $(module_needs "$n"); do add_dep "$x"; done
  ALL_DEPS="$(append_dep "$ALL_DEPS" "$n")"
}
add_php_dep() {
  local n="$1" x
  for x in $(module_needs "$n"); do add_php_dep "$x"; done
  PHP_DEPS="$(append_dep "$PHP_DEPS" "$n")"
  ALL_DEPS="$(append_dep "$ALL_DEPS" "$n")"
}

for e in $BUILTINS; do
  # builtin 的依赖必须问 builtin recipe 自己（curl 同时也是 deps/curl 的名字，
  # 不能走 module_sh 的 deps 优先解析）
  for n in $(bash "$BUNDLE/builtin/$e.sh" --needs); do add_php_dep "$n"; done
done
for e in $PECL; do
  for n in $(module_needs "$e"); do add_dep "$n"; done
done

if [ "$PLAN" = 1 ]; then
  printf 'PHP %s\n' "$PHP_VERSION"
  printf '  deps: %s\n' "${ALL_DEPS:-（无）}"
  printf '  php\n'
  printf '  ext:  %s\n' "${PECL:-（无）}"
  exit 0
fi

# 只有真正构建才建目录：--plan 不该在项目里留下空版本树
mkdir -p "$TREE" "$DEPS" "$SRCDIR" "$LOGS"

: > "$LOGS/build.log"
printf 'building\n' > "$TREE/BUILD-STATUS"
printf '%s\n' "$SELECTED_EXTS" > "$TREE/.build-exts"

# 先把配置迁/建到树内的标准位置（ext recipe 的 stamp 检查会看这里）
ensure_tree_config "$TREE"

cat > "$TREE/BUILD-PLAN" <<EOF
PHP_VERSION=$PHP_VERSION
PHP_URL="$PHP_URL"
PHP_SHA256="$PHP_SHA256"
EXTENSIONS="$SELECTED_EXTS"
EOF

{
  for d in $ALL_DEPS; do printf 'deps/%s\n' "$d"; done
  printf 'php\n'
  for e in $PECL; do printf 'ext/%s\n' "$e"; done
} > "$TREE/BUILD-ORDER"

log "PHP $PHP_VERSION"
log "deps:  ${ALL_DEPS:-（无）}"
log "php"
log "ext:   ${PECL:-（无）}"

# ---------------------------------------------------------------- deps ----
for d in $ALL_DEPS; do
  ensure_dep "$d"
done

# 旧版本目录会和当前版本产生同名 SONAME；在 PHP/ext 开始前清掉
prune_stale_deps $ALL_DEPS

# ----------------------------------------------------------------- php ----
PHP_SIG="$(
  {
    # configure 参数里 deps/<库>-<版本>/ 要归一化成 deps/<库>/：同系列小版本
    # 升级路径变了但 ABI 没变，不该因此重编 PHP（版本进 wheel.map / 升级日志）
    # 两种形态都要归一化：路径中段（…/deps/openssl-3.5.9/include）和
    # 行尾（--with-openssl=…/deps/openssl-3.5.9，无尾斜杠）
    bash "$BUNDLE/php.sh" --flags | sed -E -e 's|(deps/[A-Za-z0-9_+]+)-[^/]+/|\1/|g' -e 's|(deps/[A-Za-z0-9_+]+)-[^/ ]+$|\1|g'
    printf 'php=%s\n' "$PHP_VERSION"
    printf 'php_cflags=%s|%s\n' "${PHP_CFLAGS:-}" "${PHP_CXXFLAGS:-}"
    if lto_on php; then
      printf 'lto=%s|%s\n' "$LTO_CFLAGS" "$LTO_LDFLAGS"
    fi
    cat "$BUNDLE/php.sh" "$BUNDLE"/builtin/*.sh 2>/dev/null
    for d in $PHP_DEPS; do
      printf 'dep=%s:%s\n' "$d" "$(recipe_id "$d")"
    done
    printf 'env=%s\n' "${CFLAGS:-} ${CPPFLAGS:-} ${LDFLAGS:-}"
  } | hash_sig
)"

# 显式 --recompile：先清 PHP 状态，失败后重试不会误判为已完成
if [ "${PHPRT_FORCE_PHP:-0}" = 1 ]; then
  rm -f "$(state_file php)"
fi

if [ "${PHPRT_FORCE_PHP:-0}" != 1 ] && state_ok php "$PHP_SIG" && [ -x "$TREE/bin/php" ]; then
  log "跳过 php（configure 和依赖都没变）"
else
  log "执行 php"
  bash "$BUNDLE/php.sh" 2>&1 | tee -a "$LOGS/build.log"
  state_set php "$PHP_SIG"
fi

# ----------------------------------------------------------------- ext ----
for e in $PECL; do
  sig="$(ext_sig "$e")"
  mod="$(ext_modules "$e" | awk '{print $1}')"
  extdir="$(ext_dir_of "$TREE" 2>/dev/null || true)"
  if state_ok "ext/$e" "$sig" && [ -n "$extdir" ] && [ -f "$extdir/$mod.so" ]; then
    log "跳过 ext/$e（签名未变）"
    continue
  fi
  log "执行 ext/$e"
  ini="$(recipe_field "$BUNDLE/ext/$e.sh" ini)"
  [ "$ini" = - ] || rm -f "$TREE/etc/conf.d/$ini"
  # 外层 ext_sig 已判定要重编；强制绕过配方内部的 stamp
  FORCE=1 bash "$BUNDLE/ext/$e.sh" build 2>&1 | tee -a "$LOGS/build.log"
  state_set "ext/$e" "$sig"
done

[ -x "$TREE/bin/php" ] || die "构建完成但没有 $TREE/bin/php"

log "整理运行时库（rt SONAME + \$ORIGIN rpath）"
wheelize_tree "$TREE"

# 树内的标准布局：etc/php.ini、etc/conf.d、etc/ssl/cert.pem
ensure_tree_config "$TREE"
ensure_tree_ssl "$TREE"

{
  printf 'built_at=%s\n' "$(date '+%Y-%m-%d %H:%M:%S')"
  printf 'lto_php=%s\n' "$(lto_state php)"
  if lto_on php; then
    printf 'lto_flags=%s|%s\n' "$LTO_CFLAGS" "$LTO_LDFLAGS"
  fi
  printf 'php_cflags=%s|%s\n' "${PHP_CFLAGS:-}" "${PHP_CXXFLAGS:-}"
  [ -f "$TREE/.configure-line" ] && cat "$TREE/.configure-line"
  env -u LD_LIBRARY_PATH "$TREE/bin/php" -v | head -2
} > "$TREE/BUILD-INFO"

printf 'ok\n' > "$TREE/BUILD-STATUS"
ok "PHP $PHP_VERSION -> $TREE"
