#!/usr/bin/env bash
# 上游版本查询：只放「同一类来源怎么查」的通用逻辑。
#
# 约定：
# - recipe 用头部 upstream=<kind>:<参数> 声明来源，driver 调这里对应的函数；
# - 特殊来源由 recipe 自己实现 versions()（输出一行一个稳定版版本号）；
# - 所有函数失败返回非 0，调用方必须能接受「查不到」；
# - OFFLINE=1 时不发起任何请求。
#
# 这里不出现具体包名。

UPSTREAM_CACHE_DIR="${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/phprt/upstream"
UPSTREAM_TTL="${PHPRT_UPSTREAM_TTL:-21600}"   # 6 小时

upstream_offline() { [ "${OFFLINE:-0}" = 1 ]; }

# ---------------------------------------------------------------- 网络 ----

upstream_key() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'
}

# http_get <url> <输出文件>
# 代理顺序：用户代理（https_proxy / HTTPS_PROXY，其次 http_proxy / HTTP_PROXY）
# → 直连。http_proxy 只设置时，HTTPS 请求也用它。
http_get() {
  local url="$1" out="$2" tmp proxy
  local -a hdr=()
  command -v curl >/dev/null 2>&1 || return 1
  case "$url" in
    https://api.github.com/*)
      [ -n "${GITHUB_TOKEN:-}" ] && hdr=(-H "Authorization: Bearer $GITHUB_TOKEN") ;;
  esac
  tmp="$out.part.$$"
  rm -f "$tmp"

  _phprt_curl() {
    curl -fsSL --connect-timeout 10 --max-time 60 -A "phprt/2" "${hdr[@]}" "$@" \
      -o "$tmp" "$url"
  }

  proxy="${https_proxy:-${HTTPS_PROXY:-}}"
  [ -n "$proxy" ] || proxy="${http_proxy:-${HTTP_PROXY:-}}"

  if [ -n "$proxy" ]; then
    if _phprt_curl -x "$proxy"; then
      mv -f "$tmp" "$out"; return 0
    fi
    # 代理不通时再试直连（本次忽略代理变量）
    if https_proxy= HTTPS_PROXY= http_proxy= HTTP_PROXY= _phprt_curl; then
      mv -f "$tmp" "$out"; return 0
    fi
    rm -f "$tmp"
    return 1
  fi

  if _phprt_curl; then
    mv -f "$tmp" "$out"; return 0
  fi
  rm -f "$tmp"
  return 1
}

upstream_cache_path() { printf '%s/%s\n' "$UPSTREAM_CACHE_DIR" "$(upstream_key "$1")"; }

upstream_cache_get() {  # <key> [ttl] -> 输出缓存文件路径
  local f now age
  f="$(upstream_cache_path "$1")"
  [ -s "$f" ] || return 1
  now="$(date +%s)"
  age=$(( now - $(stat -c %Y "$f" 2>/dev/null || echo 0) ))
  [ "$age" -ge 0 ] && [ "$age" -lt "${2:-$UPSTREAM_TTL}" ] || return 1
  printf '%s\n' "$f"
}

upstream_cache_put() {  # <key>，内容从 stdin 读
  local f
  f="$(upstream_cache_path "$1")"
  mkdir -p "$UPSTREAM_CACHE_DIR"
  cat > "$f.part.$$" && mv -f "$f.part.$$" "$f"
}

# upstream_fetch <key> <url> -> 输出响应体，并写缓存
upstream_fetch() {
  local key="$1" url="$2" f tmp body
  upstream_offline && return 1
  if f="$(upstream_cache_get "$key")"; then
    cat "$f"
    return 0
  fi
  tmp="$(mktemp "${TMPDIR:-/tmp}/phprt-upstream.XXXXXX")" || return 1
  if ! http_get "$url" "$tmp"; then
    rm -f "$tmp"
    return 1
  fi
  body="$(cat "$tmp")"
  rm -f "$tmp"
  [ -n "$body" ] || return 1
  printf '%s' "$body" | upstream_cache_put "$key"
  printf '%s\n' "$body"
}

# ------------------------------------------------------------ 版本处理 ----

# 把 tag / 文件名里的版本部分取出来：v1.12.1 / openssl-3.5.9 / curl-8_23_0
upstream_norm_version() {
  sed -E \
    -e 's/^[vV]//' \
    -e 's/^[^0-9]*//' \
    -e 's/_/./g' \
    -e 's/[^0-9.].*$//' \
    -e 's/^\.*//' \
    -e 's/\.*$//' |
    grep -E '^[0-9]+(\.[0-9]+)*$'
}

# tag 比文件名严格一点：只接受「可选的一个字母前缀 + 至少两段的版本号」，
# 避免把 brian-gladman-fcrypt-2008-11-18 这类 tag 当成版本。
upstream_norm_tag() {
  sed -E \
    -e 's/^[A-Za-z][A-Za-z0-9]*-//' \
    -e 's/^[vV]//' \
    -e 's/^([0-9]+)[-_.]([0-9]+([-_.][0-9]+)*)$/\1.\2/' \
    -e 's/[-_]/./g' |
    grep -E '^[0-9]+\.[0-9]+(\.[0-9]+)*$'
}

version_cmp() {  # a b -> -1 / 0 / 1
  local a="$1" b="$2"
  [ "$a" = "$b" ] && { printf '0\n'; return 0; }
  if printf '%s\n%s\n' "$a" "$b" | sort -V -C 2>/dev/null; then
    printf -- '-1\n'
  else
    printf '1\n'
  fi
}

