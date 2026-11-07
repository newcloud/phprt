#!/usr/bin/env bash
# phprt：通用函数（被入口、命令实现和 bundle recipe 引用）
set -euo pipefail

if [ -t 1 ]; then
  C_R=$'\033[0m'; C_C=$'\033[1;36m'; C_G=$'\033[1;32m'; C_Y=$'\033[1;33m'; C_E=$'\033[1;31m'
else
  C_R=""; C_C=""; C_G=""; C_Y=""; C_E=""
fi

log()  { printf '%s==>%s %s\n' "$C_C" "$C_R" "$*"; }
ok()   { printf '%s  ok%s %s\n' "$C_G" "$C_R" "$*"; }
warn() { printf '%s  !%s %s\n' "$C_Y" "$C_R" "$*" >&2; }
die()  { printf '%s错误:%s %s\n' "$C_E" "$C_R" "$*" >&2; exit 1; }

find_project() {
  local d="$PWD"
  while :; do
    # 只认真正的项目根：~/.phprt 是全局缓存目录，不能当成项目
    if [ -f "$d/.phprt/project.conf" ] || [ -x "$d/.phprt/bin/phprt" ]; then
      printf '%s\n' "$d"; return 0
    fi
    [ "$d" = "/" ] && break
    d="$(dirname "$d")"
  done
  printf '%s\n' "$PWD"
}

if [ -z "${PHPRT_HOME:-}" ]; then
  PHPRT_PROJECT="$(find_project)"
  PHPRT_HOME="$PHPRT_PROJECT/.phprt"
else
  PHPRT_PROJECT="${PHPRT_PROJECT:-$(dirname "$PHPRT_HOME")}"
fi
export PHPRT_HOME PHPRT_PROJECT

current_version() {
  [ -L "$PHPRT_HOME/current" ] || return 1
  local t
  t="$(readlink -f "$PHPRT_HOME/current")"
  [ -n "$t" ] || return 1
  basename "$t" | sed 's/^php-//'
}

tree_of() { printf '%s/php-%s\n' "$PHPRT_HOME" "$1"; }

# 版本 -> 系列（major.minor）：8.5.12 -> 8.5；8.6.0-dev -> 8.6
series_of() { printf '%s\n' "$1" | cut -d. -f1-2; }

# 某系列已装好的最高版本（BUILD-STATUS=ok 的树；老树没有状态文件时按有 bin/php 算）
# 没有则返回空。供 install 的「已装最新即停」和 use 的系列解析用。
installed_series_max() {
  local series="$1" d v st best=""
  for d in "$PHPRT_HOME"/php-"$series".*; do
    [ -d "$d" ] || continue
    v="${d##*/php-}"
    [ -x "$d/bin/php" ] || continue
    st="$(cat "$d/BUILD-STATUS" 2>/dev/null || true)"
    if [ -f "$d/BUILD-STATUS" ] && [ "$st" != ok ]; then continue; fi
    if [ -z "$best" ]; then
      best="$v"
    else
      best="$(printf '%s\n%s\n' "$best" "$v" | sort -V | tail -1)"
    fi
  done
  [ -n "$best" ] && printf '%s\n' "$best"
  return 0
}

