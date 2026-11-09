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
# 链接方式：命令显式指定（PHPRT_LINK_MODE）优先，否则用树上记录。
export PHPRT_LINK_MODE="$(link_mode)"
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

# 每个扩展到编译方式：static（编入 PHP）/ shared（独立 .so）。
# builtin/ext 只是源码来源，不再决定方式。
STATIC_BUILTINS=""
SHARED_BUILTINS=""
STATIC_PECL=""
SHARED_PECL=""
for e in $BUILTINS; do
  m="$(mode_of "$e")" || die "无法确定 $e 的编译方式"
  sh="$(ext_recipe_sh "$e")"
  mode_supported "$sh" "$m" || die "$e 不支持 $m（type=$(recipe_types "$sh")）"
  if [ "$m" = static ]; then STATIC_BUILTINS+=" $e"; else SHARED_BUILTINS+=" $e"; fi
done
for e in $PECL; do
  m="$(mode_of "$e")" || die "无法确定 $e 的编译方式"
  sh="$(ext_recipe_sh "$e")"
  mode_supported "$sh" "$m" || die "$e 不支持 $m（type=$(recipe_types "$sh")）"
  if [ "$m" = static ]; then STATIC_PECL+=" $e"; else SHARED_PECL+=" $e"; fi
done
export PHPRT_SHARED_BUILTINS="$SHARED_BUILTINS"

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

# static builtin：依赖进 PHP 的 include/link 和签名
for e in $STATIC_BUILTINS; do
  # builtin 的依赖必须问 builtin recipe 自己（curl 同时也是 deps/curl 的名字，
  # 不能走 module_sh 的 deps 优先解析）
  for n in $(bash "$BUNDLE/builtin/$e.sh" --needs); do add_php_dep "$n"; done
done
# shared builtin：只为独立 .so 建依赖，不进 PHP 签名
for e in $SHARED_BUILTINS; do
  for n in $(needs_with_switch "$e"); do add_dep "$n"; done
done
# static PECL：依赖要链进 PHP
for e in $STATIC_PECL; do
  for n in $(module_needs "$e"); do add_php_dep "$n"; done
done
# shared PECL：只为 .so 建依赖
for e in $SHARED_PECL; do
  for n in $(module_needs "$e"); do add_dep "$n"; done
done

# 静态链接只需要给 PHP 用的依赖准备 .a；树里多出来的旧依赖不能让它失败。
export PHPRT_PHP_DEPS="$PHP_DEPS"

if [ "$PLAN" = 1 ]; then
  printf 'PHP %s\n' "$PHP_VERSION"
  printf '  deps: %s\n' "${ALL_DEPS:-（无）}"
  printf '  php\n'
  printf '  link: %s\n' "$(link_mode)"
  printf '  static: %s\n' "$(printf '%s\n' $STATIC_BUILTINS $STATIC_PECL | sed '/^$/d' | tr '\n' ' ')"
  printf '  shared: %s\n' "$(printf '%s\n' $SHARED_BUILTINS $SHARED_PECL | sed '/^$/d' | tr '\n' ' ')"
  exit 0
fi

# 只有真正构建才建目录：--plan 不该在项目里留下空版本树
mkdir -p "$TREE" "$DEPS" "$SRCDIR" "$LOGS"

# static 扩展构建需要的文件（php.sh 读 flags/ldflags，recipe 读 staged 列表）
export PHPRT_PHP_SRC="$SRCDIR/$PHP_SRC_DIR"
export STATIC_EXT_FLAGS="$BUILD_ROOT/static-ext-flags"
export STATIC_EXT_LDFLAGS="$BUILD_ROOT/static-ext-ldflags"

: > "$LOGS/build.log"
printf 'building\n' > "$TREE/BUILD-STATUS"
printf '%s\n' "$SELECTED_EXTS" > "$TREE/.build-exts"

# 先把配置迁/建到树内的标准位置（ext recipe 的 stamp 检查会看这里）
ensure_tree_config "$TREE"

MODES=""
for e in $BUILTINS $PECL; do
  MODES="$MODES${MODES:+ }$e=$(mode_of "$e")"
done
cat > "$TREE/BUILD-PLAN" <<EOF
PHP_VERSION=$PHP_VERSION
PHP_URL="$PHP_URL"
PHP_SHA256="$PHP_SHA256"
EXTENSIONS="$SELECTED_EXTS"
MODES="$MODES"
LINK_MODE="$(link_mode)"
EOF

{
  for d in $ALL_DEPS; do printf 'deps/%s\n' "$d"; done
  printf 'php\n'
  for e in $STATIC_BUILTINS $STATIC_PECL; do printf 'static/%s\n' "$e"; done
  for e in $SHARED_BUILTINS $SHARED_PECL; do printf 'shared/%s\n' "$e"; done
} > "$TREE/BUILD-ORDER"

log "PHP $PHP_VERSION"
log "deps:  ${ALL_DEPS:-（无）}"
log "php"
log "link:   $(link_mode)"
log "static: ${STATIC_BUILTINS}${STATIC_PECL:-（无）}"
log "shared: ${SHARED_BUILTINS}${SHARED_PECL:-（无）}"

