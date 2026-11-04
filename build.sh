#!/usr/bin/env bash
# =============================================================================
#  phprt 构建脚本（内部实现；日常用 install.sh / phprt）
#
#  用法:
#    ./build.sh                  按 <安装根>/configs/default 编译（已装且配置未变则跳过）
#    ./build.sh --offline        只用 downloads/ 里的包，缺包直接报错
#    ./build.sh --rebuild        复用解压好的源码重跑 configure + make
#    ./build.sh --force          清掉各模块安装目录后重来（下载保留）
#    ./build.sh --only php       只跑某个模块（openssl/zlib/libxml2/sqlite/libcurl/
#                                oniguruma/libzip/php/redis/composer）
#                                重编 php 时会自动补跑 redis/composer（它们依赖 PHP）
#    ./build.sh --skip redis     跳过某个模块（可重复）
#    ./build.sh --verify         只做安装结果自检
#    ./build.sh --clean-build    删掉解压出的源码（build/），保留 downloads
#    ./build.sh --prune          删除没有任何 PHP 版本引用的依赖/工具目录（--yes 免确认）
#    ./build.sh --jobs 8         指定并行度（默认 nproc）
#    ./build.sh --prefix DIR     换一个安装根（等价 PHPRT_HOME=DIR）
#    ./build.sh --php 8.6.0-dev  指定要构建的 PHP 版本（读 configs/php-<版本>.conf）
#
#  开关与下载地址在仓库 config.conf，版本清单在 lib/versions/，本文件只做解析与调度。
# =============================================================================
set -euo pipefail

# --prefix 要在 source common.sh 之前生效（那里会算 PHPRT_HOME）
_argv=("$@"); _i=0
while [ "$_i" -lt "${#_argv[@]}" ]; do
  case "${_argv[$_i]}" in
    --prefix)    PHPRT_HOME="${_argv[$((_i + 1))]:-}"; export PHPRT_HOME; _i=$((_i + 2)) ;;
    --prefix=*)  PHPRT_HOME="${_argv[$_i]#*=}"; export PHPRT_HOME; _i=$((_i + 1)) ;;
    --php)       PHPRT_PHP_VERSION="${_argv[$((_i + 1))]:-}"; export PHPRT_PHP_VERSION; _i=$((_i + 2)) ;;
    --php=*)     PHPRT_PHP_VERSION="${_argv[$_i]#*=}"; export PHPRT_PHP_VERSION; _i=$((_i + 1)) ;;
    *)           _i=$((_i + 1)) ;;
  esac
done

source "$(dirname "$(readlink -f "$0")")/lib/common.sh"

ONLY=""
SKIP_LIST=()
DO_VERIFY=0
DO_CLEAN_BUILD=0
DO_PRUNE=0
ASSUME_YES=0
SPEC="$*"
[ -n "$SPEC" ] || SPEC="（无参数）"

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "$0"; exit 0 ;;
    --prefix)  shift 2 ;;   # 已在上面处理
    --prefix=*) shift ;;
    --php)     shift 2 ;;   # 已在上面处理
    --php=*)   shift ;;
    --offline) export OFFLINE=1; shift ;;
    --rebuild) export REBUILD=1; shift ;;
    --force)   export FORCE=1; export REBUILD=1; shift ;;
    --verify)  DO_VERIFY=1; shift ;;
    --clean-build|--clean-cache) DO_CLEAN_BUILD=1; shift ;;
    --prune)   DO_PRUNE=1; shift ;;
    --yes)     ASSUME_YES=1; shift ;;
    --only)    ONLY="${2:-}"; shift 2 ;;
    --only=*)  ONLY="${1#*=}"; shift ;;
    --skip)    SKIP_LIST+=("${2:-}"); shift 2 ;;
    --skip=*)  SKIP_LIST+=("${1#*=}"); shift ;;
    --jobs)    export JOBS="${2:-}"; shift 2 ;;
    --jobs=*)  export JOBS="${1#*=}"; shift ;;
    *) die "未知参数: $1（--help 查看用法）" ;;
  esac
done

if [ "$DO_VERIFY" = 1 ]; then
  export PHPRT_HOME
  exec "$HERE/verify.sh"
fi

mkdir -p "$PHPRT_HOME"
exec 9>"$PHPRT_HOME/.lock"
flock -n 9 || die "另一个构建正在进行（锁: $PHPRT_HOME/.lock）"

