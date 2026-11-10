#!/usr/bin/env bash
# 源码清单与下载。
#
# 这里只做三件事：
#   1. 从 recipe 的契约字段推导源码 URL / SHA256 / 缓存文件名；
#   2. 按依赖层级排出下载顺序；
#   3. 下载、续传、校验、汇总。
#
# 构建阶段不查版本，也不决定扩展；扩展和方式由调用方设置 SELECTED_EXTS /
# PHPRT_EXT_MODES 后传进来。

# 契约 kind -> 用户看到的目录名
source_kind_label() {
  case "$1" in
    dep)  printf 'deps\n' ;;
    tool) printf 'tools\n' ;;
    ext)  printf 'ext\n' ;;
    php)  printf 'php\n' ;;
    *)    printf '%s\n' "$1" ;;
  esac
}

# 同一站点并发限制用的桶。GitHub 主站、codeload、raw 和常见镜像都算一个桶，
# 避免直连失败后所有 GitHub 任务同时压到同一个镜像上。
source_host_bucket() {
  local url="$1" h
  case "$url" in
    *://*) h="${url#*://}" ;;
    *) h="local" ;;
  esac
  h="${h%%/*}"
  h="${h##*@}"
  h="${h%%:*}"
  h="${h,,}"
  case "$h" in
    ""|local) printf 'local\n' ;;
    github.com|codeload.github.com|raw.githubusercontent.com|objects.githubusercontent.com|api.github.com)
      printf 'github\n' ;;
    gh-proxy.com|ghfast.top|ghproxy.net|gh.llkk.cc)
      printf 'github\n' ;;
    *) printf '%s\n' "$h" ;;
  esac
}

# 源码 URL 变量名：version=LIBCURL_VERSION -> LIBCURL_URL
source_url_var_of() {  # $1=recipe 文件
  local v
  v="$(recipe_version_var "$1")"
  [ -n "$v" ] || return 1
  printf '%s_URL\n' "${v%_VERSION}"
}

source_sha_var_of() {  # $1=recipe 文件
  local v
  v="$(recipe_version_var "$1")"
  [ -n "$v" ] || return 1
  printf '%s_SHA256\n' "${v%_VERSION}"
}

source_file_of_url() {
  local f="${1##*/}"
  printf '%s\n' "${f%%\?*}"
}

# ---------------------------------------------------------------------------
# 清单：每个条目一行
#   layer <TAB> kind <TAB> name <TAB> version <TAB> file <TAB> url <TAB> sha
# kind 用契约的 dep/tool/ext/php；显示时再映射成 deps/tools/ext/php。
# ---------------------------------------------------------------------------
SOURCE_ENTRIES=()
SOURCE_MAX_LAYER=0
SOURCE_EXT_LAYER=0
declare -gA SOURCE_DEPTH=()
declare -gA SOURCE_STACK=()

source_add_entry() {
  local layer="$1" kind="$2" name="$3" ver="$4" url="$5" sha="${6:-}" file
  [ -n "$url" ] || die "模块 $name 没有源码 URL"
  file="$(source_file_of_url "$url")"
  SOURCE_ENTRIES+=("$layer"$'\t'"$kind"$'\t'"$name"$'\t'"$ver"$'\t'"$file"$'\t'"$url"$'\t'"$sha")
}

source_add_php() {
  source_add_entry 0 php php "${PHP_VERSION:-}" "${PHP_URL:-}" "${PHP_SHA256:-}"
}

source_add_module() {  # $1=recipe 文件 $2=layer
  local sh="$1" layer="$2" kind name var ver uvar svar url sha
  kind="$(recipe_kind_of "$sh")"
  name="$(recipe_name_of "$sh")"
  var="$(recipe_version_var "$sh")"
  [ -n "$var" ] || die "$kind/$name 的 recipe 没有 version="
  ver="$(recipe_version "$sh")"
  [ -n "$ver" ] || die "$kind/$name 的版本变量 $var 没有值"
  uvar="$(source_url_var_of "$sh")" || die "$kind/$name 推导不出 URL 变量"
  svar="$(source_sha_var_of "$sh")" || die "$kind/$name 推导不出 SHA256 变量"
  url="${!uvar:-}"
  sha="${!svar:-}"
  source_add_entry "$layer" "$kind" "$name" "$ver" "$url" "$sha"
}

# 被依赖模块的层级：没有依赖 = 1，依赖的最大层级 + 1。
# 计算层级的同时把 dep/tool 加入清单。
# 结果放在 SOURCE_LAST_DEPTH：不能用 $(...) 调用，否则数组修改会丢在子 shell 里。
SOURCE_LAST_DEPTH=0
source_dep_depth() {
  local n="$1" sh x d max=0 layer
  if [ -n "${SOURCE_DEPTH[$n]:-}" ]; then
    SOURCE_LAST_DEPTH="${SOURCE_DEPTH[$n]}"
    return 0
  fi
  if [ -n "${SOURCE_STACK[$n]:-}" ]; then
    warn "依赖成环: $n（已忽略环上的重复层级）"
    SOURCE_LAST_DEPTH=1
    return 0
  fi
  SOURCE_STACK[$n]=1
  sh="$(module_sh "$n")"
  while IFS= read -r x; do
    [ -n "$x" ] || continue
    source_dep_depth "$x"
    d="$SOURCE_LAST_DEPTH"
    [ "$d" -gt "$max" ] && max="$d"
  done < <(module_needs "$n")
  unset 'SOURCE_STACK[$n]'
  layer=$((max + 1))
  SOURCE_DEPTH[$n]="$layer"
  case "$(recipe_kind_of "$sh")" in
    dep|tool) source_add_module "$sh" "$layer" ;;
    *) : ;;
  esac
  SOURCE_LAST_DEPTH="$layer"
}