# ---------------------------------------------------------------- deps ----
for d in $ALL_DEPS; do
  ensure_dep "$d"
done

# 旧版本目录会和当前版本产生同名 SONAME；在 PHP/ext 开始前清掉
prune_stale_deps $ALL_DEPS

# static PECL 需要源码在 php-src/ext/<名字>；shared builtin 需要 php-src/ext
# 的原目录。两边都要先确保 PHP 源码已解压。
prepare_php_src() {
  _cur_static=""
  _prev_static=""
  [ -n "$STATIC_PECL" ] || [ -n "$SHARED_BUILTINS" ] ||
    [ -f "$BUILD_ROOT/static-ext-staged" ] || return 0

  ensure_source "$PHP_ARCHIVE" "$PHP_URL" "$PHP_SHA256"
  SRC="$SRCDIR/$PHP_SRC_DIR"
  _fresh_src=0
  if [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $PHP_ARCHIVE（扩展需要 PHP 源码）"
    extract "$DOWNLOADS/$PHP_ARCHIVE" "$SRCDIR"
    _fresh_src=1
  fi
  [ -d "$SRC" ] || die "解压后找不到 $SRC"
  export PHPRT_PHP_SRC="$SRC"

  # 上一轮成功编译进 PHP 的 static 集合（名字:版本；PHP 成功后才更新）。
  # 和本轮不同就说明 configure 必须重新生成，旧目标文件也要清掉。
  _prev_static="$(cat "$BUILD_ROOT/static-ext-staged" 2>/dev/null || true)"
  _cur_static="$(for _e in $STATIC_PECL; do
    printf '%s:%s\n' "$_e" "$(ext_version "$_e")"
  done)"
  if [ "$_fresh_src" = 1 ] || [ "$_prev_static" != "$_cur_static" ]; then
    export PHPRT_BUILDCONF=1
  fi

  # 清掉上一轮 staged、这一轮不再是 static 的目录
  for _line in $_prev_static; do
    _e="${_line%%:*}"
    [ -n "$_e" ] || continue
    case " $STATIC_PECL " in
      *" $_e "*) ;;
      *) rm -rf "$SRC/ext/$_e" ;;
    esac
  done

  if [ -n "$STATIC_PECL" ]; then
    : > "$STATIC_EXT_FLAGS"
    : > "$STATIC_EXT_LDFLAGS"
    : > "$BUILD_ROOT/static-ext-staged.new"
    for _e in $STATIC_PECL; do
      bash "$BUNDLE/ext/$_e.sh" stage-static 2>&1 | tee -a "$LOGS/build.log" ||
        die "static 源码准备失败: $_e"
    done
  else
    rm -f "$STATIC_EXT_FLAGS" "$STATIC_EXT_LDFLAGS"
  fi
}
prepare_php_src