ext_dir_of() {
  local tree="$1" d
  for d in "$tree"/lib/php/extensions/*/; do
    [ -d "$d" ] && { printf '%s\n' "${d%/}"; return 0; }
  done
  return 1
}

sha_check() {
  local file="$1" expect="$2"
  [ -n "$expect" ] || return 0
  printf '%s  %s\n' "$expect" "$(basename "$file")" |
    ( cd "$(dirname "$file")" && sha256sum -c --status - ) ||
    die "sha256 校验失败: $file"
}

copy_hard() {
  local src="$1" dst="$2"
  if [ "$(stat -c %d "$src")" = "$(stat -c %d "$(dirname "$dst")")" ]; then
    ln -f "$src" "$dst" 2>/dev/null || cp -a "$src" "$dst"
  else
    cp -a "$src" "$dst"
  fi
}

find_source() {
  local name="$1" d base series=""
  local -a dirs=("$PHPRT_HOME/downloads")
  [ -n "${PHPRT_SEED:-}" ] && dirs+=("$PHPRT_SEED")
  # 版本系列目录：downloads/php-7.4/ 放这一系列专用的旧版依赖
  [ -n "${PHP_VERSION:-}" ] && series="php-${PHP_VERSION%.*}"
  for base in "${PHPRT_REPO:-}"; do
    [ -n "$base" ] || continue
    [ -n "$series" ] && dirs+=("$base/downloads/$series")
    dirs+=("$base/downloads")
  done
  dirs+=("$HOME/.phprt/downloads")
  for d in "${dirs[@]}"; do
    [ -f "$d/$name" ] && { printf '%s\n' "$d/$name"; return 0; }
  done
  return 1
}

ensure_source() {
  local name="$1" url="$2" sha="$3"
  local dst="$PHPRT_HOME/downloads/$name"
  mkdir -p "$PHPRT_HOME/downloads"
  if [ ! -s "$dst" ]; then
    local src=""
    src="$(find_source "$name" || true)"
    if [ -n "$src" ]; then
      copy_hard "$src" "$dst"
      ok "源码缓存: $name"
    else
      command -v curl >/dev/null 2>&1 || die "缺少 curl，无法下载 $name"
      log "下载 $name"
      curl -fsSL --connect-timeout 15 --max-time 1800 -o "$dst.part" "$url" ||
        { rm -f "$dst.part"; die "下载失败: $url"; }
      mv -f "$dst.part" "$dst"
    fi
  fi
  sha_check "$dst" "$sha"
}

extract() {
  local a="$1" d="$2" strip="${3:-0}" sc=()
  mkdir -p "$d"
  [ "$strip" = 1 ] && sc=(--strip-components=1)
  case "$a" in
    *.tar.gz|*.tgz) tar -xzf "$a" -C "$d" "${sc[@]}" ;;
    *.tar.xz|*.txz) tar -xJf "$a" -C "$d" "${sc[@]}" ;;
    *.tar.bz2)      tar -xjf "$a" -C "$d" "${sc[@]}" ;;
    *.zip)          unzip -q "$a" -d "$d" ;;
    *) die "不认识的压缩格式: $a" ;;
  esac
}

ext_modules() {
  case "$1" in
    openssl)   printf '%s\n' openssl ;;
    zlib)      printf '%s\n' zlib ;;
    libxml2)   printf '%s\n' dom xml simplexml xmlreader xmlwriter soap ;;
    curl)      printf '%s\n' curl ;;
    sqlite)    printf '%s\n' sqlite3 pdo_sqlite ;;
    mysqli)    printf '%s\n' mysqli pdo_mysql ;;
    mbstring)  printf '%s\n' mbstring ;;
    intl)      printf '%s\n' intl ;;
    zip)       printf '%s\n' zip ;;
    gd)        printf '%s\n' gd ;;
    ffi)       printf '%s\n' ffi ;;
    redis)     printf '%s\n' redis ;;
    protobuf)  printf '%s\n' protobuf ;;
    xdebug)    printf '%s\n' xdebug ;;
    apcu)      printf '%s\n' apcu ;;
    yaml)      printf '%s\n' yaml ;;
    swoole)    printf '%s\n' swoole ;;
    *)         printf '%s\n' "$1" ;;
  esac
}

ensure_base_files() {
  # 配置不再放在 .phprt 根下：php.ini / conf.d 都属于具体版本树（标准布局）
  mkdir -p "$PHPRT_HOME/bin/runner" "$PHPRT_HOME/downloads" \
           "$PHPRT_HOME/build" "$PHPRT_HOME/logs"
}

default_php_ini() {
  cat <<'EOF'
[PHP]
date.timezone = Asia/Shanghai
memory_limit = 256M
error_reporting = E_ALL
display_errors = On
log_errors = On
EOF
}

copy_runner() {
  local src_root="$1"
  mkdir -p "$PHPRT_HOME/bin/runner"
  cp -a "$src_root/phprt" "$PHPRT_HOME/bin/phprt"
  cp -a "$src_root/runner/." "$PHPRT_HOME/bin/runner/"
  if [ -d "$src_root/runner/vendor" ]; then
    mkdir -p "$PHPRT_HOME/vendor"
    cp -a "$src_root/runner/vendor/." "$PHPRT_HOME/vendor/"
  fi
  chmod +x "$PHPRT_HOME/bin/phprt"
  printf 'PHPRT_REPO=%s\n' "$src_root" > "$PHPRT_HOME/config"
}

write_myphp() {
  local tpl="$PHPRT_HOME/bin/runner/myphp.tmpl"
  [ -f "$tpl" ] || die "缺少模板: $tpl"
  cp -a "$tpl" "$PHPRT_PROJECT/myphp.tmp"
  chmod +x "$PHPRT_PROJECT/myphp.tmp"
  mv -f "$PHPRT_PROJECT/myphp.tmp" "$PHPRT_PROJECT/myphp"
}

write_project_conf() {
  local php="$1" exts="$2"
  printf 'PHP=%s\nEXTENSIONS="%s"\n' "$php" "$exts" > "$PHPRT_HOME/project.conf"
}

project_exts() {
  local f="$PHPRT_HOME/project.conf"
  [ -f "$f" ] || return 0
  # shellcheck disable=SC1090
  . "$f"
  printf '%s\n' "${EXTENSIONS:-}"
}

switch_current() {
  ln -sfn "php-$1" "$PHPRT_HOME/current"
}

# 系统 OpenSSL 的资源目录（默认 CA / openssl.cnf 所在）。
# 编译时用它，产物里就不会写死构建树的绝对路径；搬走树后直接跑 bin/php 也能兜底。
system_ssldir() {
  local d=""
  if command -v openssl >/dev/null 2>&1; then
    d="$(openssl version -d 2>/dev/null | sed -n 's/^OPENSSLDIR: "\(.*\)"$/\1/p')"
  fi
  case "$d" in
    /*) printf '%s\n' "$d" ;;
    *)  printf '/etc/ssl\n' ;;
  esac
}

# 树内标准配置：$PREFIX/etc/php.ini + $PREFIX/etc/conf.d。
# 老项目把配置放在 .phprt 根下，这里一次性迁进树里，之后只认标准路径。
ensure_tree_config() {
  local tree="$1" f cur
  local ini="$tree/etc/php.ini" cd="$tree/etc/conf.d"
  mkdir -p "$tree/etc" "$cd"

  if [ -f "$PHPRT_HOME/php.ini" ] && [ ! -f "$ini" ]; then
    mv -f "$PHPRT_HOME/php.ini" "$ini"
    ok "迁移配置: .phprt/php.ini -> $ini"
  fi
  if [ -d "$PHPRT_HOME/conf.d" ]; then
    for f in "$PHPRT_HOME/conf.d"/*.ini; do
      [ -f "$f" ] || continue
      [ -f "$cd/$(basename "$f")" ] || mv -f "$f" "$cd/"
    done
    rmdir "$PHPRT_HOME/conf.d" 2>/dev/null || true
  fi

  if [ ! -f "$ini" ]; then
    # 新版本树沿用当前在用的 php.ini，不用每个版本各配一份
    cur="$(readlink -f "$PHPRT_HOME/current" 2>/dev/null || true)"
    if [ -n "$cur" ] && [ "$cur" != "$tree" ] && [ -f "$cur/etc/php.ini" ]; then
      cp -f "$cur/etc/php.ini" "$ini"
    else
      default_php_ini > "$ini"
    fi
    ok "生成 $ini"
  fi
}

# CA 只有一份：$PREFIX/etc/ssl/cert.pem。
# 路径由 myphp 导出的 SSL_CERT_FILE 给出，所以整棵树搬走也不用改配置。
ensure_tree_ssl() {
  local tree="$1" src="" n
  local ssl="$tree/etc/ssl"
  mkdir -p "$tree/etc/conf.d" "$ssl"
  # 清掉旧版本留下的第二份配置/证书子目录，只保留 cert.pem
  rm -rf "$ssl/certs" "$ssl/misc" "$ssl/private"
  rm -f "$ssl/openssl.cnf" "$ssl/openssl.cnf.dist" \
        "$ssl/ct_log_list.cnf" "$ssl/ct_log_list.cnf.dist"

  if [ ! -s "$ssl/cert.pem" ]; then
    for src in "$DOWNLOADS/cacert.pem" \
               /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem \
               /etc/ssl/certs/ca-certificates.crt \
               /etc/pki/tls/certs/ca-bundle.crt \
               /etc/ssl/cert.pem; do
      [ -s "$src" ] && break
      src=""
    done
    if [ -z "$src" ]; then
      warn "没找到 CA 根证书，HTTPS 校验会失败（可把 cacert.pem 放到 $DOWNLOADS/）"
    else
      cp -fL "$src" "$ssl/cert.pem"
      chmod 644 "$ssl/cert.pem"
      n="$(grep -c 'BEGIN CERTIFICATE' "$ssl/cert.pem" 2>/dev/null || true)"
      ok "CA 根证书 ${n:-0} 个 <- $src"
    fi
  fi

  cat > "$tree/etc/conf.d/00-ssl.ini" <<'EOF'
; 由 phprt 生成，勿手改；要覆盖请在 .phprt/conf.d/ 放编号更大的 ini
; 路径来自 myphp 导出的 SSL_CERT_FILE（跟着项目移动）
openssl.cafile = "${SSL_CERT_FILE}"
curl.cainfo    = "${SSL_CERT_FILE}"
EOF
}

enable_ext() {
  local name="$1" ver tree inidir extdir m mods ld any_shared=0 any_static=0
  ver="$(current_version)" || die "还没有 current 版本"
  tree="$(tree_of "$ver")"
  inidir="$tree/etc/conf.d"
  mkdir -p "$inidir"
  extdir="$(ext_dir_of "$tree")" || die "找不到扩展目录: $tree"
  mods="$(ext_modules "$name")"
  ld="$tree/lib:$(ls -d "$tree"/deps/*/lib 2>/dev/null | tr '\n' ':')"

  # 清理重复 ini：保留 recipe 的编号，删掉 enable_ext 生成的 20-<名字>.ini
  local inis count
  inis="$(ls "$inidir"/*-"$name".ini 2>/dev/null || true)"
  count="$(printf '%s\n' "$inis" | grep -c . || true)"
  if [ "$count" -gt 1 ]; then
    printf '%s\n' "$inis" | while IFS= read -r f; do
      [ "$(basename "$f")" = "20-$name.ini" ] && rm -f "$f"
    done
  fi

  if ls "$inidir"/*-"$name".ini >/dev/null 2>&1; then
    ok "扩展 $name 已由 recipe 启用"
    return 0
  fi

  for m in $mods; do
    if [ -f "$extdir/$m.so" ]; then
      any_shared=1
    elif LD_LIBRARY_PATH="$ld" "$tree/bin/php" -m 2>/dev/null | grep -qix "$m"; then
      any_static=1
    else
      die "$m 没有编译；用 phprt install $ver --recompile --ext $name"
    fi
  done

  if [ "$any_shared" = 1 ]; then
    mkdir -p "$inidir"
    {
      printf '; 由 phprt 生成（%s）\n' "$name"
      for m in $mods; do
        [ -f "$extdir/$m.so" ] || continue
        if [ "$name" = xdebug ]; then
          printf 'zend_extension=%s.so\n' "$m"
        else
          printf 'extension=%s.so\n' "$m"
        fi
      done
      if [ "$name" = xdebug ]; then
        printf 'xdebug.mode=off\nxdebug.start_with_request=yes\nxdebug.client_host=127.0.0.1\nxdebug.client_port=9003\n'
      elif [ "$name" = swoole ]; then
        printf 'swoole.use_shortname=Off\n'
      fi
    } > "$inidir/20-$name.ini"
    ok "启用扩展 $name"
  else
    ok "扩展 $name 已编译在 PHP 内"
  fi
}

disable_ext() {
  local name="$1" ver tree inidir
  ver="$(current_version)" || die "还没有 current 版本"
  tree="$(tree_of "$ver")"
  inidir="$tree/etc/conf.d"
  if ls "$inidir"/*-"$name".ini >/dev/null 2>&1; then
    rm -f "$inidir"/*-"$name".ini
    ok "停用扩展 $name"
  else
    warn "扩展 $name 没有独立的 ini（可能是编译在内），停用需要重新编译"
  fi
}

# ---------------------------------------------------------------------------
# 兼容层：让旧 phprt 的 deps/builtin/ext recipe 能在 bundle 里直接运行
# ---------------------------------------------------------------------------
FORCE="${FORCE:-0}"
REBUILD="${REBUILD:-0}"
OFFLINE="${OFFLINE:-0}"
VENDOR="${VENDOR:-$PHPRT_HOME/bin/runner/vendor}"
[ -d "$VENDOR" ] || VENDOR="$PHPRT_HOME/vendor"
DOWNLOADS="$PHPRT_HOME/downloads"
export DOWNLOADS VENDOR

need_cmds() {
  local c
  for c in "$@"; do command -v "$c" >/dev/null 2>&1 || die "缺少命令: $c"; done
}

# ---------------------------------------------------------------------------
# LTO：只给需要跨目标文件优化的模块用。
# 动态库之间不能跨库 LTO，每个 .so / 可执行文件各自在自己的链接单元内优化。
# LTO_CFLAGS / LTO_LDFLAGS 只放 LTO 本身；基础 -O2 由各 recipe 自己决定。
# ---------------------------------------------------------------------------
LTO_CFLAGS="${LTO_CFLAGS:--flto=auto}"
LTO_LDFLAGS="${LTO_LDFLAGS:--flto=auto}"

lto_on() {
  local var="LTO_$(printf '%s' "$1" | tr 'a-z-' 'A-Z_')"
  [ "${!var:-0}" = 1 ]
}

lto_state() { lto_on "$1" && printf 1 || printf 0; }

load_config() {
  [ -n "${BUNDLE:-}" ] || return 0
  # shellcheck disable=SC1090
  . "$BUNDLE/bundle.conf"
}

dep_dir() { printf '%s/%s-%s\n' "$DEPS" "$1" "$2"; }

dep_dir_of() {
  local n="$1"
  case "$n" in
    openssl)   dep_dir openssl   "${OPENSSL_VERSION:-}" ;;
    zlib)      dep_dir zlib      "${ZLIB_VERSION:-}" ;;
    libxml2)   dep_dir libxml2   "${LIBXML2_VERSION:-}" ;;
    sqlite)    dep_dir sqlite    "${SQLITE_VERSION:-}" ;;
    curl)      dep_dir curl      "${LIBCURL_VERSION:-}" ;;
    oniguruma) dep_dir oniguruma "${ONIGURUMA_VERSION:-}" ;;
    libzip)    dep_dir libzip    "${LIBZIP_VERSION:-}" ;;
    icu)       dep_dir icu       "${ICU_VERSION:-}" ;;
    libpng)    dep_dir libpng    "${LIBPNG_VERSION:-}" ;;
    jpeg)      dep_dir jpeg      "${JPEG_VERSION:-}" ;;
    freetype)  dep_dir freetype  "${FREETYPE_VERSION:-}" ;;
    webp)      dep_dir webp      "${WEBP_VERSION:-}" ;;
    aom)       dep_dir aom       "${AOM_VERSION:-}" ;;
    avif)      dep_dir avif      "${AVIF_VERSION:-}" ;;
    libffi)    dep_dir libffi    "${LIBFFI_VERSION:-}" ;;
    libyaml)   dep_dir libyaml   "${LIBYAML_VERSION:-}" ;;
    autoconf)  printf '%s/tools/autoconf-%s\n' "$PHPRT_HOME" "${AUTOCONF_VERSION:-2.72}" ;;
    re2c)      printf '%s/tools/re2c-%s\n'     "$PHPRT_HOME" "${RE2C_VERSION:-4.6}" ;;
    perl5core) printf '%s/tools/perl5core-%s\n' "$PHPRT_HOME" "${PERL_SHIM_VERSION:-5.42.2}" ;;
    *) die "未知依赖: $n" ;;
  esac
}

module_needs() {
  local n="$1" f
  for f in "$BUNDLE/deps/$n.sh" "$BUNDLE/ext/$n.sh"; do
    [ -f "$f" ] || continue
    sed -n 's/^# *phprt:.*[[:space:]]needs=\([^[:space:]]*\).*/\1/p' "$f" |
      head -1 | tr ',' ' '
    return 0
  done
  return 0
}