# 被选中扩展的依赖。builtin 的开关在 PHP configure 里可能是关的（shared），
# 但独立 .so 或静态链接仍然要它的依赖，所以这里强制按 1 取 --needs。
source_root_needs() {
  local e="$1" sh out t
  if [ -f "$BUNDLE/builtin/$e.sh" ]; then
    out="$(needs_with_switch "$e" || true)"
    sh="$BUNDLE/builtin/$e.sh"
    t="$(recipe_tools_of "$sh")"
    printf '%s\n' $out $t | awk 'NF && !seen[$0]++'
  else
    module_needs "$e"
  fi
}

# 生成清单。调用前必须设置 BUNDLE / SELECTED_EXTS / PHPRT_EXT_MODES。
source_plan() {
  local exts="$1" e sh kind max=0 d ext_count=0
  local -a uniq=()

  SOURCE_ENTRIES=()
  SOURCE_DEPTH=()
  SOURCE_STACK=()
  SOURCE_MAX_LAYER=0
  SOURCE_EXT_LAYER=0

  export SELECTED_EXTS="$exts"
  builtin_apply_switches
  apply_ext_opts

  source_add_php

  for e in $exts; do
    case " ${uniq[*]:-} " in
      *" $e "*) continue ;;
    esac
    uniq+=("$e")
  done

  for e in "${uniq[@]}"; do
    ext_recipe_sh "$e" >/dev/null 2>&1 || die "未知扩展: $e$(ext_name_hint "$e")"
    while IFS= read -r x; do
      [ -n "$x" ] || continue
      source_dep_depth "$x"
      d="$SOURCE_LAST_DEPTH"
      [ "$d" -gt "$max" ] && max="$d"
    done < <(source_root_needs "$e")
  done

  local ext_layer=$((max + 1))
  for e in "${uniq[@]}"; do
    sh="$(ext_recipe_sh "$e")" || continue
    kind="$(recipe_kind_of "$sh")"
    [ "$kind" = builtin ] && continue
    source_add_module "$sh" "$ext_layer"
    ext_count=$((ext_count + 1))
  done
  [ "$ext_count" -gt 0 ] && SOURCE_EXT_LAYER="$ext_layer"

  SOURCE_MAX_LAYER=0
  local line l
  for line in "${SOURCE_ENTRIES[@]}"; do
    l="${line%%$'\t'*}"
    [ "$l" -gt "$SOURCE_MAX_LAYER" ] && SOURCE_MAX_LAYER="$l"
  done
  return 0
}

source_plan_lines() {
  [ ${#SOURCE_ENTRIES[@]} -gt 0 ] || return 0
  printf '%s\n' "${SOURCE_ENTRIES[@]}" |
    LC_ALL=C sort -t $'\t' -k1,1n -s
}

source_plan_print_urls() {
  local layer kind name ver file url sha label
  while IFS=$'\t' read -r layer kind name ver file url sha; do
    [ -n "$url" ] || continue
    if [ "$kind" = php ]; then
      printf '#php %s\n%s\n' "$file" "$url"
    else
      label="$(source_kind_label "$kind")"
      printf '#%s-%s %s\n%s\n' "$label" "$name" "$file" "$url"
    fi
  done < <(source_plan_lines)
}

# ---------------------------------------------------------------------------
# 下载
# ---------------------------------------------------------------------------

source_human_size() {
  local n
  n="$(stat -c %s "$1" 2>/dev/null || printf 0)"
  awk -v n="$n" 'BEGIN {
    if (n >= 1048576) printf "%.1f MB", n / 1048576;
    else if (n >= 1024) printf "%.1f KB", n / 1024;
    else printf "%d B", n;
  }'
}

# GitHub 镜像：先直连，失败后按顺序换。只对 https://github.com/ 生效。
source_github_mirrors() {
  if [ -n "${PHPRT_GITHUB_MIRRORS+x}" ]; then
    printf '%s\n' ${PHPRT_GITHUB_MIRRORS}
    return 0
  fi
  printf '%s\n' \
    "https://gh-proxy.com/" \
    "https://ghfast.top/" \
    "https://ghproxy.net/" \
    "https://gh.llkk.cc/"
}

source_url_candidates() {  # $1=原始 URL
  local url="$1" m
  printf '%s\n' "$url"
  case "$url" in
    https://github.com/*)
      while IFS= read -r m; do
        [ -n "$m" ] || continue
        case "$m" in
          */) printf '%s%s\n' "$m" "$url" ;;
          *)  printf '%s/%s\n' "$m" "$url" ;;
        esac
      done < <(source_github_mirrors)
      ;;
  esac
}

# 代理只用用户配置的：https_proxy → HTTPS_PROXY → http_proxy → HTTP_PROXY。
# 只设 HTTP_PROXY 时，HTTPS 下载也显式传给 curl -x。
source_proxy_args() {
  local p
  p="${https_proxy:-${HTTPS_PROXY:-${http_proxy:-${HTTP_PROXY:-}}}}"
  [ -n "$p" ] && printf '%s\n' "$p"
  return 0
}

source_curl_proxy_opts() {  # 输出给 curl 的参数，每行一个
  local p np
  p="$(source_proxy_args)"
  [ -n "$p" ] || return 0
  printf '%s\n' -x "$p"
  np="${no_proxy:-${NO_PROXY:-}}"
  if [ -n "$np" ]; then
    printf '%s\n' --noproxy "$np"
  fi
}

source_aria2_proxy_opts() {  # 输出给 aria2c 的参数，每行一个
  local p np
  p="$(source_proxy_args)"
  [ -n "$p" ] || return 0
  printf '%s\n' "--all-proxy=$p"
  np="${no_proxy:-${NO_PROXY:-}}"
  if [ -n "$np" ]; then
    printf '%s\n' "--no-proxy=$np"
  fi
}

source_aria2c_enabled() {
  [ "${PHPRT_ARIA2:-1}" = 1 ] && command -v aria2c >/dev/null 2>&1
}