# 预发布版本：PECL 会给出 3.2.17RC1（state 仍标 stable），
# libjpeg-turbo 用 3.1.90 表示 3.2.0 的 beta。两种都算预发布。
version_is_prerelease() {
  local v="$1" last
  case "$v" in
    *[Aa][Ll][Pp][Hh][Aa]*|*[Bb][Ee][Tt][Aa]*|*[Rr][Cc][0-9]*|*[Dd][Ee][Vv]*) return 0 ;;
  esac
  last="${v##*.}"
  [ "$last" -ge 90 ] 2>/dev/null
}

version_stable_filter() {
  local v
  while IFS= read -r v; do
    version_is_prerelease "$v" || printf '%s\n' "$v"
  done
}

version_max() {
  version_stable_filter | grep -E '^[0-9]+(\.[0-9]+)*$' | sort -V | tail -1
}

# 安全更新：major.minor 不变，patch 变大。
# 两段式版本（如 autoconf 2.72 -> 2.73）按 major 不变处理；
# 一段式版本不做安全更新判断。
version_is_safe_update() {
  local cur="$1" new="$2" n
  [ "$(version_cmp "$cur" "$new")" = -1 ] || return 1
  n="$(printf '%s' "$cur" | awk -F. '{print NF}')"
  case "$n" in
    2) [ "${cur%%.*}" = "${new%%.*}" ] ;;
    3|4|5|6) [ "$(printf '%s' "$cur" | cut -d. -f1,2)" = \
              "$(printf '%s' "$new" | cut -d. -f1,2)" ] ;;
    *) return 1 ;;
  esac
}

version_safe_max() {  # <当前版本>，候选从 stdin 读
  local cur="$1" v best=""
  while IFS= read -r v; do
    [ -n "$v" ] || continue
    version_is_prerelease "$v" && continue
    version_is_safe_update "$cur" "$v" || continue
    if [ -z "$best" ] || [ "$(version_cmp "$v" "$best")" = 1 ]; then
      best="$v"
    fi
  done
  printf '%s\n' "$best"
}

# ------------------------------------------------------------ 来源实现 ----

# GitHub Releases/Tags：owner/repo。tag 里的版本用 norm 提取。
upstream_versions_github() {
  local repo="$1" body
  body="$(upstream_fetch "github-tags-$(upstream_key "$repo")" \
    "https://api.github.com/repos/$repo/tags?per_page=100")" || return 1
  printf '%s' "$body" |
    grep -oE '"name"[[:space:]]*:[[:space:]]*"[^"]+"' |
    sed -E 's/.*"([^"]+)"$/\1/' |
    grep -viE '(^|[-_])(alpha|beta|rc|dev|pre)[0-9._-]' |
    upstream_norm_tag | sort -u
}

# PECL：只取 <s>stable</s> 的版本
upstream_versions_pecl() {
  local pkg="$1" body
  body="$(upstream_fetch "pecl-$(upstream_key "$pkg")" \
    "https://pecl.php.net/rest/r/$pkg/allreleases.xml")" || return 1
  printf '%s' "$body" | tr -d '\n' | sed -E 's#</r>#&\n#g' |
    awk '/<s>stable<\/s>/ {
           if (match($0, /<v>[^<]+<\/v>/))
             print substr($0, RSTART+3, RLENGTH-7)
         }' |
    grep -viE '[0-9](alpha|beta|rc|dev|pre)[0-9._-]*$' |
    upstream_norm_version | sort -u
}

# php.net：某个系列的最新发行版（接口一次只返回一个版本）
upstream_versions_phpnet() {
  local series="$1" body
  body="$(upstream_fetch "phpnet-$(upstream_key "$series")" \
    "https://www.php.net/releases/index.php?json&version=$series")" || return 1
  printf '%s' "$body" |
    grep -oE '"version"[[:space:]]*:[[:space:]]*"[0-9.]+"' |
    sed -E 's/.*"([0-9.]+)"/\1/'
}

# 目录索引（GNU FTP、CPAN、zlib fossils 等）：用正则从页面里抠版本
upstream_versions_listing() {
  local url="$1" pat="$2" body
  body="$(upstream_fetch "listing-$(upstream_key "$url")" "$url")" || return 1
  printf '%s' "$body" | grep -oE "$pat" | upstream_norm_version | sort -u
}

# recipe 头部 upstream= 的分发。新增来源时只改这里和上面的实现。
upstream_versions_by_field() {
  local u="$1"
  case "$u" in
    github:*) upstream_versions_github "${u#github:}" ;;
    pecl:*)   upstream_versions_pecl "${u#pecl:}" ;;
    phpnet:*) upstream_versions_phpnet "${u#phpnet:}" ;;
    *)        return 1 ;;
  esac
}

# 某个 recipe 的可用版本列表：优先 recipe 自己的 versions()，其次 latest_version()，
# 最后按头部 upstream= 走公共实现。失败返回非 0。
module_versions() {
  local sh="$1" up
  up="$(recipe_field "$sh" upstream)"
  if recipe_has_func "$sh" versions; then
    bash "$sh" versions 2>/dev/null || return 1
  elif recipe_has_func "$sh" latest_version; then
    bash "$sh" latest_version 2>/dev/null || return 1
  elif [ -n "$up" ] && [ "$up" != none ]; then
    upstream_versions_by_field "$up" || return 1
  else
    return 1
  fi
}