module_sh() {
  local n="$1" d
  case "$n" in
    autoconf|re2c|perlshim) printf '%s/tools/%s.sh\n' "$BUNDLE" "$n"; return 0 ;;
  esac
  for d in deps ext; do
    [ -f "$BUNDLE/$d/$n.sh" ] && { printf '%s\n' "$BUNDLE/$d/$n.sh"; return 0; }
  done
  die "没有模块 $n"
}

stamp_file() {
  case "$1" in
    php|redis|protobuf|xdebug|apcu|yaml|swoole)
      printf '%s/.phprt-stamp-%s\n' "$TREE" "$1" ;;
    perlshim)
      printf '%s/.phprt-stamp-perlshim\n' "$(dep_dir_of perl5core)" ;;
    *)
      printf '%s/.phprt-stamp-%s\n' "$(dep_dir_of "$1")" "$1" ;;
  esac
}

stamp_ok() {
  local f; f="$(stamp_file "$1")"
  [ -f "$f" ] && [ "$(cat "$f")" = "$2" ]
}

stamp_write() {
  local f; f="$(stamp_file "$1")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$2" > "$f"
}

ensure_clean_target() {
  [ "$FORCE" = 1 ] || return 0
  [ -d "$2" ] || return 0
  log "清空 $2（--force）"
  rm -rf "$2"
}