source_fetch_one() {  # $1=url $2=目标文件 $3=超时秒数
  local url="$1" dest="$2" mt="$3" rc=0 conns="${PHPRT_ARIA2_CONNECTIONS:-4}"
  local -a px=()

  case "$conns" in
    ''|*[!0-9]*|0) conns=4 ;;
  esac

  if source_aria2c_enabled; then
    mapfile -t px < <(source_aria2_proxy_opts)
    local -a runner=()
    command -v timeout >/dev/null 2>&1 && runner=(timeout "$mt")
    "${runner[@]}" aria2c \
      --continue=true \
      --max-connection-per-server="$conns" \
      --split="$conns" \
      --min-split-size=1M \
      --file-allocation=none \
      --auto-file-renaming=false \
      --allow-overwrite=true \
      --max-tries=2 --retry-wait=1 \
      --connect-timeout=8 --timeout=60 \
      --console-log-level=warn --summary-interval=0 --download-result=hide \
      --user-agent=phprt/2 \
      "${px[@]}" \
      --dir="$DOWNLOADS" --out="$(basename "$dest")" "$url" >/dev/null || rc=$?
    if [ "$rc" -eq 0 ]; then
      return 0
    fi
    # aria2c 不支持或失败时退回 curl；先清掉它的控制文件，避免干扰续传。
    rm -f "$dest.aria2"
  fi

  mapfile -t px < <(source_curl_proxy_opts)
  rc=0
  curl -fsSL --connect-timeout 8 --max-time "$mt" \
    --speed-limit 1024 --speed-time 60 \
    "${px[@]}" -C - -o "$dest" "$url" || rc=$?
  return "$rc"
}

source_curl_download() {  # $1=url $2=目标文件
  local url="$1" dest="$2" rc=0 timeout direct_timeout candidate mt
  local first=1
  command -v curl >/dev/null 2>&1 || { warn "缺少 curl，无法下载 $url"; return 127; }
  timeout="${PHPRT_DOWNLOAD_TIMEOUT:-900}"
  case "$timeout" in
    ''|*[!0-9]*|0) timeout=900 ;;
  esac
  direct_timeout="${PHPRT_GITHUB_DIRECT_TIMEOUT:-60}"
  case "$direct_timeout" in
    ''|*[!0-9]*|0) direct_timeout=60 ;;
  esac

  while IFS= read -r candidate; do
    [ -n "$candidate" ] || continue
    mt="$timeout"
    if [ "$first" = 1 ] && [ "$candidate" = "$url" ]; then
      case "$url" in
        https://github.com/*) mt="$direct_timeout" ;;
      esac
    fi
    if [ "$candidate" != "$url" ]; then
      log "GitHub 直连失败，换镜像: $candidate"
    fi
    rc=0
    source_fetch_one "$candidate" "$dest" "$mt" || rc=$?
    if [ "$rc" -eq 0 ]; then
      return 0
    fi
    # 416 / 33：文件可能已经完整；交给后面的校验判断。
    if { [ "$rc" -eq 22 ] || [ "$rc" -eq 33 ]; } && [ -s "$dest" ]; then
      return 0
    fi
    first=0
  done < <(source_url_candidates "$url")
  return "$rc"
}

source_remote_size() {  # $1=url -> Content-Length，取不到返回 1
  local url="$1" out candidate size
  local -a px=()
  mapfile -t px < <(source_curl_proxy_opts)
  while IFS= read -r candidate; do
    [ -n "$candidate" ] || continue
    out="$(curl -fsIL --connect-timeout 8 --max-time 30 \
      "${px[@]}" "$candidate" 2>/dev/null)" || continue
    size="$(printf '%s\n' "$out" | tr -d '\r' |
      awk 'tolower($1) == "content-length:" { v = $2 } END { if (v != "") print v }')"
    if [ -n "$size" ]; then
      printf '%s\n' "$size"
      return 0
    fi
  done < <(source_url_candidates "$url")
  return 1
}

# 已存在的文件是否可以直接跳过。返回 0=跳过，1=需要下载/续传。
source_cached_ok() {  # $1=文件 $2=sha $3=url
  local file="$1" sha="$2" url="$3" actual remote local_size
  [ -s "$file" ] || return 1

  if [ -n "$sha" ]; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
    [ "$actual" = "$sha" ] && return 0
    # 可能只是没下完；交给 curl -C - 续传，下载完成后再判断。
    return 1
  fi

  if [ "${OFFLINE:-0}" = 1 ]; then
    warn "离线模式，无 SHA256，按已存在处理: $(basename "$file")"
    return 0
  fi

  remote="$(source_remote_size "$url" || true)"
  if [ -z "$remote" ]; then
    warn "无 SHA256，服务端也未提供大小，按已存在处理: $(basename "$file")"
    return 0
  fi
  local_size="$(stat -c %s "$file" 2>/dev/null || printf 0)"
  [ "$local_size" = "$remote" ] && return 0
  if [ "$local_size" -gt "$remote" ]; then
    warn "删除 $(basename "$file")：本地比远端大（本地 $local_size，远端 $remote）"
    rm -f "$file"
    return 1
  fi
  return 1
}

# 下载完成后校验。失败时删除并打印原因，由调用方决定是否重试一次。
source_verify_file() {  # $1=文件 $2=sha $3=url
  local file="$1" sha="$2" url="$3" actual remote local_size
  [ -s "$file" ] || { warn "下载完成但文件为空: $(basename "$file")"; rm -f "$file"; return 1; }

  if [ -n "$sha" ]; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
    [ "$actual" = "$sha" ] && return 0
    warn "删除 $(basename "$file")：SHA256 不匹配"
    printf '    期望: %s\n    实际: %s\n' "$sha" "$actual" >&2
    rm -f "$file"
    return 1
  fi

  if [ "${OFFLINE:-0}" = 1 ]; then
    warn "离线模式，已跳过校验: $(basename "$file")"
    return 0
  fi
  remote="$(source_remote_size "$url" || true)"
  if [ -z "$remote" ]; then
    warn "无 SHA256，已跳过校验: $(basename "$file")"
    return 0
  fi
  local_size="$(stat -c %s "$file" 2>/dev/null || printf 0)"
  [ "$local_size" = "$remote" ] && return 0
  warn "删除 $(basename "$file")：大小不一致（本地 $local_size，远端 $remote）"
  rm -f "$file"
  return 1
}

