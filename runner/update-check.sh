#!/usr/bin/env bash
# phprt：上游版本查询（phprt outdated 用）
# 只读：查询结果只用于显示和写缓存，不下载、不编译、不改 bundle.conf。
# 约定：查询函数成功时向 stdout 打印最新正式版本号并返回 0；
#       查不到 / 网络失败返回 1；没有该软件的查询器返回 2。
# 系列规则与 cmd_update 一致：只认 major.minor 同系列，跨系列不算更新。
set -euo pipefail

# 查询用 GET：先直连；失败且用户没自己设 https_proxy 时，
# 按 bundle.conf 的网络约定自动走代理（PHPRT_PROXY 可改）。
uc_fetch() {
  local url="$1"
  curl -fsSL --connect-timeout 10 --max-time 30 "$url" 2>/dev/null && return 0
  [ -z "${https_proxy:-}${HTTPS_PROXY:-}" ] || return 1
  curl -fsSL --connect-timeout 10 --max-time 30 \
    -x "${PHPRT_PROXY:-http://127.0.0.1:10808}" "$url" 2>/dev/null
}

# 当前版本所在系列（major.minor），跨系列不算更新。
uc_series() { printf '%s\n' "$1" | cut -d. -f1-2; }

# ---------------------------------------------------------------------------
# 各软件查询器。统一口径：排除 draft / prerelease / alpha / beta / rc / dev，
# 版本比较交给 sort -V，不做字符串比较。
# ---------------------------------------------------------------------------

uc_query_php() {    # php.net 官方 JSON：?version=8.5 直接给该系列最新一条
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch "https://www.php.net/releases/index.php?json&version=$series" |
    jq -r 'if type == "object" then .version else empty end' 2>/dev/null |
    grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | head -1)" || return 1
  [ -n "$out" ] || return 1
  [ "$(uc_series "$out")" = "$series" ] || return 1
  printf '%s\n' "$out"
}

uc_query_openssl() {  # GitHub Tags API；tag 形如 openssl-3.5.10
  local series out
  series="$(uc_series "$1")"
  # 用 Tags API 而不是 Releases API：后者每个 release 带完整 changelog，
  # per_page=100 响应约 1MB，慢网络下一个请求要一分钟以上；tags 响应只有
  # 名字+sha（50KB），功能上等价——draft 发布不会打 tag，alpha/beta/rc
  # 后缀的 tag（如 openssl-4.1.0-beta2）被下面的系列正则排除，
  # 排序仍由自己 sort -V 完成，不依赖 API 返回顺序。
  out="$(uc_fetch 'https://api.github.com/repos/openssl/openssl/tags?per_page=100' |
    jq -r 'if type == "array" then .[].name else empty end' 2>/dev/null |
    sed 's/^openssl-//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_libxml2() {  # GNOME GitLab Releases API；tag 形如 v2.15.4
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://gitlab.gnome.org/api/v4/projects/GNOME%2Flibxml2/releases?per_page=100' |
    jq -r 'if type == "array" then .[] else empty end
           | select(.upcoming_release | not)
           | .tag_name' 2>/dev/null |
    sed 's/^v//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

# 通用 GitHub tags 查询：$1=owner/repo $2=当前版本 $3=tag 前缀（v / curl- / 无）
# tag 里的下划线统一转成点（curl 的 8_23_0 -> 8.23.0）
uc_query_gh_tags() {
  local repo="$1" cur="$2" pre="$3" series out
  series="$(uc_series "$cur")"
  out="$(uc_fetch "https://api.github.com/repos/$repo/tags?per_page=100" |
    jq -r 'if type == "array" then .[].name else empty end' 2>/dev/null |
    sed "s/^$pre//" | tr '_' '.' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_zlib()      { uc_query_gh_tags madler/zlib "$1" v; }
uc_query_oniguruma() { uc_query_gh_tags kkos/oniguruma "$1" v; }
uc_query_libzip()    { uc_query_gh_tags nih-at/libzip "$1" v; }
uc_query_libpng()    { uc_query_gh_tags pnggroup/libpng "$1" v; }
uc_query_avif()      { uc_query_gh_tags AOMediaCodec/libavif "$1" v; }
uc_query_libffi()    { uc_query_gh_tags libffi/libffi "$1" v; }

uc_query_curl() { uc_query_gh_tags curl/curl "$1" curl-; }

uc_query_icu() {  # ICU 用 /releases/latest：GitHub 自动排除 draft/prerelease，
  local out        # 且只要一个 release 的体积（全量 releases 的 notes 有几 MB，
  out="$(uc_fetch 'https://api.github.com/repos/unicode-org/icu/releases/latest' |  # per_page 多大都会超时）
    jq -r 'if type == "object" then .tag_name else empty end' 2>/dev/null |
    grep -E '^release-[0-9]+\.[0-9]+$' | sed 's/^release-//')" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_jpeg() {  # libjpeg-turbo 的 dev 版是 X.Y.9x（如 3.1.90），排除
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://api.github.com/repos/libjpeg-turbo/libjpeg-turbo/tags?per_page=100' |
    jq -r 'if type == "array" then .[].name else empty end' 2>/dev/null |
    grep -E "^${series//./\\.}\.[0-9]+(\.[0-9]+)?$" |
    grep -Ev '\.[89][0-9]$' |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_sqlite() {  # 官方 download 页只给最新版；版本号编码 3540100 -> 3.54.1
  local series code maj min pat out
  series="$(uc_series "$1")"
  code="$(uc_fetch 'https://www.sqlite.org/download.html' |
    grep -oE 'sqlite-amalgamation-[0-9]+' | head -1 | grep -oE '[0-9]+$')" || return 1
  [ -n "$code" ] || return 1
  maj=$((code / 1000000)); min=$((code / 10000 % 100)); pat=$((code / 100 % 100))
  out="$maj.$min.$pat"
  [ "$(uc_series "$out")" = "$series" ] || return 1
  printf '%s\n' "$out"
}

uc_query_freetype() {  # Savannah 官方目录列表
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://download.savannah.gnu.org/releases/freetype/' |
    grep -oE 'freetype-2\.[0-9]+\.[0-9]+\.tar\.gz' |
    sed 's/^freetype-//; s/\.tar\.gz$//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_webp() {  # googlesource tags；响应首行是防 XSSI 的 )]}'，跳过
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://chromium.googlesource.com/webm/libwebp/+refs/tags?format=JSON' |
    tail -n +2 | jq -r 'keys[]' 2>/dev/null |
    sed 's/^v//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_aom() {  # Google Cloud Storage 官方目录列表
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://storage.googleapis.com/aom-releases/' |
    grep -oE 'libaom-[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz' |
    sed 's/^libaom-//; s/\.tar\.gz$//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

uc_query_libyaml() {  # pyyaml.org 官方目录列表
  local series out
  series="$(uc_series "$1")"
  out="$(uc_fetch 'https://pyyaml.org/download/libyaml/' |
    grep -oE 'yaml-[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz' |
    sed 's/^yaml-//; s/\.tar\.gz$//' |
    grep -E "^${series//./\\.}\.[0-9]+$" |
    sort -Vu | tail -1)" || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

# 该软件是否有查询器（uc_query_one 的前置判断，避免查之前先跑一次网络）
uc_supported() {
  case "$1" in
    php|openssl|zlib|libxml2|sqlite|curl|oniguruma|libzip|icu|libpng|jpeg|freetype|webp|aom|avif|libffi|libyaml) return 0 ;;
    *) return 1 ;;
  esac
}