fetch() {
  local url="$1" dest="$2" expect="${3:-}"
  ensure_source "$(basename "$dest")" "$url" "$expect"
  [ -f "$dest" ] || cp -a "$PHPRT_HOME/downloads/$(basename "$dest")" "$dest"
}

dep_lib_dirs() {
  local n d
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib" ] && printf '%s\n' "$d/lib"
    [ -d "$d/lib64" ] && printf '%s\n' "$d/lib64"
  done
  return 0
}

dep_include_flags() {
  local n d out=""
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/include" ] && out+="-I$d/include "
  done
  printf '%s\n' "${out% }"
}

dep_build_flags() {
  local n d out="-Wl,--disable-new-dtags "
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib" ]   && out+="-L$d/lib -Wl,-rpath,$d/lib "
    [ -d "$d/lib64" ] && out+="-L$d/lib64 -Wl,-rpath,$d/lib64 "
  done
  printf '%s\n' "${out% }"
}

dep_pc_path() {
  local n d out=""
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib/pkgconfig" ] && out+="$d/lib/pkgconfig:"
  done
  printf '%s\n' "${out%:}"
}

enabled_dep_names() {
  local d b
  for d in "$DEPS"/*; do
    [ -d "$d" ] || continue
    b="$(basename "$d")"
    printf '%s\n' "${b%-*}"
  done | awk 'NF && !seen[$0]++'
}

dep_env() {
  local names pc cpp ld
  names="$(module_needs "$1")"
  [ -n "$names" ] || return 0
  cpp="$(dep_include_flags $names)"
  ld="$(dep_build_flags $names)"
  pc="$(dep_pc_path $names)"
  [ -n "$cpp" ] && export CPPFLAGS="$cpp${CPPFLAGS:+ $CPPFLAGS}"
  [ -n "$ld" ]  && export LDFLAGS="$ld${LDFLAGS:+ $LDFLAGS}"
  [ -n "$pc" ]  && export PKG_CONFIG_PATH="$pc${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  return 0
}

hash_sig() { sha256sum | cut -c1-20; }

state_dir() { printf '%s/.phprt-state\n' "$TREE"; }
state_file() { printf '%s/%s.sig\n' "$(state_dir)" "$1"; }
state_get() { local f; f="$(state_file "$1")"; [ -f "$f" ] && cat "$f"; }
state_set() {
  local f; f="$(state_file "$1")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$2" > "$f"
}
state_ok() { [ "$(state_get "$1")" = "$2" ]; }

recipe_hash() {
  local f; f="$(module_sh "$1")"
  sha256sum "$f" | cut -d' ' -f1
}

artifact_ok() {
  local d; d="$(dep_dir_of "$1")"
  [ -d "$d" ] || return 1
  find "$d" -maxdepth 3 \( -name '*.so' -o -name '*.so.*' -o -name '*.a' \) \
    -print -quit 2>/dev/null | grep -q .
}

# 同一依赖在 deps/ 下只保留当前版本目录。
# 版本更新后旧目录不会自动删除，而 wheelize_tree 会扫描 deps/*/lib，
# 两份同名 SONAME 会让映射指回旧库。
prune_stale_deps() {
  local n cur old
  [ -n "${DEPS:-}" ] || return 0
  for n in "$@"; do
    cur="$(dep_dir_of "$n")"
    for old in "$DEPS/$n"-*; do
      [ -d "$old" ] || continue
      [ "$old" = "$cur" ] && continue
      log "清理旧依赖 $(basename "$old")"
      rm -rf -- "$old"
    done
  done
}

# 依赖的稳定 ABI 身份：它装的所有库的原始 SONAME 集合（排序去重）。
# 同系列小版本升级 SONAME 不变 => 身份不变 => 消费方签名不变、不重编；
# 跨 ABI 系列（SONAME 变）才变。真实版本记录在配方 stamp、wheel.map 和
# 升级日志里，不进消费方签名（签名是「构建输入有没有变」，不是版本史）。
dep_rt_id() {
  local d f s
  d="$(dep_dir_of "$1")"
  [ -d "$d/lib" ] || [ -d "$d/lib64" ] ||
    die "dep_rt_id: $1 未构建（$d 不存在）"
  {
    for f in "$d"/lib/*.so.* "$d"/lib64/*.so.*; do
      [ -f "$f" ] || continue
      [ -L "$f" ] && continue
      s="$(patchelf --print-soname "$f" 2>/dev/null)" ||
        die "dep_rt_id: 读不到 SONAME: $f"
      [ -n "$s" ] || die "dep_rt_id: 共享库缺少 SONAME: $f"
      printf '%s\n' "$s"
    done
  } | sort -u | paste -sd, -
}

dep_sig() {
  local n="$1" x
  {
    printf 'recipe=%s\n' "$(recipe_hash "$n")"
    case "$n" in
      curl)
        printf 'switches=%s/%s\n' "${ENABLE_OPENSSL:-0}" "${ENABLE_ZLIB:-0}"
        printf 'optional=openssl:%s\n' "$(dep_rt_id openssl)"
        printf 'optional=zlib:%s\n' "$(dep_rt_id zlib)" ;;
      libxml2|libzip)
        printf 'switches=%s\n' "${ENABLE_ZLIB:-0}"
        printf 'optional=zlib:%s\n' "$(dep_rt_id zlib)" ;;
    esac
    for x in $(module_needs "$n"); do
      printf 'need=%s:%s:%s\n' "$x" "$(dep_rt_id "$x")" "$(recipe_hash "$x")"
    done
  } | hash_sig
}

ensure_dep() {
  local n="$1" sig x
  for x in $(module_needs "$n"); do
    ensure_dep "$x"
  done
  sig="$(dep_sig "$n")"
  if state_ok "deps/$n" "$sig" && artifact_ok "$n"; then
    ok "依赖 $n 已完成，跳过"
    return 0
  fi
  log "构建依赖 $n"
  # 外层 dep_sig 已经判定要重编；必须绕过配方内部的 stamp，
  # 否则会出现「外层要重编、内层跳过」，随后把旧产物记成新签名。
  FORCE=1 bash "$(module_sh "$n")"
  artifact_ok "$n" || die "依赖 $n 构建后没有产物"
  state_set "deps/$n" "$sig"
}

ensure_needs() {
  local n
  for n in $(module_needs "$1"); do
    ensure_dep "$n"
  done
  return 0
}

php_sig() { printf 'php=%s\n' "${PHP_VERSION:-}"; }

php_abi_sig() {
  [ -x "$TREE/bin/php" ] || { printf 'missing\n'; return 0; }
  local ld="$TREE/lib:$(ls -d "$TREE"/deps/*/lib 2>/dev/null | tr '\n' ':')"
  LD_LIBRARY_PATH="$ld" "$TREE/bin/php" -i 2>/dev/null |
    grep -E '^(PHP API|Zend Module Api No|Zend Extension Api No|Thread Safety|Debug Build)' |
    sort | hash_sig
}

ext_version() {
  case "$1" in
    redis)    printf '%s\n' "${REDIS_VERSION:-}" ;;
    protobuf) printf '%s\n' "${PROTOBUF_VERSION:-}" ;;
    xdebug)   printf '%s\n' "${XDEBUG_VERSION:-}" ;;
    apcu)     printf '%s\n' "${APCU_VERSION:-}" ;;
    yaml)     printf '%s\n' "${YAML_EXT_VERSION:-}" ;;
    swoole)   printf '%s\n' "${SWOOLE_VERSION:-}" ;;
    *)        printf '\n' ;;
  esac
}