source_record_failure() {  # $1=label $2=name $3=ver $4=url [$5=原因]
  [ -n "${SOURCE_FAILED_FILE:-}" ] || return 0
  printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "${5:-}" >> "$SOURCE_FAILED_FILE"
}

source_download_entry() {  # $1..$5=kind name ver file url $6=sha
  local kind="$1" name="$2" ver="$3" file="$4" url="$5" sha="${6:-}"
  local label dest rc=0
  label="$(source_kind_label "$kind")"
  dest="$DOWNLOADS/$file"

  mkdir -p "$DOWNLOADS"

  source_curl_download "$url" "$dest" || rc=$?
  if [ "$rc" -ne 0 ]; then
    if [ "$rc" -eq 28 ]; then
      warn "[$label] $name $ver 超时（已下载部分保留，下次运行可继续）"
    else
      warn "[$label] $name $ver 失败（文件保留，下次运行可继续）"
    fi
    printf '    %s\n' "$url" >&2
    source_record_failure "$label" "$name" "$ver" "$url"
    return 1
  fi

  if ! source_verify_file "$dest" "$sha" "$url"; then
    log "重试 [$label] $name $ver"
    rc=0
    source_curl_download "$url" "$dest" || rc=$?
    if [ "$rc" -ne 0 ]; then
      if [ "$rc" -eq 28 ]; then
        warn "[$label] $name $ver 重试超时"
      else
        warn "[$label] $name $ver 重试失败"
      fi
      printf '    %s\n' "$url" >&2
      source_record_failure "$label" "$name" "$ver" "$url"
      return 1
    fi
    if ! source_verify_file "$dest" "$sha" "$url"; then
      warn "[$label] $name $ver 校验仍失败"
      source_record_failure "$label" "$name" "$ver" "$url"
      return 1
    fi
  fi

  ok "[$label] $name $ver 完成（$(source_human_size "$dest")）"
}

# 前置下载。$1=扩展列表 $2=1 时额外写 download-urls-*.txt
# 返回非 0 表示有文件失败；调用方应停止构建。
prefetch_sources() {
  local exts="$1" write_list="${2:-0}" proxy
  proxy="$(source_proxy_args)"
  if [ -n "$proxy" ]; then
    printf '代理: %s\n' "$proxy"
  else
    printf '代理: 未设置（直连）\n'
  fi
  if source_aria2c_enabled; then
    printf '下载器: aria2c（每文件 %s 连接）\n' "${PHPRT_ARIA2_CONNECTIONS:-4}"
  else
    printf '下载器: curl\n'
  fi
  if source_ui_progress; then
    ( prefetch_sources_progress "$exts" "$write_list" )
  else
    prefetch_sources_plain "$exts" "$write_list"
  fi
}

prefetch_sources_plain() {
  local exts="$1" write_list="${2:-0}" jobs="${PHPRT_DOWNLOAD_JOBS:-10}"
  local host_jobs="${PHPRT_DOWNLOAD_HOST_JOBS:-${PHPRT_HOST_JOBS:-2}}"
  local total idx=0 running=0
  local line l kind name ver file url sha label src
  local plan_file failed_file rc=0

  case "$jobs" in
    ''|*[!0-9]*|0) die "--download-jobs 需要正整数: $jobs" ;;
  esac
  case "$host_jobs" in
    ''|*[!0-9]*|0) die "--download-host-jobs 需要正整数: $host_jobs" ;;
  esac

  source_plan "$exts"

  [ -n "${DOWNLOADS:-}" ] || die "内部错误：没有设置 DOWNLOADS"
  total="$(source_plan_lines | wc -l)"
  [ "$total" -gt 0 ] || return 0

  mkdir -p "$DOWNLOADS"
  log "源码目录: $DOWNLOADS"
  printf '    并发 %s（每站点 %s）；已存在的文件会跳过；install / add ext / update 也使用这个目录。\n' \
    "$jobs" "$host_jobs"

  plan_file="$(mktemp "${TMPDIR:-/tmp}/phprt-sources.XXXXXX")"
  failed_file="$(mktemp "${TMPDIR:-/tmp}/phprt-sources-failed.XXXXXX")"
  SOURCE_FAILED_FILE="$failed_file"
  export SOURCE_FAILED_FILE
  source_plan_lines > "$plan_file"

  if [ "$write_list" = 1 ]; then
    local list="$DOWNLOADS/download-urls-php${PHP_VERSION}-$(date +%Y%m%d).txt"
    source_plan_print_urls > "$list"
    printf '下载清单: %s\n' "$list"
  fi

  # 先按清单顺序检查所有包：已存在的直接打印，其余进队列。
  # 队列跨层连续启动，受全局并发和单站点并发限制。
  local -a plan_lines=() queue=()
  mapfile -t plan_lines < "$plan_file"
  local entry qi qkind qname qver qfile qurl qsha
  for line in "${plan_lines[@]}"; do
    IFS=$'\t' read -r l kind name ver file url sha <<< "$line"
    idx=$((idx + 1))
    label="$(source_kind_label "$kind")"

    if source_cached_ok "$DOWNLOADS/$file" "$sha" "$url"; then
      printf '[%d/%d] [%s] %s %s  已存在\n' "$idx" "$total" "$label" "$name" "$ver"
      continue
    fi

    # 旧构建目录 / 仓库种子里有同一个包时，直接复用，不联网。
    if [ ! -s "$DOWNLOADS/$file" ]; then
      src="$(find_source "$file" || true)"
      [ "$src" = "$DOWNLOADS/$file" ] && src=""
      if [ -n "$src" ]; then
        copy_hard "$src" "$DOWNLOADS/$file"
        if source_cached_ok "$DOWNLOADS/$file" "$sha" "$url"; then
          printf '[%d/%d] [%s] %s %s  已存在（复用缓存）\n' \
            "$idx" "$total" "$label" "$name" "$ver"
          continue
        fi
      fi
    fi

    if [ "${OFFLINE:-0}" = 1 ]; then
      printf '[%d/%d] [%s] %s %s  缺少\n' "$idx" "$total" "$label" "$name" "$ver"
      printf '      %s\n' "$url"
      source_record_failure "$label" "$name" "$ver" "$url"
      continue
    fi

    queue+=("$idx"$'\t'"$line")
  done

  local -A qhost=() qline=() qstarted=() pid_of=() host_running=()
  local qpending=0 qh

  # queue 已经在前面按清单顺序装好；这里补上 host 和启动标记。
  for entry in "${queue[@]}"; do
    qi="${entry%%$'\t'*}"
    qline[$qi]="${entry#*$'\t'}"
    IFS=$'\t' read -r l qkind qname qver qfile qurl qsha <<< "${qline[$qi]}"
    qhost[$qi]="$(source_host_bucket "$qurl")"
    qstarted[$qi]=0
    qpending=$((qpending + 1))
  done

  start_more_plain() {
    for entry in "${queue[@]}"; do
      [ "$running" -lt "$jobs" ] || break
      qi="${entry%%$'\t'*}"
      [ "${qstarted[$qi]}" = 1 ] && continue
      qh="${qhost[$qi]}"
      [ "${host_running[$qh]:-0}" -ge "$host_jobs" ] && continue

      IFS=$'\t' read -r l qkind qname qver qfile qurl qsha <<< "${qline[$qi]}"
      label="$(source_kind_label "$qkind")"
      printf '[%d/%d] [%s] %s %s  下载中\n      %s\n' \
        "$qi" "$total" "$label" "$qname" "$qver" "$qurl"
      (
        source_download_entry "$qkind" "$qname" "$qver" "$qfile" "$qurl" "$qsha"
      ) &
      pid_of[$qi]=$!
      qstarted[$qi]=1
      host_running[$qh]=$(( ${host_running[$qh]:-0} + 1 ))
      running=$((running + 1))
      qpending=$((qpending - 1))
    done
  }

  while [ "$qpending" -gt 0 ] || [ "$running" -gt 0 ]; do
    start_more_plain
    [ "$running" -gt 0 ] || continue
    sleep 0.1
    local -a keys=("${!pid_of[@]}")
    for qi in "${keys[@]}"; do
      if ! kill -0 "${pid_of[$qi]}" 2>/dev/null; then
        wait "${pid_of[$qi]}" 2>/dev/null || true
        qh="${qhost[$qi]}"
        host_running[$qh]=$(( ${host_running[$qh]:-1} - 1 ))
        unset 'pid_of[$qi]'
        running=$((running - 1))
      fi
    done
  done

  if [ -s "$failed_file" ]; then
    rc=1
    printf '\n' >&2
    warn "有源码包下载失败："
    while IFS=$'\t' read -r label name ver url reason; do
      printf '  [%s] %s %s\n    %s\n' "$label" "$name" "$ver" "$url" >&2
      [ -n "$reason" ] && printf '    原因: %s\n' "$reason" >&2
      case "$url" in
        https://github.com/*)
          local m
          m="$(source_github_mirrors | head -1 || true)"
          [ -n "$m" ] && printf '    镜像: %s%s\n' "${m%/}/" "$url" >&2
          ;;
      esac
    done < "$failed_file"
    printf '    可复制上面的链接手动下载，放到 %s 后重试。\n' "$DOWNLOADS" >&2
  fi

  rm -f "$plan_file" "$failed_file"
  unset SOURCE_FAILED_FILE
  return "$rc"
}