write_home_readme() {
  cat > "$PHPRT_HOME/README.txt" <<'EOF'
这是 phprt 的安装根，脚本本身在源码仓库里（不在这个目录）。

composer.phar Composer（项目的 ./myphp composer ... 用它）
php<版本>_*.ini  php.ini（default / dev / prod，每个版本一套）
php-<版本>/    PHP 本体（每个版本一套，互不干扰）
deps/         依赖库，跨版本共享
tools/        构建工具（autoconf、perl 核心模块）
downloads/    下载的源码包 + SHA256SUMS —— 建议保留，删了下次要重下
build/        解压后的源码与中间产物 —— 可以删
logs/         每次构建一个目录（latest 指向最近一次）+ history.log
configs/      一个版本一份配置：php-<版本>.conf + default

清理： ./build.sh --clean-build        只删 build/
       ./build.sh --prune [--yes]      删没被任何 PHP 版本引用的 deps/tools/php-*/php*_*.ini
EOF
}

# ------------------------------------------------------- 清理子命令 ------
do_clean_build() {
  local before
  if [ -d "$SRCDIR" ] && [ -n "$(ls -A "$SRCDIR" 2>/dev/null)" ]; then
    before="$(du -sh "$SRCDIR" 2>/dev/null | cut -f1)"
    log "删除解压出的源码与中间产物: $SRCDIR（约 $before）"
    rm -rf "${SRCDIR:?}"/*
    ok "已清理；下载包保留在 $DOWNLOADS"
  else
    ok "build/ 已经是空的，无需清理"
  fi
}

do_prune() {
  local -a keep=() cand=()
  local d f n hit
  # 当前配置会用到的依赖与构建工具
  for n in openssl zlib libxml2 sqlite curl oniguruma libzip icu autoconf perl5core re2c; do
    keep+=("$(dep_dir_of "$n")")
  done
  keep+=("$PREFIX")
  # 已经装好的其它 PHP 版本（有签名才算装好）连同它链接的依赖一起保留
  for d in "$PHPRT_HOME"/php-*; do
    [ -d "$d" ] || continue
    [ -f "$d/.phprt-stamp-php" ] || continue
    keep+=("$d")
    f="$d/.phprt-deps"
    [ -f "$f" ] || continue
    while IFS= read -r n; do
      [ -n "$n" ] && keep+=("$n")
    done < "$f"
  done

  # 已卸载版本留下的 php<版本>_*.ini
  for f in "$PHPRT_HOME"/php*_*.ini; do
    [ -f "$f" ] || continue
    n="$(basename "$f" | sed -n 's/^php\(.*\)_[A-Za-z0-9_]*\.ini$/\1/p')"
    [ -n "$n" ] || continue
    [ -d "$PHPRT_HOME/php-$n" ] || cand+=("$f")
  done

  for d in "$DEPS"/* "$TOOLS"/* "$PHPRT_HOME"/php-*; do
    [ -d "$d" ] || continue
    hit=0
    for f in "${keep[@]}"; do
      [ "$d" = "$f" ] && { hit=1; break; }
    done
    [ "$hit" = 1 ] || cand+=("$d")
  done

  if [ "${#cand[@]}" = 0 ]; then
    ok "没有可清理的目录（当前配置与被引用的依赖都还在用）"
    return 0
  fi

  log "以下目录/文件没有被配置或任何 php-*/ 引用："
  for d in "${cand[@]}"; do
    printf '    %-40s %s\n' "$(short "$d")" "$(du -sh "$d" 2>/dev/null | cut -f1)"
  done

  if [ "$ASSUME_YES" != 1 ]; then
    [ -t 0 ] || { warn "非交互环境，加 --yes 才会真的删除"; return 0; }
    printf '确认删除？[y/N] '
    read -r ans
    case "$ans" in
      y|Y|yes|YES) ;;
      *) warn "已取消"; return 0 ;;
    esac
  fi

  for d in "${cand[@]}"; do
    rm -rf "$d" && ok "已删除 $(short "$d")"
  done
}

if [ "$DO_CLEAN_BUILD" = 1 ] || [ "$DO_PRUNE" = 1 ]; then
  export PHPRT_NO_LOGS=1   # 这两个子命令不写日志，别留下空目录
fi
load_config
write_home_readme

if [ "$DO_CLEAN_BUILD" = 1 ]; then
  do_clean_build
  exit 0
fi
if [ "$DO_PRUNE" = 1 ]; then
  do_prune
  exit 0
fi