ext_sig() {
  local n="$1" x
  {
    printf 'recipe=%s\n' "$(recipe_hash "$n")"
    printf 'version=%s\n' "$(ext_version "$n")"
    printf 'php_version=%s\n' "${PHP_VERSION:-}"
    printf 'php_abi=%s\n' "$(php_abi_sig)"
    for x in $(module_needs "$n"); do
      printf 'dep=%s:%s\n' "$x" "$(dep_rt_id "$x")"
    done
  } | hash_sig
}

# ---------------------------------------------------------------------------
# 运行时库整理（wheel 方式）：deps/*/lib 里的共享库全部复制到 lib/phprt/，
# 名字换成稳定的运行时名（rt + SONAME 的 ABI 版本号）、SONAME 同步改写，
# 消费方的 DT_NEEDED 一起改，所有 ELF 的 RPATH 改成 $ORIGIN 相对路径。
# 之后运行时不再需要 LD_LIBRARY_PATH。
#
# 命名规则：rt 标签 = 上游 SONAME 的版本号（它就是上游的 ABI 系列信号）。
#   libssl.so.3    -> libssl-rt3.so.3
#   libz.so.1      -> libz-rt1.so.1
#   libicuuc.so.79 -> libicuuc-rt79.so.79
#   libyaml-0.so.2 -> libyaml-0-rt2.so.2
# 同系列小版本升级 SONAME 不变 => 运行时名不变 => 消费方 DT_NEEDED 零改写；
# 跨 ABI 系列（SONAME 变化）才改名并改写消费方。内容 hash 不再参与命名，
# 完整性改由 wheel.map 里的 sha256 列负责。
# ---------------------------------------------------------------------------
wheel_runtime_name() {   # $1=SONAME
  case "$1" in
    *.so.*) printf '%s-rt%s.so.%s\n' "${1%%.so.*}" "${1##*.so.}" "${1##*.so.}" ;;
    *.so)   printf '%s-rt.so\n'   "${1%.so}" ;;
    *)      printf '%s-rt\n'      "$1" ;;
  esac
}