# ---------------------------------------------------------------------------
# 终端进度框：gum 画外框，多行进度由这里轮询文件大小维护。
# 只在 stdout 是终端、能找到 gum 且没有设置 PHPRT_UI=plain 时启用。
# ---------------------------------------------------------------------------
source_ui_progress() {
  case "${PHPRT_UI:-auto}" in
    plain) return 1 ;;
    progress)
      if command -v gum >/dev/null 2>&1; then
        return 0
      fi
      warn "找不到 gum，改用逐行下载日志"
      return 1
      ;;
    auto)
      [ -t 1 ] || return 1
      [ "${TERM:-dumb}" != dumb ] || return 1
      [ -z "${NO_COLOR:-}" ] || return 1
      command -v gum >/dev/null 2>&1
      ;;
    *) return 1 ;;
  esac
}

source_progress_bar() {  # $1=百分比 $2=宽度
  local pct="$1" width="$2" fill i out=""
  [ "$pct" -lt 0 ] && pct=0
  [ "$pct" -gt 100 ] && pct=100
  fill=$((pct * width / 100))
  for ((i = 0; i < width; i++)); do
    if [ "$i" -lt "$fill" ]; then out+="█"; else out+="░"; fi
  done
  printf '%s' "$out"
}

source_progress_fit() {  # $1=文本 $2=最大字符数
  local s="$1" max="$2"
  [ "$max" -lt 1 ] && max=1
  if [ "${#s}" -gt "$max" ]; then
    printf '%s…' "${s:0:max-1}"
  else
    printf '%s' "$s"
  fi
}

source_progress_rate() {  # $1=bytes/s
  local n="$1"
  awk -v n="$n" 'BEGIN {
    if (n >= 1048576) printf "%.1f MB/s", n / 1048576;
    else if (n >= 1024) printf "%.0f KB/s", n / 1024;
    else printf "%d B/s", n;
  }'
}

declare -gA SOURCE_UI_SIZE=()
declare -gA SOURCE_UI_TS=()
declare -gA SOURCE_UI_RATE=()

source_progress_speed() {  # $1=idx $2=当前大小 -> 设置 SOURCE_UI_SPEED_TEXT
  local idx="$1" cur="$2" now delta dt inst old
  now="$(date +%s%N)"
  if [ -n "${SOURCE_UI_TS[$idx]:-}" ]; then
    delta=$((cur - SOURCE_UI_SIZE[$idx]))
    dt=$(((now - SOURCE_UI_TS[$idx]) / 1000000))
    if [ "$dt" -ge 80 ]; then
      [ "$delta" -lt 0 ] && delta=0
      inst=$((delta * 1000 / dt))
      old="${SOURCE_UI_RATE[$idx]:-0}"
      SOURCE_UI_RATE[$idx]=$(((old * 2 + inst) / 3))
    fi
  fi
  SOURCE_UI_SIZE[$idx]="$cur"
  SOURCE_UI_TS[$idx]="$now"
  SOURCE_UI_SPEED_TEXT="$(source_progress_rate "${SOURCE_UI_RATE[$idx]:-0}")"
}