# --------------------------------------------------------------- 调度 ------
run_module() { # run_module <name>
  local name="$1" s
  for s in ${SKIP_LIST[@]+"${SKIP_LIST[@]}"}; do
    [ "$s" = "$name" ] && { warn "跳过模块 $name（--skip）"; return 0; }
  done
  local script="$MODULES/$name.sh"
  [ -f "$script" ] || die "开关打开了 $name，但模块脚本不存在: $script"
  log "模块: $name"
  bash "$script"
}

# 模块顺序固定（名字 = lib/modules/<名字>.sh 的文件名）
ALL_MODULES=(openssl zlib libxml2 sqlite libcurl oniguruma libzip
             icu libpng jpeg freetype libwebp libaom libavif libffi libyaml
             php redis protobuf xdebug apcu yaml swoole composer)

enabled_module() { # enabled_module <name>
  case "$1" in
    openssl)   [ "${ENABLE_OPENSSL:-0}" = 1 ] ;;
    zlib)      [ "${ENABLE_ZLIB:-0}" = 1 ] ;;
    libxml2)   [ "${ENABLE_LIBXML2:-0}" = 1 ] ;;
    sqlite)    [ "${ENABLE_SQLITE:-0}" = 1 ] ;;
    libcurl)   [ "${ENABLE_LIBCURL:-0}" = 1 ] ;;
    oniguruma) [ "${ENABLE_ONIGURUMA:-0}" = 1 ] ;;
    libzip)    [ "${ENABLE_LIBZIP:-0}" = 1 ] ;;
    php)       : ;;
    icu)       [ "${ENABLE_ICU:-0}" = 1 ] ;;
    libpng|jpeg|freetype) [ "${ENABLE_GD:-0}" = 1 ] ;;
    libwebp)   [ "${ENABLE_WEBP:-0}" = 1 ] ;;
    libaom|libavif) [ "${ENABLE_AVIF:-0}" = 1 ] ;;
    libffi)    [ "${ENABLE_FFI:-0}" = 1 ] ;;
    libyaml)   [ "${ENABLE_YAML:-0}" = 1 ] ;;
    redis)     [ "${ENABLE_REDIS:-0}" = 1 ] ;;
    protobuf)  [ "${ENABLE_PROTOBUF:-0}" = 1 ] ;;
    xdebug)    [ "${ENABLE_XDEBUG:-0}" = 1 ] ;;
    apcu)      [ "${ENABLE_APCU:-0}" = 1 ] ;;
    yaml)      [ "${ENABLE_YAML:-0}" = 1 ] ;;
    swoole)    [ "${ENABLE_SWOOLE:-0}" = 1 ] ;;
    composer)  [ "${ENABLE_COMPOSER:-0}" = 1 ] ;;
    *)         return 1 ;;
  esac
}

MODULES_TO_RUN=()
if [ -z "$ONLY" ]; then
  for m in "${ALL_MODULES[@]}"; do
    enabled_module "$m" && MODULES_TO_RUN+=("$m")
  done
else
  # curl 是 libcurl 的别名（依赖目录叫 curl-<版本>，模块脚本叫 libcurl.sh）
  [ "$ONLY" = curl ] && ONLY=libcurl
  case " ${ALL_MODULES[*]} " in
    *" $ONLY "*) ;;
    *) die "未知模块: $ONLY（可用: ${ALL_MODULES[*]}）" ;;
  esac
  MODULES_TO_RUN=("$ONLY")
  if [ "$ONLY" = php ]; then
    # redis.so / composer.phar 都装在 PHP 目录或依赖 PHP 版本，PHP 一变就得跟着重来
    [ "${ENABLE_REDIS:-0}" = 1 ]    && MODULES_TO_RUN+=(redis)
    [ "${ENABLE_PROTOBUF:-0}" = 1 ] && MODULES_TO_RUN+=(protobuf)
    [ "${ENABLE_XDEBUG:-0}" = 1 ]   && MODULES_TO_RUN+=(xdebug)
    [ "${ENABLE_APCU:-0}" = 1 ]     && MODULES_TO_RUN+=(apcu)
    [ "${ENABLE_YAML:-0}" = 1 ]     && MODULES_TO_RUN+=(yaml)
    [ "${ENABLE_SWOOLE:-0}" = 1 ]   && MODULES_TO_RUN+=(swoole)
    [ "${ENABLE_COMPOSER:-0}" = 1 ] && MODULES_TO_RUN+=(composer)
  fi