wheelize_tree() {
  local tree="$1" flat="$1/lib/phprt"
  local f soname new old elf osoname onew dup depdir depver line rest
  need_cmds patchelf sha256sum
  mkdir -p "$flat"

  # 保留上一轮映射：跨 ABI 系列升级时运行时名会变，已改写过的 NEEDED 要能
  # 跟到新名字；同系列小版本升级名字不变，映射为空、零改写。
  local prev="$flat/wheel.map.prev" repl="$flat/.replace"
  if [ -f "$flat/wheel.map" ]; then cp -f "$flat/wheel.map" "$prev"; else : > "$prev"; fi
  : > "$flat/wheel.map"
  rm -f "$flat"/*.so*

  for f in "$tree"/deps/*/lib/*.so.* "$tree"/deps/*/lib64/*.so.*; do
    [ -f "$f" ] || continue
    [ -L "$f" ] && continue
    soname="$(patchelf --print-soname "$f" 2>/dev/null || true)"
    [ -n "$soname" ] || soname="$(basename "$f")"
    # 运行时名取自 SONAME 的 ABI 版本号；没有版本号的 SONAME 无法生成
    # 有 ABI 语义的名字，直接失败（不静默回退）
    case "$soname" in
      *.so.*) ;;
      *) die "SONAME 没有 ABI 版本号，无法生成运行时名: $soname（$f）" ;;
    esac
    new="$(wheel_runtime_name "$soname")"
    # 依赖版本取自 deps 目录名（deps/<库>-<版本>/lib，目录名规则由
    # dep_dir_of 保证：<库名>-<版本>）；只作记录用，不参与命名
    depdir="$(basename "$(dirname "$(dirname "$f")")")"
    depver="${depdir#*-}"
    cp -af "$f" "$flat/$new"
    patchelf --set-soname "$new" --set-rpath '$ORIGIN' --force-rpath "$flat/$new"
    # sha256 列最后统一回填（见函数尾部）：所有 patchelf 之后才算
    printf '%s|%s|%s|-\n' "$soname" "$new" "$depver" >> "$flat/wheel.map"
    # 注意：deps/*/lib 保持原样，不动它的软链。它是构建期的链接目录，
    # 里面的库名字还是原始 SONAME；改过名字的副本只放在 lib/phprt，
    # 消费方的 DT_NEEDED 由下面的 --replace-needed 统一改写。
    # （如果把 deps 的 .so 软链指到 lib/phprt，后续 add ext 链接时
    #   ld 会跟不上二级依赖的运行时名，报 undefined reference。）
  done

  if [ ! -s "$flat/wheel.map" ]; then
    rm -f "$prev"
    return 0
  fi

  # 同一原始 SONAME / 同一运行时名 只能对应一个文件。出现重复通常是旧
  # 依赖目录没清掉，继续往下走会让 patchelf 只改到其中一份，运行时可能
  # 仍用旧库。
  dup="$(cut -d'|' -f1 "$flat/wheel.map" | sort | uniq -d)"
  dup="$dup$(cut -d'|' -f2 "$flat/wheel.map" | sort | uniq -d)"
  if [ -n "$dup" ]; then
    printf '%s\n' "$dup" >&2
    die "wheel.map 中同一个 SONAME / 运行时名 对应多个文件（旧依赖目录没清干净？）"
  fi

  # 本轮 原始SONAME -> 运行时名；上一轮的运行时名 -> 本轮运行时名。
  # repl 前半部分是本轮映射（首次 wheel 化用），后半部分是跨轮映射
  # （上一轮的运行时名 -> 本轮运行时名，名字没变就不追加、零改写）。
  # .prev 兼容旧的两列空格格式（hash 时代）。
  cp -f "$flat/wheel.map" "$repl"
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    case "$line" in
      *\|*) osoname="${line%%|*}"; rest="${line#*|}"; onew="${rest%%|*}" ;;
      *)     osoname="${line%% *}"; onew="${line#* }" ;;
    esac
    [ -n "$onew" ] || continue
    new="$(awk -F'|' -v s="$osoname" '$1 == s { print $2; exit }' "$flat/wheel.map")"
    [ -n "$new" ] && [ "$onew" != "$new" ] && printf '%s|%s\n' "$onew" "$new" >> "$repl"
  done < "$prev"

  while IFS='|' read -r old new rest; do
    [ -n "$old" ] || continue
    for elf in "$tree"/bin/* "$flat"/*.so* "$tree"/lib/*.so* \
               "$tree"/lib/php/extensions/*/*.so; do
      [ -f "$elf" ] || continue
      [ -L "$elf" ] && continue
      readelf -h "$elf" >/dev/null 2>&1 || continue
      patchelf --replace-needed "$old" "$new" "$elf" 2>/dev/null || true
    done
  done < "$repl"
  rm -f "$repl" "$prev"

  for elf in "$tree"/bin/*; do
    [ -f "$elf" ] || continue
    [ -L "$elf" ] && continue
    readelf -h "$elf" >/dev/null 2>&1 || continue
    patchelf --set-rpath '$ORIGIN/../lib/phprt' --force-rpath "$elf"
  done
  for elf in "$flat"/*.so*; do
    [ -f "$elf" ] && patchelf --set-rpath '$ORIGIN' --force-rpath "$elf"
  done
  for elf in "$tree"/lib/php/extensions/*/*.so; do
    [ -f "$elf" ] && patchelf --set-rpath '$ORIGIN/../../../phprt' --force-rpath "$elf"
  done

  # sha256 必须最后算：set-soname / replace-needed / set-rpath 都会改字节，
  # 任何一步之后算都不是最终内容。回填第 4 列。
  local tmpmap="$flat/.wheel.map.tmp"
  : > "$tmpmap"
  while IFS='|' read -r src rt ver sha0; do
    [ -n "$src" ] || continue
    printf '%s|%s|%s|%s\n' "$src" "$rt" "$ver" \
      "$(sha256sum "$flat/$rt" | cut -d' ' -f1)" >> "$tmpmap"
  done < "$flat/wheel.map"
  mv -f "$tmpmap" "$flat/wheel.map"

  ok "运行时库已整理到 $flat（$(wc -l < "$flat/wheel.map") 个）"
}