source_progress_write_state() {  # $1=文件，其余字段
  local f="$1"; shift
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$(IFS=$'\t'; printf '%s' "$*")" > "$f.tmp.$$"
  mv -f "$f.tmp.$$" "$f"
}

source_progress_read_state() {  # $1=文件 -> 设置 ST_*
  local f="$1"
  ST_STATE=wait ST_LABEL=deps ST_NAME=? ST_VER=- ST_FILE=- ST_URL=- ST_TOTAL=0
  [ -f "$f" ] || return 0
  IFS=$'\t' read -r ST_STATE ST_LABEL ST_NAME ST_VER ST_FILE ST_URL ST_TOTAL < "$f" || true
  case "$ST_TOTAL" in
    ''|*[!0-9]*) ST_TOTAL=0 ;;
  esac
}

# 进度行。$1=idx $2=终端宽度
# 结果放在 SOURCE_UI_ROW；不能用 $(...) 调用，否则速度采样会丢在子 shell 里。
source_progress_row() {
  local idx="$1" cols="$2" cur=0 pct=0 barw tag speed bar spinner
  local compact=0 tag_max=24
  local f="$SOURCE_UI_STATE_DIR/$idx"
  source_progress_read_state "$f"

  tag="[${ST_LABEL}] ${ST_NAME} ${ST_VER}"
  if [ "$cols" -lt 76 ]; then
    compact=1
    tag_max=18
    barw=$((cols - 30))
    [ "$barw" -lt 6 ] && barw=6
  else
    barw=$((cols - 73))
    [ "$barw" -lt 8 ] && barw=8
    [ "$barw" -gt 40 ] && barw=40
  fi
  tag="$(printf '%.*s' "$tag_max" "$tag")"

  case "$ST_STATE" in
    done)
      printf -v SOURCE_UI_ROW '  %-26s %s✓ 完成%s %s' "$tag" "$C_G" "$C_R" \
        "$(source_human_size "$DOWNLOADS/$ST_FILE")"
      ;;
    skip)
      printf -v SOURCE_UI_ROW '  %-26s %s✓ 已存在%s' "$tag" "$C_G" "$C_R"
      ;;
    fail)
      printf -v SOURCE_UI_ROW '  %-26s %s✗ 失败%s %s 已下载' "$tag" "$C_E" "$C_R" \
        "$(source_human_size "$DOWNLOADS/$ST_FILE")"
      ;;
    verify)
      printf -v SOURCE_UI_ROW '  %-26s %s校验中%s %s' "$tag" "$C_Y" "$C_R" \
        "$(source_human_size "$DOWNLOADS/$ST_FILE")"
      ;;
    download)
      cur="$(stat -c %s "$DOWNLOADS/$ST_FILE" 2>/dev/null || printf 0)"
      if [ "$ST_TOTAL" -gt 0 ]; then
        pct=$((cur * 100 / ST_TOTAL))
        [ "$pct" -gt 100 ] && pct=100
        bar="$(source_progress_bar "$pct" "$barw")"
        source_progress_speed "$idx" "$cur"
        speed="$SOURCE_UI_SPEED_TEXT"
        if [ "$compact" = 1 ]; then
          printf -v SOURCE_UI_ROW '  %-18s [%s%s%s] %3d%%' \
            "$tag" "$C_C" "$bar" "$C_R" "$pct"
        else
          printf -v SOURCE_UI_ROW '  %-26s [%s%s%s] %3d%% %8s / %-8s %10s' \
            "$tag" "$C_C" "$bar" "$C_R" "$pct" \
            "$(source_human_size_bytes "$cur")" "$(source_human_size_bytes "$ST_TOTAL")" "$speed"
        fi
      else
        spinner="${SPIN_CHARS:0:1}"
        printf -v SOURCE_UI_ROW '  %-26s %s%s%s %s' "$tag" "$C_C" "${spinner:-·}" "$C_R" \
          "$(source_human_size_bytes "$cur")"
      fi
      ;;
    *)
      printf -v SOURCE_UI_ROW '  %-26s %s等待%s' "$tag" "$C_D" "$C_R"
      ;;
  esac
}

source_human_size_bytes() {  # $1=字节数（不读文件）
  local n="$1"
  awk -v n="$n" 'BEGIN {
    if (n >= 1048576) printf "%.1f MB", n / 1048576;
    else if (n >= 1024) printf "%.0f KB", n / 1024;
    else printf "%d B", n;
  }'
}

source_progress_render_frame() {  # $1=正文
  printf '%s\n' "$1" | gum style \
    --width "${SOURCE_UI_FRAME_WIDTH:-78}" \
    --border rounded --border-foreground 240 --padding '0 1' --no-strip-ansi
}

source_progress_erase() {  # 擦掉上一帧
  local n="${SOURCE_UI_PREV_LINES:-0}" i
  [ "$n" -gt 0 ] || return 0
  printf '\033[%dA' "$n"
  for ((i = 0; i < n; i++)); do printf '\033[2K\n'; done
  printf '\033[%dA' "$n"
  SOURCE_UI_PREV_LINES=0
}

source_progress_draw() {  # $1=正文
  local frame lines n=0
  frame="$(source_progress_render_frame "$1")"
  lines="$(printf '%s\n' "$frame" | wc -l)"
  if [ "${SOURCE_UI_PREV_LINES:-0}" -gt 0 ]; then
    printf '\033[%dA' "$SOURCE_UI_PREV_LINES"
  fi
  while IFS= read -r line; do
    printf '\033[2K%s\n' "$line"
    n=$((n + 1))
  done <<< "$frame"
  SOURCE_UI_PREV_LINES="$n"
}

source_progress_error_reason() {  # $1=err 文件
  local f="$1" line
  line="$(awk 'NF { last = $0 } END { if (last != "") print last }' "$f" 2>/dev/null || true)"
  printf '%s\n' "$line"
}