# 按软件名分发；返回 2 = 暂无查询器（调用方显示「暂不支持」，不是查询失败）
uc_query_one() {
  case "$1" in
    php)        uc_query_php "$2" ;;
    openssl)    uc_query_openssl "$2" ;;
    zlib)       uc_query_gh_tags madler/zlib "$2" v ;;
    libxml2)    uc_query_libxml2 "$2" ;;
    sqlite)     uc_query_sqlite "$2" ;;
    curl)       uc_query_curl "$2" ;;
    oniguruma)  uc_query_oniguruma "$2" ;;
    libzip)     uc_query_gh_tags nih-at/libzip "$2" v ;;
    icu)        uc_query_icu "$2" ;;
    libpng)     uc_query_gh_tags pnggroup/libpng "$2" v ;;
    jpeg)       uc_query_jpeg "$2" ;;
    freetype)   uc_query_freetype "$2" ;;
    webp)       uc_query_webp "$2" ;;
    aom)        uc_query_aom "$2" ;;
    avif)       uc_query_gh_tags AOMediaCodec/libavif "$2" v ;;
    libffi)     uc_query_gh_tags libffi/libffi "$2" v ;;
    libyaml)    uc_query_libyaml "$2" ;;
    *)          return 2 ;;
  esac
}

# 查询来源标签（写进缓存，供以后 update 直接用缓存数据定位下载页）
uc_source() {
  case "$1" in
    php)      printf 'php.net' ;;
    libxml2)  printf 'gitlab' ;;
    sqlite)   printf 'sqlite.org' ;;
    freetype) printf 'savannah' ;;
    webp)     printf 'googlesource' ;;
    aom)      printf 'googleapis' ;;
    libyaml)  printf 'pyyaml.org' ;;
    *)        printf 'github' ;;
  esac
}

# 显示名：与 phprt 现有中文输出风格一致
uc_name() {
  case "$1" in
    php)     printf 'PHP' ;;
    openssl) printf 'OpenSSL' ;;
    *)       printf '%s' "$1" ;;
  esac
}