# 全树 ELF 的链接关系快照：相对路径 | SONAME | 有序 NEEDED | RPATH/RUNPATH。
# phprt update 前后各取一份做 diff，断言消费方的动态链接关系零变化。
elf_link_snapshot() {
  local tree="$1" out="$2" f soname needed rpath
  {
    # 扫整棵树（deps/ 构建目录除外），按 ELF 文件头识别，不靠文件名猜
    while IFS= read -r f; do
      [ -L "$f" ] && continue
      readelf -h "$f" >/dev/null 2>&1 || continue
      soname="$(readelf -d "$f" 2>/dev/null | awk -F'[][]' '/\(SONAME\)/{print $2; exit}')"
      needed="$(readelf -d "$f" 2>/dev/null | awk -F'[][]' '/\(NEEDED\)/{print $2}' | paste -sd, -)"
      rpath="$(readelf -d "$f" 2>/dev/null | awk -F'[][]' '/\(RPATH\)|\(RUNPATH\)/{print $2}' | paste -sd: -)"
      printf '%s|soname=%s|needed=%s|rpath=%s\n' \
        "${f#"$tree"/}" "${soname:-}" "$needed" "${rpath:-}"
    done < <(find "$tree" -path "$tree/deps" -prune -o -type f -print 2>/dev/null | LC_ALL=C sort)
  } > "$out"
}