source_progress_worker() {  # $1=idx $2=清单行
  local idx="$1" line="$2" state="$SOURCE_UI_STATE_DIR/$1"
  local l kind name ver file url sha label total rc=0 reason
  IFS=$'\t' read -r l kind name ver file url sha <<< "$line"
  label="$(source_kind_label "$kind")"
  source_progress_write_state "$state" download "$label" "$name" "$ver" "$file" "$url" 0
  total="$(source_remote_size "$url" || true)"
  [ -n "$total" ] || total=0
  source_progress_write_state "$state" download "$label" "$name" "$ver" "$file" "$url" "$total"
  source_curl_download "$url" "$DOWNLOADS/$file" >/dev/null 2>"$SOURCE_UI_STATE_DIR/$idx.err" || rc=$?
  if [ "$rc" -ne 0 ]; then
    reason="$(source_progress_error_reason "$SOURCE_UI_STATE_DIR/$idx.err")"
    source_progress_write_state "$state" fail "$label" "$name" "$ver" "$file" "$url" "$total"
    source_record_failure "$label" "$name" "$ver" "$url" "$reason"
    return 1
  fi

  source_progress_write_state "$state" verify "$label" "$name" "$ver" "$file" "$url" "$total"
  if ! source_verify_file "$DOWNLOADS/$file" "$sha" "$url" 2>>"$SOURCE_UI_STATE_DIR/$idx.err"; then
    total="$(source_remote_size "$url" || true)"
    [ -n "$total" ] || total=0
    source_progress_write_state "$state" download "$label" "$name" "$ver" "$file" "$url" "$total"
    rc=0
    source_curl_download "$url" "$DOWNLOADS/$file" >/dev/null 2>>"$SOURCE_UI_STATE_DIR/$idx.err" || rc=$?
    if [ "$rc" -ne 0 ] || ! source_verify_file "$DOWNLOADS/$file" "$sha" "$url" 2>>"$SOURCE_UI_STATE_DIR/$idx.err"; then
      reason="$(source_progress_error_reason "$SOURCE_UI_STATE_DIR/$idx.err")"
      source_progress_write_state "$state" fail "$label" "$name" "$ver" "$file" "$url" "$total"
      source_record_failure "$label" "$name" "$ver" "$url" "$reason"
      return 1
    fi
  fi

  source_progress_write_state "$state" done "$label" "$name" "$ver" "$file" "$url" "$total"
  return 0
}

source_progress_final_row() {  # $1=idx
  local idx="$1"
  source_progress_read_state "$SOURCE_UI_STATE_DIR/$idx"
  local tag="[${ST_LABEL}] ${ST_NAME} ${ST_VER}"
  case "$ST_STATE" in
    done|skip)
      if [ "$ST_STATE" = done ]; then
        ok "$tag 完成（$(source_human_size "$DOWNLOADS/$ST_FILE")）"
      else
        ok "$tag 已存在"
      fi
      ;;
    fail)
      warn "$tag 失败（文件保留，下次运行可继续）"
      ;;
    *)
      warn "$tag 未完成"
      ;;
  esac
}