# ----------------------------------------------------------------- php ----
PHP_SIG="$(
  {
    # configure 参数里 deps/<库>-<版本>/ 要归一化成 deps/<库>/：同系列小版本
    # 升级路径变了但 ABI 没变，不该因此重编 PHP（版本进 wheel.map / 升级日志）
    # 两种形态都要归一化：路径中段（…/deps/openssl-3.5.9/include）和
    # 行尾（--with-openssl=…/deps/openssl-3.5.9，无尾斜杠）
    bash "$BUNDLE/php.sh" --flags | sed -E -e 's|(deps/[A-Za-z0-9_+]+)-[^/]+/|\1/|g' -e 's|(deps/[A-Za-z0-9_+]+)-[^/ ]+$|\1|g'
    printf 'php=%s\n' "$PHP_VERSION"
    printf 'link_mode=%s\n' "$(link_mode)"
    printf 'php_cflags=%s|%s\n' "${PHP_CFLAGS:-}" "${PHP_CXXFLAGS:-}"
    if lto_on php; then
      printf 'lto=%s|%s\n' "$LTO_CFLAGS" "$LTO_LDFLAGS"
    fi
    # 只算有效内容：注释和空行不进签名，改说明文字不再触发 PHP 重编。
    # 行首 # 才是注释；参数值和字符串里的 # 保留。
    # builtin 只算本次选中的：bundle 里新增/修改未选中的 recipe 不触发重编。
    grep -hv -e '^[[:space:]]*#' -e '^[[:space:]]*$' \
      "$BUNDLE/php.sh" 2>/dev/null || true
    # 顺序必须与 --ext 的书写顺序无关，否则同一组扩展会有不同签名。
    # 只算 static 的 builtin：shared builtin 不进 PHP configure。
    for _e in $(printf '%s\n' $STATIC_BUILTINS | sort -u); do
      _f="$BUNDLE/builtin/$_e.sh"
      [ -f "$_f" ] || continue
      grep -hv -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$_f" 2>/dev/null || true
    done
    # 只有 static builtin 影响 PHP 签名；shared builtin 新增/删除都不该重编 PHP。
    for _e in $(printf '%s\n' $STATIC_BUILTINS | sort -u); do
      printf 'builtin_mode=%s:static\n' "$_e"
    done
    # static PECL 的版本和 recipe 也要进 PHP 签名
    for _e in $(printf '%s\n' $STATIC_PECL | sort -u); do
      printf 'static_ext=%s:%s:%s\n' "$_e" "$(ext_version "$_e")" "$(recipe_hash "$_e")"
    done
    # 依赖顺序同样不能跟着 --ext 的书写顺序走
    for d in $(printf '%s\n' $PHP_DEPS | sort -u); do
      printf 'dep=%s:%s:%s\n' "$d" "$(recipe_id "$d")" \
        "$(basename "$(recipe_dir_of "$d")")"
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

# 只有 PHP 成功构建后才记下 static 集合；失败时保留旧值，重试仍会
# 触发 distclean/buildconf。
printf '%s\n' "$_cur_static" | sed '/^$/d' > "$BUILD_ROOT/static-ext-staged"

# 切到 static 的扩展：旧的 .so 和 extension= 行必须清掉，否则会
# 出现 “Module already loaded”。
cleanup_static_exts() {
  local e m extdir ini
  extdir="$(ext_dir_of "$TREE" 2>/dev/null || true)"
  for e in $STATIC_BUILTINS $STATIC_PECL; do
    if [ -n "$extdir" ]; then
      for m in $(ext_modules "$e"); do rm -f "$extdir/$m.so"; done
    fi
    for ini in "$TREE"/etc/conf.d/*-"$e".ini; do
      [ -f "$ini" ] || continue
      sed -i -E '/^[[:space:]]*(zend_)?extension[[:space:]]*=/d' "$ini"
      if ! grep -qvE '^[[:space:]]*(;|#|$)' "$ini"; then rm -f "$ini"; fi
    done
  done
  # static PECL 的设置项（如 apcu）保留：先让 recipe 写 ini，再去掉 extension 行
  for e in $STATIC_PECL; do
    local sh ini
    sh="$(ext_recipe_sh "$e")"
    ini="$(recipe_field "$sh" ini)"
    [ "$ini" != - ] || continue
    if recipe_has_func "$sh" enable; then
      bash "$sh" enable >/dev/null 2>&1 || true
      sed -i -E '/^[[:space:]]*(zend_)?extension[[:space:]]*=/d' \
        "$TREE/etc/conf.d/$ini" 2>/dev/null || true
      if [ -f "$TREE/etc/conf.d/$ini" ] &&
         ! grep -qvE '^[[:space:]]*(;|#|$)' "$TREE/etc/conf.d/$ini"; then
        rm -f "$TREE/etc/conf.d/$ini"
      fi
    fi
  done
}
cleanup_static_exts

# ----------------------------------------------------------------- ext ----
# shared builtin：从 php-src/ext/<名字> 拷出来单独 phpize
for e in $SHARED_BUILTINS; do
  sig="$(ext_sig "$e")"
  mod="$(ext_modules "$e" | awk '{print $1}')"
  extdir="$(ext_dir_of "$TREE" 2>/dev/null || true)"
  if state_ok "ext/$e" "$sig" && [ -n "$extdir" ] && [ -f "$extdir/$mod.so" ]; then
    log "跳过 builtin/$e（shared，签名未变）"
    continue
  fi
  log "执行 builtin/$e（shared）"
  SRC="$SRCDIR/shared-$e-$PHP_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    cp -a "$PHPRT_PHP_SRC/ext/$e" "$SRC" || die "找不到 PHP 源码 ext/$e"
  fi
  declare -a sargs=()
  while IFS= read -r a; do
    [ -n "$a" ] && sargs+=("$a")
  done < <(flags_with_switch "$e")
  FORCE=1 ext_build_ready "$e" "$SRC" "$sig" "$e（shared builtin）" "${sargs[@]}"
  state_set "ext/$e" "$sig"
done

# shared PECL
for e in $SHARED_PECL; do
  sig="$(ext_sig "$e")"
  mod="$(ext_modules "$e" | awk '{print $1}')"
  extdir="$(ext_dir_of "$TREE" 2>/dev/null || true)"
  if state_ok "ext/$e" "$sig" && [ -n "$extdir" ] && [ -f "$extdir/$mod.so" ]; then
    log "跳过 ext/$e（签名未变）"
    continue
  fi
  log "执行 ext/$e"
  ini="$(recipe_field "$BUNDLE/ext/$e.sh" ini)"
  # 外层 ext_sig 已判定要重编；强制绕过配方内部的 stamp
  FORCE=1 bash "$BUNDLE/ext/$e.sh" build 2>&1 | tee -a "$LOGS/build.log"
  # 构建成功后再重写 ini。不要在这里提前删 ini：构建失败会把这个扩展
  # 顺带停掉；成功后重写还能让 ini 跟上 recipe 的模板变化。
  if [ "$ini" != - ]; then
    bash "$BUNDLE/ext/$e.sh" enable >> "$LOGS/build.log" 2>&1 \
      || die "扩展 $e 的 enable() 失败（见 $LOGS/build.log）"
  fi
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
