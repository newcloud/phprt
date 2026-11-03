#!/usr/bin/env bash
# =============================================================================
#  公共函数（被 build.sh、install.sh、phprt、verify.sh、modules/*.sh 引用）
#  路径、日志、下载（断点续传 + 代理回退）、sha256、解压、stamp
#
#  两个概念：
#    仓库（REPO）        这套脚本自己所在的项目，用 git 管理，只放代码
#    安装根（PHPRT_HOME） 编译产物：~/.phprt 或 <项目>/.phprt，可挪可删
#
#  安装根布局：
#    composer.phar          Composer（项目里 ./myphp composer ... 用）
#    ini/                   php.ini 预设（default / dev / prod）
#    php<版本>_<别名>.ini     php.ini（default/dev/prod，每个版本一套）
#    php-<版本>/             PHP 本体（每个 PHP 版本一套）
#    deps/<名字>-<版本>/      依赖库，跨版本共享
#    tools/<名字>-<版本>/     构建工具（autoconf、perl 核心模块）
#    downloads/             下载缓存（可整体拷走，装的时候就不必联网）
#    build/                 解压出来的源码（可删）
#    logs/                  每次构建一个目录 + history.log
# =============================================================================
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"   # 仓库的 lib/
REPO="$(cd "$HERE/.." && pwd)"                                          # 仓库根
VENDOR="$HERE/vendor"
MODULES="$HERE/modules"