prefetch_sources_progress() {
  local exts="$1" write_list="${2:-0}" jobs="${PHPRT_DOWNLOAD_JOBS:-10}"
  local host_jobs="${PHPRT_DOWNLOAD_HOST_JOBS:-${PHPRT_HOST_JOBS:-2}}"
  local total idx=0 running=0 rc=0
  local line l kind name ver file url sha label
  local plan_file failed_file
  local cols="${COLUMNS:-80}"
  local SPIN_CHARS="⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
  SOURCE_UI_FRAME_WIDTH=$((cols - 2))
  [ "$SOURCE_UI_FRAME_WIDTH" -lt 40 ] && SOURCE_UI_FRAME_WIDTH=40
  [ "$SOURCE_UI_FRAME_WIDTH" -gt 160 ] && SOURCE_UI_FRAME_WIDTH=160
  cols="$SOURCE_UI_FRAME_WIDTH"

  case "$jobs" in
    ''|*[!0-9]*|0) die "--download-jobs 需要正整数: $jobs" ;;
  esac
  case "$host_jobs" in
    ''|*[!0-9]*|0) die "--download-host-jobs 需要正整数: $host_jobs" ;;
  esac

  source_plan "$exts"
  [ -n "${DOWNLOADS:-}" ] || die "内部错误：没有设置 DOWNLOADS"
  total="$(source_plan_lines | wc -l)"
  [ "$total" -gt 0 ] || return 0

  SOURCE_UI_STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/phprt-ui.XXXXXX")"
  export SOURCE_UI_STATE_DIR
  plan_file="$(mktemp "${TMPDIR:-/tmp}/phprt-sources.XXXXXX")"
  failed_file="$(mktemp "${TMPDIR:-/tmp}/phprt-sources-failed.XXXXXX")"
  SOURCE_FAILED_FILE="$failed_file"
  export SOURCE_FAILED_FILE
  source_plan_lines > "$plan_file"

  mkdir -p "$DOWNLOADS"
  # 完整地址先打印一次，进度框里不再放 URL，避免框宽和行数跳动。
  printf '下载地址:\n'
  source_plan_print_urls
  printf '\n'
  printf '\033[?25l'
  trap 'printf "\033[?25h"' EXIT INT TERM

  if [ "$write_list" = 1 ]; then
    local list="$DOWNLOADS/download-urls-php${PHP_VERSION}-$(date +%Y%m%d).txt"
    source_plan_print_urls > "$list"
    printf '下载清单: %s\n' "$list"
  fi

  local -a all_idx=() queue=() plan_lines=()
  local -A line_of=() host_of=() started=() pid_of=() host_running=()
  local qpending=0

  mapfile -t plan_lines < "$plan_file"

  # 先把所有包都放进状态表：已存在/离线缺失直接定状态，其余进队列。
  for line in "${plan_lines[@]}"; do
    IFS=$'\t' read -r l kind name ver file url sha <<< "$line"
    idx=$((idx + 1))
    all_idx+=("$idx")
    label="$(source_kind_label "$kind")"
    if source_cached_ok "$DOWNLOADS/$file" "$sha" "$url"; then
      source_progress_write_state "$SOURCE_UI_STATE_DIR/$idx" skip "$label" "$name" "$ver" "$file" "$url" 0
      continue
    fi
    if [ "${OFFLINE:-0}" = 1 ]; then
      source_progress_write_state "$SOURCE_UI_STATE_DIR/$idx" fail "$label" "$name" "$ver" "$file" "$url" 0
      source_record_failure "$label" "$name" "$ver" "$url"
      continue
    fi
    source_progress_write_state "$SOURCE_UI_STATE_DIR/$idx" wait "$label" "$name" "$ver" "$file" "$url" 0
    queue+=("$idx")
    host_of[$idx]="$(source_host_bucket "$url")"
    started[$idx]=0
    qpending=$((qpending + 1))
    line_of[$idx]="$l"$'\t'"$kind"$'\t'"$name"$'\t'"$ver"$'\t'"$file"$'\t'"$url"$'\t'"$sha"
  done

  source_progress_start_more() {
    local qi h
    for qi in "${queue[@]}"; do
      [ "$running" -lt "$jobs" ] || break
      [ "${started[$qi]}" = 1 ] && continue
      h="${host_of[$qi]}"
      [ "${host_running[$h]:-0}" -ge "$host_jobs" ] && continue
      started[$qi]=1
      source_progress_worker "$qi" "${line_of[$qi]}" &
      pid_of[$qi]=$!
      host_running[$h]=$(( ${host_running[$h]:-0} + 1 ))
      running=$((running + 1))
      qpending=$((qpending - 1))
    done
  }

  # 一屏放不下时，优先保留正在下载/失败的行，再补等待/完成的行。
  source_progress_visible_ids() {
    local limit=$(( ${LINES:-24} - 6 ))
    [ "$limit" -lt 4 ] && limit=4
    if [ "${#all_idx[@]}" -le "$limit" ]; then
      printf '%s\n' "${all_idx[@]}"
      return 0
    fi
    local i n=0
    local -a priority=() others=()
    for i in "${all_idx[@]}"; do
      source_progress_read_state "$SOURCE_UI_STATE_DIR/$i"
      case "$ST_STATE" in
        download|verify|fail) priority+=("$i") ;;
        *) others+=("$i") ;;
      esac
    done
    for i in "${priority[@]}"; do
      [ "$n" -ge "$limit" ] && break
      printf '%s\n' "$i"
      n=$((n + 1))
    done
    for i in "${others[@]}"; do
      [ "$n" -ge "$limit" ] && break
      printf '%s\n' "$i"
      n=$((n + 1))
    done
  }

  while [ "$qpending" -gt 0 ] || [ "$running" -gt 0 ]; do
    source_progress_start_more

    local body="" shown_ids=() i url_line
    mapfile -t shown_ids < <(source_progress_visible_ids)
    body="源码目录: $(source_progress_fit "$DOWNLOADS" $((cols - 8)))"$'\n'
    body+="$(source_progress_fit "共 ${#all_idx[@]} 个包，并发 $jobs（每站点 $host_jobs）；已存在会自动跳过" $((cols - 2)))"$'\n\n'
    for i in "${shown_ids[@]}"; do
      source_progress_row "$i" "$cols"
      body+="$SOURCE_UI_ROW"$'\n'
    done
    if [ "${#shown_ids[@]}" -lt "${#all_idx[@]}" ]; then
      body+="    … 还有 $(( ${#all_idx[@]} - ${#shown_ids[@]} )) 个包"$'\n'
    fi
    source_progress_draw "$body"

    # 回收已结束的 worker
    local -a keys=("${!pid_of[@]}")
    for i in "${keys[@]}"; do
      if ! kill -0 "${pid_of[$i]}" 2>/dev/null; then
        wait "${pid_of[$i]}" 2>/dev/null || true
        local h="${host_of[$i]}"
        host_running[$h]=$(( ${host_running[$h]:-1} - 1 ))
        unset 'pid_of[$i]'
        running=$((running - 1))
      fi
    done
    sleep 0.2
  done

  # 最后再画一帧，把 done/fail 状态留在屏幕上。
  local body="" shown_ids=() i
  mapfile -t shown_ids < <(source_progress_visible_ids)
  body="源码目录: $(source_progress_fit "$DOWNLOADS" $((cols - 8)))"$'\n'
  body+="共 ${#all_idx[@]} 个包，并发 $jobs（每站点 $host_jobs）；已存在会自动跳过"$'\n\n'
  for i in "${shown_ids[@]}"; do
    source_progress_row "$i" "$cols"
    body+="$SOURCE_UI_ROW"$'\n'
  done
  if [ "${#shown_ids[@]}" -lt "${#all_idx[@]}" ]; then
    body+="    … 还有 $(( ${#all_idx[@]} - ${#shown_ids[@]} )) 个包"$'\n'
  fi
  source_progress_draw "$body"

  if [ -s "$failed_file" ]; then
    rc=1
    printf '\n' >&2
    warn "有源码包下载失败："
    while IFS=$'\t' read -r label name ver url reason; do
      printf '  [%s] %s %s\n    %s\n' "$label" "$name" "$ver" "$url" >&2
      [ -n "$reason" ] && printf '    原因: %s\n' "$reason" >&2
      case "$url" in
        https://github.com/*)
          local m
          m="$(source_github_mirrors | head -1 || true)"
          [ -n "$m" ] && printf '    镜像: %s%s\n' "${m%/}/" "$url" >&2
          ;;
      esac
    done < "$failed_file"
    printf '    可复制上面的链接手动下载，放到 %s 后重试。\n' "$DOWNLOADS" >&2
  fi

  rm -rf "$SOURCE_UI_STATE_DIR"
  rm -f "$plan_file" "$failed_file"
  unset SOURCE_FAILED_FILE SOURCE_UI_STATE_DIR
  return "$rc"
}