# wheel 完整性：sha256、文件名与 DT_SONAME 一致、OpenSSL 只许一份且成套、
# 消费方不得残留原始 SONAME。
wheel_integrity_check() {
  local tree="$1" flat="$1/lib/phprt" map="$1/lib/phprt/wheel.map"
  local src rt ver sha f n ss sc elf needed line bad=""
  [ -f "$map" ] || { warn "缺少 $map"; return 1; }
  # 格式必须是 4 列 pipe 分隔（旧 hash 时代的两列空格格式直接判失败）
  awk -F'|' 'NF != 4 { bad=1 } END { exit bad ? 1 : 0 }' "$map" || {
    warn "wheel.map 格式不对（需要 4 列 pipe 分隔: 原始SONAME|运行时名|版本|sha256）"
    return 1
  }
  while IFS='|' read -r src rt ver sha; do
    [ -n "$src" ] && [ -n "$rt" ] && [ -n "$sha" ] || { warn "wheel.map 空字段: $src|$rt|$ver|$sha"; return 1; }
    f="$flat/$rt"
    [ -s "$f" ] || { warn "缺文件: $rt"; return 1; }
    [ "$(sha256sum "$f" | cut -d' ' -f1)" = "$sha" ] || { warn "sha256 不符: $rt"; return 1; }
    [ "$(patchelf --print-soname "$f" 2>/dev/null)" = "$rt" ] || { warn "DT_SONAME 与文件名不符: $rt"; return 1; }
  done < "$map"
  # OpenSSL 唯一性：libssl/libcrypto 各一份，且 wheel.map 里版本一致
  if grep -q '^libssl\.so\.3|' "$map"; then
    n="$(find "$flat" -maxdepth 1 -name 'libssl-rt*.so*' | wc -l)"
    [ "$n" = 1 ] || { warn "libssl 运行时库不止一份 ($n)"; return 1; }
    n="$(find "$flat" -maxdepth 1 -name 'libcrypto-rt*.so*' | wc -l)"
    [ "$n" = 1 ] || { warn "libcrypto 运行时库不止一份 ($n)"; return 1; }
    ss="$(awk -F'|' '$1=="libssl.so.3"{print $3}' "$map")"
    sc="$(awk -F'|' '$1=="libcrypto.so.3"{print $3}' "$map")"
    [ -n "$ss" ] && [ "$ss" = "$sc" ] || { warn "libssl/libcrypto 版本不一致: $ss vs $sc"; return 1; }
  fi
  # flat 里的每个 .so* 都必须在 map 里（防遗留旧库：跨系列/迁移后旧名残留）
  while IFS= read -r f; do
    b="$(basename "$f")"
    awk -F'|' -v n="$b" '$2 == n { found=1 } END { exit found ? 0 : 1 }' "$map" || {
      warn "lib/phprt 里有文件不在 wheel.map: $b（遗留旧库？）"
      return 1
    }
  done < <(find "$flat" -maxdepth 1 -name '*.so*' -type f 2>/dev/null)
  # 消费方不得残留原始 SONAME（wheel 化没改写干净会加载到系统库）
  while IFS= read -r elf; do
    needed="$(readelf -d "$elf" 2>/dev/null | awk -F'[][]' '/\(NEEDED\)/{print $2}' | paste -sd' ' -)"
    [ -z "$needed" ] && continue
    while IFS='|' read -r src rt ver sha; do
      [ -n "$src" ] || continue
      case " $needed " in *" $src "*) bad+="$elf -> $src"$'\n' ;; esac
    done < "$map"
  done < <(find "$tree/bin" "$tree/lib" -type f \( -name '*.so*' -o -name php \) 2>/dev/null)
  [ -z "$bad" ] || { printf '%s' "$bad" >&2; warn "消费方仍引用原始 SONAME"; return 1; }
  return 0
}