# 同系列且更新才报「发现更新」；本地比上游新（罕见）按「已是最新」处理
uc_status() {
  local cur="$1" latest="$2"
  [ "$cur" = "$latest" ] && { printf '已是最新'; return 0; }
  [ "$(uc_series "$cur")" = "$(uc_series "$latest")" ] || { printf '已是最新'; return 0; }
  [ "$(printf '%s\n%s\n' "$cur" "$latest" | sort -V | head -1)" = "$cur" ] &&
    printf '发现更新' || printf '已是最新'
}

# 表格一行
uc_row() {
  printf '%-12s %-12s %-12s %s\n' "$1" "$2" "$3" "$4"
}

# ---------------------------------------------------------------------------
# 终端动态显示（仅 TTY）。查询层与显示层分离：这里只负责画线和原地刷新。
# 布局（行号从表格顶部起算）：
#   1 标题 / 2 空行 / 3 表头 / 4 分隔线 / 5..4+R 数据行 / 5+R 空行
#   6+R 进度行 / 7+R 「正在查询」行（光标锚点，停在该行行尾）
# 刷新一律用相对光标移动（上移 N 行 / 恢复锚点），不假设表格从第 1 行开始，
# 也不反复打印整个表格或清屏。注意：上移指令不改变列，必须先 \r 回第 0 列
# 再清行重写，否则新内容会从旧列开始（行向右偏）。
# ---------------------------------------------------------------------------

uc_tty() { [ -t 1 ]; }

# 首次绘制完整表格；数据行初始为「等待查询」，无查询器的行直接「暂不支持」
uc_draw_table() {
  local -n _p="$1" _c="$2" _d="$3" _q="$4"
  local nq="$5"
  local i st
  printf 'phprt 软件更新检查\n\n'
  printf '%-12s %-12s %-12s %s\n' '软件包' '当前版本' '最新版本' '状态'
  printf '%s\n' '---------------------------------------------'
  for i in "${!_p[@]}"; do
    if [ "${_q[$i]}" = 1 ]; then st='等待查询'; else st='暂不支持'; fi
    uc_row "${_d[$i]}" "${_c[$i]}" '-' "$st"
  done
  printf '\n进度：0/%s\n' "$nq"
  printf '正在查询：等待开始'
}

# 锚点（正在查询行）原地改写
uc_write_querying() { printf '\r\033[2K正在查询：%s' "$1"; }

# 进度行 = 锚点上一行；保存锚点 → 上移 → \r 回第 0 列 → 改写 → 回到锚点
uc_write_progress() {
  printf '\033[s\033[1A\r\033[2K进度：%s/%s\033[u' "$1" "$2"
}

# $1=从锚点上移行数 $2..$5=该行四列内容
uc_update_row() {
  printf '\033[s\033[%dA\r\033[2K%-12s %-12s %-12s %s\033[u' "$1" "$2" "$3" "$4" "$5"
}

# 查询一个软件：成功则把结果追加到 $3（JSONL，供缓存合并），
# 并置 UC_RC / UC_LATEST / UC_STATUS。查询器与缓存格式在此统一收口。
uc_consider() {
  local out st
  out="$(uc_query_one "$1" "$2")" && UC_RC=0 || UC_RC=$?
  UC_LATEST='-'; UC_STATUS='查询失败'
  [ "$UC_RC" = 0 ] || return 0
  UC_LATEST="$out"
  UC_STATUS="$(uc_status "$2" "$out")"
  printf '{"name":"%s","current":"%s","latest":"%s","series":"%s","status":"%s","source":"%s","checked_at":%s}\n' \
    "$1" "$2" "$out" "$(uc_series "$2")" \
    "$( [ "$UC_STATUS" = '发现更新' ] && printf update_available || printf up_to_date )" \
    "$(uc_source "$1")" "$(date +%s)" >> "$3"
}

# ---------------------------------------------------------------------------
# 缓存：每次查询成功就追加一行，最后原子合并进 results.json。
# 查询失败不覆盖历史成功结果；update 阶段直接读这里的版本，不再重复请求。
# 格式：{updated_at, packages: {<名>: {current, latest, series, status,
#        source, checked_at}}}，status 为 update_available / up_to_date。
# ---------------------------------------------------------------------------

uc_cache_save() {  # $1=本次成功的 JSONL 临时文件
  local cdir cfile ctmp oldjson
  [ -s "$1" ] || return 0
  cdir="$PHPRT_HOME/cache/update-check"
  mkdir -p "$cdir"
  cfile="$cdir/results.json"
  ctmp="$cdir/.results.$$"
  oldjson="$( [ -f "$cfile" ] && cat "$cfile" || printf '{"packages":{}}' )"
  if jq -n --argjson old "$oldjson" --slurpfile new "$1" \
    '{updated_at:(now|floor),
      packages: ($old.packages + ($new | map({key:.name, value:.}) | from_entries))}' \
    > "$ctmp" 2>/dev/null; then
    mv -f "$ctmp" "$cfile"
  else
    rm -f "$ctmp"
  fi
}