# 安装根：install.sh 会传绝对路径；手动用时默认 ~/.phprt
PHPRT_HOME="${PHPRT_HOME:-$HOME/.phprt}"
case "$PHPRT_HOME" in
  /*) ;;
  *)  PHPRT_HOME="$PWD/$PHPRT_HOME" ;;
esac
PHPRT_HOME="${PHPRT_HOME%/}"

DEPS="$PHPRT_HOME/deps"
TOOLS="$PHPRT_HOME/tools"
DOWNLOADS="$PHPRT_HOME/downloads"
SRCDIR="$PHPRT_HOME/build"
LOGS_DIR="$PHPRT_HOME/logs"
CONFIG="${PHPRT_CONFIG:-$PHPRT_HOME/config.conf}"

# 一次构建 = 一个 RUN_ID = 一个日志目录；模块作为子进程时继承同一个值
RUN_ID="${PHPRT_RUN_ID:-$(date '+%Y-%m-%d_%H-%M-%S')}"
LOGS="$LOGS_DIR/$RUN_ID"
MANIFEST="$DOWNLOADS/SHA256SUMS"
HISTORY="$LOGS_DIR/history.log"

# build.sh 会设置它，让所有输出同时写进本次构建的 summary.log
SUMMARY_LOG="${SUMMARY_LOG:-}"

PROXY="${PHPRT_PROXY:-http://127.0.0.1:10808}"
JOBS="${JOBS:-$(nproc 2>/dev/null || echo 4)}"
OFFLINE="${OFFLINE:-0}"
FORCE="${FORCE:-0}"
REBUILD="${REBUILD:-0}"

if [ -t 1 ]; then
  C_R=$'\033[0m'; C_C=$'\033[1;36m'; C_G=$'\033[1;32m'; C_Y=$'\033[1;33m'; C_E=$'\033[1;31m'
else
  C_R=""; C_C=""; C_G=""; C_Y=""; C_E=""
fi

_rec()  { if [ -n "$SUMMARY_LOG" ]; then printf '%s\n' "$*" >> "$SUMMARY_LOG"; fi; }
log()   { printf '%s==>%s %s\n' "$C_C" "$C_R" "$*"; _rec "==> $*"; }
ok()    { printf '%s  ok%s %s\n' "$C_G" "$C_R" "$*"; _rec "  ok $*"; }
warn()  { printf '%s  !%s %s\n' "$C_Y" "$C_R" "$*" >&2; _rec "  ! $*"; }
die()   { printf '%s错误:%s %s\n' "$C_E" "$C_R" "$*" >&2; _rec "错误: $*"; exit 1; }

# 安装根内路径的简写，用于打印（/home/z/.phprt/php-8.5.12 -> php-8.5.12）
short() { printf '%s\n' "${1#"$PHPRT_HOME"/}"; }

load_config() {
  [ -f "$CONFIG" ] || die "缺少配置: $CONFIG（先运行 ./install.sh，它会把配置写到安装根）"
  # shellcheck disable=SC1090
  . "$CONFIG"
  [ -n "${PHP_VERSION:-}" ] || die "config.conf 里 PHP_VERSION 不能为空（见 lib/versions/）"
  PREFIX="$PHPRT_HOME/php-$PHP_VERSION"
  # 目录只在这里创建：早于 load_config 使用的函数都不该依赖它们是已存在的
  mkdir -p "$DEPS" "$TOOLS" "$DOWNLOADS" "$SRCDIR" "$PREFIX"
  # verify.sh / --prune 不写日志，不该留下空日志目录
  [ "${PHPRT_NO_LOGS:-0}" = 1 ] || mkdir -p "$LOGS"
}

need_cmds() {
  local c
  for c in "$@"; do
    command -v "$c" >/dev/null 2>&1 || die "缺少命令: $c"
  done
}

# ------------------------------------------------------- 依赖目录与签名 ------
# 依赖安装目录：<安装根>/deps/<名字>-<版本>/
dep_dir() { printf '%s/%s-%s\n' "$DEPS" "$1" "$2"; }

dep_dir_of() { # dep_dir_of <名字>
  case "$1" in
    openssl)   dep_dir openssl   "$OPENSSL_VERSION" ;;
    zlib)      dep_dir zlib      "$ZLIB_VERSION" ;;
    libxml2)   dep_dir libxml2   "$LIBXML2_VERSION" ;;
    sqlite)    dep_dir sqlite    "$SQLITE_VERSION" ;;
    curl)      dep_dir curl      "$LIBCURL_VERSION" ;;
    oniguruma) dep_dir oniguruma "$ONIGURUMA_VERSION" ;;
    libzip)    dep_dir libzip    "$LIBZIP_VERSION" ;;
    icu)       dep_dir icu       "$ICU_VERSION" ;;
    libpng)    dep_dir libpng    "$LIBPNG_VERSION" ;;
    jpeg)      dep_dir jpeg      "$JPEG_VERSION" ;;
    freetype)  dep_dir freetype  "$FREETYPE_VERSION" ;;
    libffi)    dep_dir libffi    "$LIBFFI_VERSION" ;;
    libyaml)   dep_dir libyaml   "$LIBYAML_VERSION" ;;
    autoconf)  printf '%s/autoconf-%s\n' "$TOOLS" "$AUTOCONF_VERSION" ;;
    perl5core) printf '%s/perl5core-%s\n' "$TOOLS" "$PERL_SHIM_VERSION" ;;
    *) die "未知依赖: $1" ;;
  esac
}

# 安装根里的 php.ini：php<版本>_<别名>.ini（default / dev / prod / 自定义）
ini_file() { printf '%s/php%s_%s.ini\n' "$PHPRT_HOME" "$1" "$2"; }

# CA 路径：优先从构建生成的 php.ini 里抄，抄不到就按系统常见位置探测
ca_ini_lines() {
  local phpini="$PREFIX/etc/php.ini" ca
  if [ -f "$phpini" ] && grep -qE '^(openssl\.cafile|curl\.cainfo)' "$phpini"; then
    grep -E '^(openssl\.cafile|curl\.cainfo)' "$phpini"
    return 0
  fi
  for ca in /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem \
            /etc/ssl/certs/ca-certificates.crt /etc/ssl/cert.pem; do
    [ -f "$ca" ] || continue
    printf 'openssl.cafile = "%s"\ncurl.cainfo = "%s"\n' "$ca" "$ca"
    return 0
  done
  return 0
}

# 按 config.conf 开关列出本次启用且确实存在的依赖库目录（顺序固定）
enabled_dep_names() {
  [ "${ENABLE_OPENSSL:-0}" = 1 ]   && echo openssl
  [ "${ENABLE_ZLIB:-0}" = 1 ]      && echo zlib
  [ "${ENABLE_LIBXML2:-0}" = 1 ]   && echo libxml2
  [ "${ENABLE_SQLITE:-0}" = 1 ]    && echo sqlite
  [ "${ENABLE_LIBCURL:-0}" = 1 ]   && echo curl
  [ "${ENABLE_ONIGURUMA:-0}" = 1 ] && echo oniguruma
  [ "${ENABLE_LIBZIP:-0}" = 1 ]    && echo libzip
  [ "${ENABLE_ICU:-0}" = 1 ]       && echo icu
  [ "${ENABLE_GD:-0}" = 1 ]        && echo libpng && echo jpeg && echo freetype
  [ "${ENABLE_FFI:-0}" = 1 ]       && echo libffi
  [ "${ENABLE_YAML:-0}" = 1 ]      && echo libyaml
  return 0
}

dep_lib_dirs() { # dep_lib_dirs <名字...>  -> 存在的 <dep>/lib
  local n d
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib" ] && printf '%s\n' "$d/lib"
  done
  return 0
}

dep_pc_path() { # dep_pc_path <名字...>  -> 存在的 <dep>/lib/pkgconfig（冒号连接）
  local n d out=""
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib/pkgconfig" ] && out+="$d/lib/pkgconfig:"
  done
  printf '%s\n' "${out%:}"
}

dep_include_flags() { # dep_include_flags <名字...> -> "-I... -I..."
  local n d out=""
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/include" ] && out+="-I$d/include "
  done
  printf '%s\n' "${out% }"
}

# 签名文件放在被描述的那个安装目录里（删目录 = 签名一起没了 = 会重装）
stamp_file() { # stamp_file <模块名>
  case "$1" in
    php|redis|xdebug|apcu|yaml) printf '%s/.phprt-stamp-%s\n' "$PREFIX" "$1" ;;
    composer)  printf '%s/.phprt-stamp-composer\n' "$PHPRT_HOME" ;;
    perlshim)  printf '%s/.phprt-stamp-perlshim\n' "$(dep_dir_of perl5core)" ;;
    *)         printf '%s/.phprt-stamp-%s\n' "$(dep_dir_of "$1")" "$1" ;;
  esac
}

stamp_ok() { # stamp_ok <模块名> <签名>   目录不存在时直接返回 1
  local f
  f="$(stamp_file "$1")"
  [ -f "$f" ] && [ "$(cat "$f")" = "$2" ]
}

stamp_write() { # stamp_write <模块名> <签名>
  local f
  f="$(stamp_file "$1")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$2" > "$f"
}

# --force 时先清掉该模块自己的安装目录（依赖互相独立，不会误删别的）
ensure_clean_target() { # ensure_clean_target <模块名> <安装目录>
  [ "$FORCE" = 1 ] || return 0
  [ -d "$2" ] || return 0
  log "清空 $2（--force）"
  rm -rf "$2"
}

# PHP 的签名：PHP 版本 + 每个开关及其版本（改任一项都会重编 PHP）
php_sig() {
  local s="php=$PHP_VERSION"
  s+=";openssl=${ENABLE_OPENSSL:-0}:$OPENSSL_VERSION"
  s+=";zlib=${ENABLE_ZLIB:-0}:$ZLIB_VERSION"
  s+=";libxml2=${ENABLE_LIBXML2:-0}:$LIBXML2_VERSION"
  s+=";sqlite=${ENABLE_SQLITE:-0}:$SQLITE_VERSION"
  s+=";curl=${ENABLE_LIBCURL:-0}:$LIBCURL_VERSION"
  s+=";oniguruma=${ENABLE_ONIGURUMA:-0}:$ONIGURUMA_VERSION"
  s+=";icu=${ENABLE_ICU:-0}:$ICU_VERSION"
  s+=";libzip=${ENABLE_LIBZIP:-0}:$LIBZIP_VERSION"
  s+=";mysqli=${ENABLE_MYSQLI:-0}"
  s+=";gd=${ENABLE_GD:-0}:$LIBPNG_VERSION/$JPEG_VERSION/$FREETYPE_VERSION"
  s+=";ffi=${ENABLE_FFI:-0}:$LIBFFI_VERSION"
  printf '%s\n' "$s"
}

php_is_current() {
  [ -x "$PREFIX/bin/php" ] || return 1
  stamp_ok php "$(php_sig)" || return 1
  [ -f "$PREFIX/.phprt-deps" ] || return 1
  local d
  while IFS= read -r d; do
    [ -n "$d" ] || continue
    [ -d "$d" ] || return 1
  done < "$PREFIX/.phprt-deps"
}

# ---------------------------------------------------------------- 下载 ------
_dl_once() { # _dl_once <url> <dest> <proxy|"">
  local url="$1" dest="$2" proxy="$3" pargs=()
  [ -n "$proxy" ] && pargs=(-x "$proxy")
  # 先尝试断点续传；服务器不支持 Range 时会报错，再从头下
  local opt=(--connect-timeout 15 --max-time 1800 --speed-limit 1024 --speed-time 60
             --retry 3 --retry-delay 2)
  if ! curl -fsSL "${pargs[@]}" "${opt[@]}" -C - -o "$dest" "$url" 2>/dev/null; then
    rm -f "$dest"
    curl -fsSL "${pargs[@]}" "${opt[@]}" -o "$dest" "$url" || return 1
  fi
  [ -s "$dest" ]
}

fetch() { # fetch <url> [dest] [sha256]
  local url="$1" dest="${2:-}" expect="${3:-}"
  [ -n "$dest" ] || dest="$DOWNLOADS/$(basename "${url%%\?*}")"

  if [ -s "$dest" ]; then
    ok "缓存命中 $(basename "$dest")"
    sha_check "$dest" "$expect"
    return 0
  fi

  [ "$OFFLINE" = 1 ] && die "离线模式，但缺少: $(basename "$dest")（把它放进 $DOWNLOADS/ 再试）"

  # 先下到 .part 再原子改名：下载中断时不会留下一个"看起来像缓存"的半截文件
  local part="$dest.part"
  log "下载 $(basename "$dest")"
  if _dl_once "$url" "$part" ""; then
    :
  else
    warn "直连失败，改用代理 $PROXY"
    _dl_once "$url" "$part" "$PROXY" || { rm -f "$part"; die "下载失败: $url"; }
  fi
  mv -f "$part" "$dest"
  ok "$(basename "$dest") $(du -h "$dest" | cut -f1)"
  sha_check "$dest" "$expect"
}

# 校验：给定期望值则强制比对；否则查 SHA256SUMS，没有就记录（TOFU）
sha_check() {
  local file="$1" expect="${2:-}" name dir
  name="$(basename "$file")"; dir="$(dirname "$file")"
  mkdir -p "$DOWNLOADS"
  if [ -n "$expect" ]; then
    printf '%s  %s\n' "$expect" "$name" | ( cd "$dir" && sha256sum -c --status - ) \
      || die "sha256 校验失败（配置值）: $name"
    return 0
  fi
  if grep -q "  $name\$" "$MANIFEST" 2>/dev/null; then
    ( cd "$dir" && grep "  $name\$" "$MANIFEST" | sha256sum -c --status - ) \
      || die "sha256 校验失败（SHA256SUMS）: $name"
  else
    ( cd "$dir" && sha256sum "$name" ) >> "$MANIFEST"
  fi
}

# ---------------------------------------------------------------- 解压 ------
extract() { # extract <archive> <destdir> [strip]
  local a="$1" d="$2" strip="${3:-0}" sc=()
  mkdir -p "$d"
  [ "$strip" = 1 ] && sc=(--strip-components=1)
  case "$a" in
    *.tar.gz|*.tgz)  tar -xzf "$a" -C "$d" "${sc[@]}" ;;
    *.tar.xz|*.txz)  tar -xJf "$a" -C "$d" "${sc[@]}" ;;
    *.tar.bz2)       tar -xjf "$a" -C "$d" "${sc[@]}" ;;
    *.zip)           unzip -q "$a" -d "$d" ;;
    *) die "不认识的压缩格式: $a" ;;
  esac
}