fi

# 依赖没开时给个提醒（不阻止构建，用户可能就想那样）
[ "${ENABLE_LIBCURL:-0}" = 1 ] && [ "${ENABLE_OPENSSL:-0}" != 1 ] && \
  warn "ENABLE_LIBCURL=1 但 ENABLE_OPENSSL=0：curl 不会有 https"
[ "${ENABLE_LIBZIP:-0}" = 1 ] && [ "${ENABLE_ZLIB:-0}" != 1 ] && \
  warn "ENABLE_LIBZIP=1 但 ENABLE_ZLIB=0：zip 的 deflate 压缩不可用"
[ "${ENABLE_COMPOSER:-0}" = 1 ] && [ "${ENABLE_OPENSSL:-0}" != 1 ] && \
  warn "ENABLE_COMPOSER=1 但 ENABLE_OPENSSL=0：Composer 下载不了依赖"

# ---------------------------------------------------------- php.ini 生成 ----
# 每个版本三份：default（以构建产物为底）/ dev / prod，都带上 CA 路径。
# 只在缺失时生成，不覆盖用户改过的文件（想重置就删掉再 build）。
gen_inis() {
  local ver="$PHP_VERSION" ca def name f
  ca="$(ca_ini_lines)"

  def="$(ini_file "$ver" default)"
  if [ ! -f "$def" ] && [ -f "$PREFIX/etc/php.ini" ]; then
    {
      printf '; phprt 默认 php.ini（PHP %s）\n' "$ver"
      printf '; ./myphp 和 phprt exec 默认用它；要改默认配置就改这个文件。\n'
      printf '; 这里不写 extension_dir：由 ./myphp 用 -d 给出，安装根整体搬走也有效。\n\n'
      grep -vE '^extension_dir[[:space:]]*=' "$PREFIX/etc/php.ini"
    } > "$def.tmp"
    mv -f "$def.tmp" "$def"
  fi

  for name in dev prod; do
    f="$(ini_file "$ver" "$name")"
    [ -f "$REPO/ini/$name.ini" ] || continue
    [ -f "$f" ] && continue
    {
      printf '; phprt: PHP %s / %s\n' "$ver" "$name"
      cat "$REPO/ini/$name.ini"
      printf '\n[PHP]\n%s\n' "$ca"
    } > "$f.tmp"
    mv -f "$f.tmp" "$f"
  done

  ok "php.ini: $(short "$(ini_file "$ver" default)") · dev · prod"
}

# ------------------------------------------------------- history/latest ------
STARTED="$(date '+%Y-%m-%d %H:%M:%S')"
SECONDS=0

finish() { # finish <退出码>
  local rc="$1" status dur
  if [ "$rc" = 0 ]; then status="成功"; else status="失败"; fi
  dur="${SECONDS}s"
  printf '%s  %s  %-6s %s\n' "$STARTED" "$status" "$dur" "$SPEC" >> "$HISTORY"
  if [ -d "$LOGS" ]; then
    ln -sfn "logs/$RUN_ID" "$LOGS_DIR/latest"
  fi
  if [ "$rc" = 0 ]; then
    printf '\n%s构建完成%s\n' "$C_G" "$C_R"
    printf '  下一步: 到项目目录跑  phprt init   生成 ./myphp\n'
    printf '  日志: %s\n' "$(short "$LOGS/summary.log")"
    printf '  记录: %s\n' "$(short "$HISTORY")"
  else
    printf '\n%s构建失败%s（日志: %s）\n' "$C_E" "$C_R" "$(short "$LOGS")" >&2
  fi
}
on_exit() { local rc=$?; finish "$rc"; exit "$rc"; }
trap on_exit EXIT

export PHPRT_RUN_ID="$RUN_ID"
export PHPRT_HOME
export SUMMARY_LOG="$LOGS/summary.log"
: > "$SUMMARY_LOG"
{
  printf 'run:    %s\n' "$RUN_ID"
  printf 'home:   %s\n' "$PHPRT_HOME"
  printf 'php:    %s\n' "$PHP_VERSION"
  printf 'start:  %s\n' "$STARTED"
  printf '命令:   %s\n' "$SPEC"
  printf '模块:   %s\n' "${MODULES_TO_RUN[*]}"
  printf 'jobs:   %s\n\n' "$JOBS"
} >> "$SUMMARY_LOG"

for m in "${MODULES_TO_RUN[@]}"; do
  run_module "$m"
done

gen_inis

