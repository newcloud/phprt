#!/usr/bin/env bash
# phprt：通用函数（被入口、命令实现和 bundle recipe 引用）
set -euo pipefail

# recipe 用 "${PHPRT_RUNNER}/common.sh" 引用驱动；直接执行 recipe 或 driver
# 没导出这个变量时，用 common.sh 自己所在的 runner 目录兜底。
PHPRT_RUNNER="${PHPRT_RUNNER:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
export PHPRT_RUNNER

if [ -t 1 ]; then
  C_R=$'\033[0m'; C_C=$'\033[1;36m'; C_G=$'\033[1;32m'; C_Y=$'\033[1;33m'; C_E=$'\033[1;31m'; C_D=$'\033[2m'
else
  C_R=""; C_C=""; C_G=""; C_Y=""; C_E=""; C_D=""
fi

log()  { printf '%s==>%s %s\n' "$C_C" "$C_R" "$*"; }
ok()   { printf '%s  ok%s %s\n' "$C_G" "$C_R" "$*"; }
warn() { printf '%s  !%s %s\n' "$C_Y" "$C_R" "$*" >&2; }
die()  { printf '%s错误:%s %s\n' "$C_E" "$C_R" "$*" >&2; exit 1; }

find_project() {
  local d="$PWD"
  while :; do
    # 只认真正的项目根：~/.phprt 是全局缓存目录，不能当成项目。
    # current 软链是项目标记；旧版布局可能只有 project.conf，保留兼容。
    if [ -L "$d/.phprt/current" ] || [ -f "$d/.phprt/project.conf" ]; then
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
  local -a dirs=()
  [ -n "${PHPRT_SEED:-}" ] && dirs+=("$PHPRT_SEED")
  # 版本系列目录：downloads/php-7.4/ 放这一系列专用的旧版依赖
  [ -n "${PHP_VERSION:-}" ] && series="php-${PHP_VERSION%.*}"
  for base in "${PHPRT_REPO:-}"; do
    [ -n "$base" ] || continue
    [ -n "$series" ] && dirs+=("$base/downloads/$series")
    dirs+=("$base/downloads")
  done
  dirs+=("$DOWNLOADS")
  # 旧布局：源码包放在 build-php-<版本>/downloads/。只读兼容，升级后不用重下。
  for d in "$PHPRT_HOME"/build-php-*/downloads; do
    [ -d "$d" ] && dirs+=("$d")
  done
  dirs+=("${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/phprt/downloads")
  dirs+=("$HOME/.phprt/downloads")     # 旧版全局缓存
  dirs+=("$PHPRT_HOME/downloads")      # 更早的 v1 项目布局（只读兼容）
  for d in "${dirs[@]}"; do
    [ -f "$d/$name" ] && { printf '%s\n' "$d/$name"; return 0; }
  done
  return 1
}

# 下载并校验单个源码包。下载/续传全部交给 curl -C -，这里只做校验和失败报告。
ensure_source() {
  local name="$1" url="$2" sha="$3"
  local dst="$DOWNLOADS/$name" src
  mkdir -p "$DOWNLOADS"

  if [ ! -s "$dst" ]; then
    src="$(find_source "$name" || true)"
    if [ -n "$src" ]; then
      copy_hard "$src" "$dst"
      ok "源码缓存: $name"
    fi
  fi

  if [ ! -s "$dst" ]; then
    if [ "${PHPRT_NO_DOWNLOAD:-0}" = 1 ]; then
      die "源码缺失: $name（构建阶段不再联网；先运行 phprt download）"
    fi
    log "下载 $name -> $dst"
    source_curl_download "$url" "$dst" || die "下载失败: $url"
  fi

  if ! source_verify_file "$dst" "$sha" "$url"; then
    if [ "${PHPRT_NO_DOWNLOAD:-0}" = 1 ]; then
      die "源码校验失败: $name（先运行 phprt download 重新下载）"
    fi
    log "重新下载 $name"
    source_curl_download "$url" "$dst" || die "下载失败: $url"
    source_verify_file "$dst" "$sha" "$url" || die "源码校验失败: $name"
  fi
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

# ---------------------------------------------------------------------------
# recipe 注册表（contract=v2）
#
# “某个扩展/依赖叫什么、版本从哪来、产出哪些模块、依赖谁”全部从 recipe
# 头部的一行 `# phprt:` 读取，不再维护 dep_dir_of / ext_modules / ext_version
# 之类的硬编码表。字段与校验规则见 docs/RECIPE.md。
# ---------------------------------------------------------------------------

recipe_header() {
  awk 'NR <= 12 && /^# *phprt:/ { sub(/^# *phprt: */, ""); print; exit }' "$1"
}

recipe_field() {  # $1=文件 $2=键
  recipe_header "$1" | tr ' ' '\n' | sed -n "s/^$2=//p" | head -1
}

recipe_kind_of() {
  case "$1" in
    */builtin/*) printf 'builtin\n' ;;
    */deps/*)    printf 'dep\n' ;;
    */ext/*)     printf 'ext\n' ;;
    */tools/*)   printf 'tool\n' ;;
    *)           printf 'unknown\n' ;;
  esac
}

recipe_name_of() { basename "$1" .sh; }

recipe_has_func() {
  grep -qE "^[[:space:]]*$2[[:space:]]*\\(\\)[[:space:]]*\\{" "$1"
}

recipe_version_var() {  # $1=文件 -> 头部 version= 的变量名
  recipe_field "$1" version
}

recipe_version() {  # $1=文件 -> 头部 version= 指向的变量的值
  local var v
  var="$(recipe_version_var "$1")"
  [ -n "$var" ] || return 0
  v="${!var:-}"
  [ -n "$v" ] && printf '%s\n' "$v"
  return 0
}

recipe_switch()     { recipe_field "$1" switch; }
recipe_ini()        { recipe_field "$1" ini; }
recipe_upstream()   { recipe_field "$1" upstream; }
recipe_category()   { recipe_field "$1" category; }
recipe_menu_order() { recipe_field "$1" menu_order; }
recipe_wheel()      { recipe_field "$1" wheel; }
recipe_desc()       { recipe_field "$1" desc; }
recipe_notes() {  # $1=文件 -> 一行一个 note slug
  local v
  v="$(recipe_field "$1" note)"
  [ -n "$v" ] || return 0
  printf '%s\n' "${v//+/ }"
}

recipe_needs_of() {  # $1=文件 -> 空格分隔的头部 needs
  recipe_field "$1" needs | tr ',' ' '
}

recipe_tools_of() {  # $1=文件 -> 空格分隔的头部 tools
  recipe_field "$1" tools | tr ',' ' '
}

recipe_modules_list() {  # $1=文件 -> 空格分隔的 modules；ext 省略时用 name
  local m
  m="$(recipe_field "$1" modules)"
  [ -n "$m" ] || m="$(recipe_name_of "$1")"
  printf '%s\n' "${m//,/ }"
}

recipe_hooks() {  # $1=文件 -> 一行一个已实现的可选钩子
  local f="$1" h
  for h in flags configure_args needs opts build enable verify stage_static \
           versions latest_version download_url checksum; do
    recipe_has_func "$f" "$h" && printf '%s\n' "$h"
  done
}

# 临时把 switch 置成指定值再查询，查询完恢复。用于 --flags / --needs 的 0/1 对比。
recipe_query_with_switch() {  # $1=recipe $2=0|1 $3=动词
  local sh="$1" val="$2" verb="$3" sw old out rc=0
  sw="$(recipe_switch "$sh")"
  if [ -z "$sw" ]; then
    out="$(bash "$sh" "$verb")" || rc=$?
    printf '%s\n' "$out"
    return "$rc"
  fi
  old="${!sw:-0}"
  printf -v "$sw" '%s' "$val"
  export "${sw?}"
  out="$(bash "$sh" "$verb")" || rc=$?
  printf -v "$sw" '%s' "$old"
  export "${sw?}"
  printf '%s\n' "$out"
  return "$rc"
}

_recipe_needs_only_sh() {  # $1=recipe -> 硬依赖，不含 tools
  local sh="$1"
  if recipe_has_func "$sh" needs; then
    bash "$sh" --needs 2>/dev/null || true
  else
    recipe_needs_of "$sh"
  fi
}

# 从给定依赖名出发，递归收集 dep/tool 的版本、needs、tools、notes。
# 输出：name|kind|version|needs|tools|notes（用 | 避免空字段被 read 折叠）
recipe_needs_closure() {
  local -a q=("$@")
  local -A seen=()
  local n sh kind ver needs tools notes x
  while [ ${#q[@]} -gt 0 ]; do
    n="${q[0]}"
    q=("${q[@]:1}")
    [ -n "$n" ] || continue
    [ -n "${seen[$n]+x}" ] && continue
    seen[$n]=1
    sh="$(module_sh "$n" 2>/dev/null)" || continue
    kind="$(recipe_kind_of "$sh")"
    ver="$(recipe_version "$sh")"
    needs="$(_recipe_needs_only_sh "$sh" | awk 'NF && !seen[$0]++' | tr '\n' ' ' || true)"
    needs="${needs% }"
    tools="$(recipe_tools_of "$sh")"
    notes="$(recipe_notes "$sh" | tr '\n' ' ' || true)"
    notes="${notes% }"
    printf '%s|%s|%s|%s|%s|%s\n' \
      "$n" "$kind" "$ver" "$needs" "$tools" "$notes"
    for x in $needs $tools; do
      [ -n "$x" ] && q+=("$x")
    done
  done
}

# JSON 输出辅助。值都是单行；换行转成 \n 兜底。
json_escape() {
  printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\t/\\t/g' -e 's/\r//g'
}

json_words() {  # 空格分隔 -> JSON 字符串数组
  local first=1 w out=""
  for w in $*; do
    [ -n "$w" ] || continue
    [ "$first" = 1 ] || out+=","
    first=0
    out+="\"$(json_escape "$w")\""
  done
  printf '[%s]' "$out"
}

json_lines() {  # 一行一个 -> JSON 字符串数组
  local first=1 line out=""
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    [ "$first" = 1 ] || out+=","
    first=0
    out+="\"$(json_escape "$line")\""
  done
  printf '[%s]' "$out"
}

json_notes() {  # $1=recipe -> [{slug,text}]；文本来自 notes.conf，缺省用 slug
  local f="$1" slug text first=1 out="" nf="${BUNDLE:-}/notes.conf"
  while IFS= read -r slug; do
    [ -n "$slug" ] || continue
    text=""
    [ -f "$nf" ] && text="$(sed -n "s/^$slug=//p" "$nf" | head -1)"
    [ -n "$text" ] || text="$slug"
    [ "$first" = 1 ] || out+=","
    first=0
    out+="{\"slug\":\"$(json_escape "$slug")\",\"text\":\"$(json_escape "$text")\"}"
  done < <(recipe_notes "$f")
  printf '[%s]' "$out"
}

# 模块（扩展/依赖/工具）的 recipe 路径。搜索顺序 deps -> ext -> tools -> builtin，
# 避免 curl 这类同名模块命中 builtin。
module_sh() {
  local n="$1" d
  for d in deps ext tools builtin; do
    [ -f "$BUNDLE/$d/$n.sh" ] && { printf '%s\n' "$BUNDLE/$d/$n.sh"; return 0; }
  done
  die "没有模块 $n（bundles/<版本>/{deps,ext,tools,builtin}/$n.sh 都不存在）"
}

# 扩展名（builtin / PECL）专用解析：同名 dep 不能抢。
# 例：libxml2 / zlib 的 builtin 与 dep 同名，依赖解析仍走 module_sh
# 的 deps 优先顺序，扩展解析走这里。
ext_recipe_sh() {
  local n="$1"
  [ -f "$BUNDLE/builtin/$n.sh" ] && { printf '%s\n' "$BUNDLE/builtin/$n.sh"; return 0; }
  [ -f "$BUNDLE/ext/$n.sh" ] && { printf '%s\n' "$BUNDLE/ext/$n.sh"; return 0; }
  return 1
}

# 扩展改过名字时给一句提示（只用于“未知扩展”报错）。
# 映射放在 bundle 数据里，driver 不认识具体包名。
ext_name_hint() {
  local f="${BUNDLE:-}/aliases.conf" old new
  [ -n "${BUNDLE:-}" ] && [ -f "$f" ] || return 0
  while IFS='=' read -r old new; do
    case "$old" in ''|\#*) continue ;; esac
    if [ "$old" = "$1" ] && [ -n "$new" ]; then
      printf '；%s 已改名为 %s' "$old" "$new"
      return 0
    fi
  done < "$f"
  return 0
}

# 安装位置：dep -> deps/<名字>-<版本>；tool -> tools/<名字>-<版本>；
# builtin/ext -> PHP 树本身（.so 由 phpize 装进扩展目录）
recipe_dir_of() {
  local name="$1" sh kind var ver
  load_config
  sh="$(module_sh "$name")"
  kind="$(recipe_kind_of "$sh")"
  var="$(recipe_version_var "$sh")"
  ver="$(recipe_version "$sh")"
  case "$kind" in
    dep)  [ -n "$ver" ] || die "dep/$name 的版本变量 ${var:-<空>} 没有值"
          printf '%s/%s-%s\n' "$DEPS" "$name" "$ver" ;;
    tool) [ -n "$ver" ] || die "tool/$name 的版本变量 ${var:-<空>} 没有值"
          printf '%s/tools/%s-%s\n' "$BUILD_ROOT" "$name" "$ver" ;;
    ext|builtin)
          printf '%s\n' "$TREE" ;;
    *)    die "无法确定 $name 的安装位置（kind=$kind）" ;;
  esac
}

builtin_names() {
  local f
  for f in "$BUNDLE"/builtin/*.sh; do
    [ -f "$f" ] || continue
    recipe_name_of "$f"
  done | sort
}

ext_names() {
  local f
  for f in "$BUNDLE"/ext/*.sh; do
    [ -f "$f" ] || continue
    recipe_name_of "$f"
  done | sort
}

all_ext_names() { { builtin_names; ext_names; } | sort -u; }

# 按 SELECTED_EXTS 设置所有 builtin/ext 的开关变量（ENABLE_*）。
# 这些变量被 builtin 的 --flags/--needs 和条件依赖读取。
builtin_apply_switches() {
  local f e sw v
  for f in "$BUNDLE"/builtin/*.sh "$BUNDLE"/ext/*.sh; do
    [ -f "$f" ] || continue
    e="$(recipe_name_of "$f")"
    sw="$(recipe_switch "$f")"
    [ -n "$sw" ] || continue
    case " ${SELECTED_EXTS//,/ } " in
      *" $e "*) v=1 ;;
      *)        v=0 ;;
    esac
    # shared 的 builtin 不在 PHP configure 里编，开关置 0 让 flags() 输出
    # 负向参数（--without-X）；独立 .so 走后面的 phpize 阶段。
    if [ "$v" = 1 ] && [ "$(recipe_kind_of "$f")" = builtin ] &&
       [ "$(mode_of "$e" 2>/dev/null || true)" = shared ]; then
      v=0
    fi
    printf -v "$sw" '%s' "$v"
    export "${sw?}"
  done
}

# ---------------------------------------------------------------------------
# 扩展子选项：recipe 可选定义 opts()
#
# 输出（每行三列，TAB 分隔）：
#   key<TAB>state<TAB>label
#
# state：
#   on    默认启用
#   off   默认关闭
#   fixed 始终启用；界面展示但不可关闭
#
# key 映射到环境变量 ${switch}_${KEY}，例如 gd 的 jpeg -> ENABLE_GD_JPEG。
# 驱动只认这三列，不认具体扩展名；将来要加分组/说明，在后面追加列即可。
# ---------------------------------------------------------------------------
ext_opt_var() {  # $1=switch $2=key -> 环境变量名
  local sw="$1" key="$2"
  printf '%s_%s\n' "$sw" "$(printf '%s' "$key" | tr '[:lower:]-' '[:upper:]_')"
}

declare -gA PR_OPTS_META_CACHE=()

recipe_opts_meta() {  # $1=recipe 文件 -> key<TAB>state<TAB>label
  local sh="$1" out
  if [ -n "${PR_OPTS_META_CACHE[$sh]+x}" ]; then
    printf '%s\n' "${PR_OPTS_META_CACHE[$sh]}"
    return 0
  fi
  recipe_has_func "$sh" opts || return 0
  out="$(BUNDLE="${BUNDLE:-$(cd "$(dirname "$sh")/.." && pwd)}" bash "$sh" --opts)" ||
    { warn "读取子选项失败: $sh"; return 1; }
  printf '%s\n' "$out" | awk -F'\t' '
    NF == 0 { next }
    NF < 3 || $1 !~ /^[a-z][a-z0-9_]*$/ || $2 !~ /^(on|off|fixed)$/ || seen[$1]++ {
      exit 1
    }
  ' || { warn "opts() 输出格式错误（需要 key<TAB>state<TAB>label）: $sh"; return 1; }
  PR_OPTS_META_CACHE[$sh]="$out"
  printf '%s\n' "$out"
}

ext_opts_get() {  # $1="gd=..." $2=扩展名 -> 子选项列表；没写返回 2
  local pair
  for pair in $1; do
    if [ "${pair%%=*}" = "$2" ]; then
      printf '%s\n' "${pair#*=}"
      return 0
    fi
  done
  return 2
}

# ---------------------------------------------------------------------------
# 扩展分类：选择器按这个顺序分组，recipe 头部用 category=<名字> 声明。
# 值不能带空格（头部字段用空格分隔）；显示名可以带空格。
# ---------------------------------------------------------------------------
PHPRT_EXT_CATEGORIES=(Database CacheStorage Compression Cryptography Image Internationalization Network Runtime Other)
declare -gA PHPRT_CATEGORY_LABELS=(
  [Database]=Database
  [CacheStorage]='Cache & Storage'
  [Compression]=Compression
  [Cryptography]=Cryptography
  [Image]='Image Processing'
  [Internationalization]=Internationalization
  [Network]=Network
  [Runtime]=Runtime
  [Other]=Other
)

ext_category_rank() {  # $1=分类名 -> 1..N；未知按 Other
  local c i=0
  for c in "${PHPRT_EXT_CATEGORIES[@]}"; do
    i=$((i + 1))
    [ "$c" = "$1" ] && { printf '%s' "$i"; return 0; }
  done
  printf '%s' "${#PHPRT_EXT_CATEGORIES[@]}"
}

ext_category_label() {  # $1=分类名 -> 界面显示名
  local l="${PHPRT_CATEGORY_LABELS[$1]:-}"
  printf '%s' "${l:-${1:-Other}}"
}

ext_category_of() {  # $1=recipe 文件 -> 分类名（缺省 Other）
  local c
  c="$(recipe_category "$1")"
  case " ${PHPRT_EXT_CATEGORIES[*]} " in
    *" $c "*) printf '%s' "$c" ;;
    *)        printf 'Other' ;;
  esac
}

ext_menu_order() {  # $1=recipe 文件 -> 分类内的排序号（缺省 1000）
  local o
  o="$(recipe_menu_order "$1")"
  case "$o" in ''|*[!0-9]*) printf '1000' ;; *) printf '%s' "$o" ;; esac
}

# 合并两份子选项 spec：同名扩展用新的，其它保留。
# `add ext --opt` 只改点名的扩展，不该把树里其它扩展的配置重置成默认。
ext_opts_merge() {  # $1=已有 spec $2=新增 spec
  local base="${1:-}" add="${2:-}" pair e out=""
  for pair in $base; do
    e="${pair%%=*}"
    case " $add " in
      *" $e="*) continue ;;
    esac
    out="${out}${out:+ }$pair"
  done
  for pair in $add; do
    out="${out}${out:+ }$pair"
  done
  printf '%s' "$out"
}

apply_ext_opts() {
  local e sh sw key state label var v pair meta list
  local -A want=()
  for pair in ${PHPRT_EXT_OPTS:-}; do
    case "$pair" in
      *=*) want["${pair%%=*}"]="${pair#*=}" ;;
      *)   warn "PHPRT_EXT_OPTS 格式应为 扩展=opt1,opt2: $pair" ;;
    esac
  done
  for e in ${SELECTED_EXTS:-}; do
    sh="$(ext_recipe_sh "$e")" || continue
    recipe_has_func "$sh" opts || continue
    sw="$(recipe_switch "$sh")"
    [ -n "$sw" ] || continue
    meta="$(recipe_opts_meta "$sh")" || continue
    if [ -n "${want[$e]+x}" ]; then
      list="${want[$e]}"
    else
      list="__default__"
    fi
    while IFS=$'\t' read -r key state label; do
      [ -n "$key" ] || continue
      var="$(ext_opt_var "$sw" "$key")"
      if [ "$list" = "__default__" ]; then
        case "$state" in on|fixed) v=1 ;; *) v=0 ;; esac
      else
        case ",$list," in *",$key,"*) v=1 ;; *) v=0 ;; esac
        [ "$state" = fixed ] && v=1
      fi
      printf -v "$var" '%s' "$v"
      export "$var"
    done <<< "$meta"
  done
}

ext_opts_effective() {  # $1=扩展列表 -> "gd=jpeg,png,..."（只列有 opts 的扩展）
  local e sh sw key state label var on list out="" meta
  for e in $1; do
    sh="$(ext_recipe_sh "$e")" || continue
    recipe_has_func "$sh" opts || continue
    sw="$(recipe_switch "$sh")"
    meta="$(recipe_opts_meta "$sh")" || continue
    list=""
    while IFS=$'\t' read -r key state label; do
      [ -n "$key" ] || continue
      if [ "$state" = fixed ]; then
        on=1
      else
        var="$(ext_opt_var "$sw" "$key")"
        on="${!var:-0}"
      fi
      [ "$on" = 1 ] && list="${list}${list:+,}$key"
    done <<< "$meta"
    out="${out}${out:+ }$e=$list"
  done
  printf '%s\n' "$out"
}

validate_ext_opts() {  # $1=扩展列表 $2="gd=jpeg,webp ..."
  local exts="$1" spec="${2:-}" pair e sh list key k meta
  local -a keys=()
  for pair in $spec; do
    case "$pair" in
      *=*) e="${pair%%=*}"; list="${pair#*=}" ;;
      *)   die "--opt 格式应为 扩展=opt1,opt2: $pair" ;;
    esac
    case " $exts " in
      *" $e "*) ;;
      *) die "--opt 的扩展 $e 不在本次扩展列表里" ;;
    esac
    sh="$(ext_recipe_sh "$e")" || die "未知扩展: $e"
    recipe_has_func "$sh" opts || die "$e 没有可配置的子选项"
    meta="$(recipe_opts_meta "$sh")" || die "读取 $e 的子选项失败"
    keys=()
    while IFS=$'\t' read -r key state label; do
      [ -n "$key" ] && keys+=("$key")
    done <<< "$meta"
    for k in ${list//,/ }; do
      [ -n "$k" ] || continue
      case " ${keys[*]} " in
        *" $k "*) ;;
        *) die "$e 没有子选项 $k（可用: ${keys[*]}）" ;;
      esac
    done
  done
}

load_tree_ext_opts() {  # $1=tree；命令行已经给了就不覆盖
  [ -n "${PHPRT_EXT_OPTS:-}" ] && return 0
  local f="$1/.build-opts"
  [ -s "$f" ] || return 0
  PHPRT_EXT_OPTS="$(tr '\n' ' ' < "$f")"
  PHPRT_EXT_OPTS="${PHPRT_EXT_OPTS% }"
  export PHPRT_EXT_OPTS
}

# ---------------------------------------------------------------------------
# 编译方式：static（编入 PHP）/ shared（独立 .so）
# 契约字段：type=static|shared|static,shared，default=static|shared
# ---------------------------------------------------------------------------
recipe_types() {  # $1=recipe 文件 -> 空格分隔的模式列表
  recipe_field "$1" type | tr ',' ' '
}

recipe_default_mode() {  # $1=recipe 文件
  recipe_field "$1" default
}

mode_supported() {  # $1=recipe 文件 $2=mode
  case " $(recipe_types "$1") " in
    *" $2 "*) return 0 ;;
  esac
  return 1
}

# ---------------------------------------------------------------------------
# 链接方式：shared（依赖 .so）/ static（依赖 .a，编入 PHP）
# 记录在 $TREE/.phprt-state/link；PHPRT_LINK_MODE 是本次运行的覆盖值。
# ---------------------------------------------------------------------------
link_mode_file_of() {  # $1=tree
  printf '%s/.phprt-state/link\n' "$1"
}

link_mode_get() {  # $1=tree，可省略用 $TREE
  local t="${1:-${TREE:-}}" f
  [ -n "$t" ] || return 1
  f="$(link_mode_file_of "$t")"
  [ -f "$f" ] && cat "$f"
}

link_mode_set() {  # $1=mode $2=tree（可省略用 $TREE）
  local m="$1" t="${2:-${TREE:-}}" f
  [ -n "$t" ] || return 1
  f="$(link_mode_file_of "$t")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$m" > "$f"
}

# 本次覆盖 -> 树上记录 -> shared
link_mode() {
  local m
  m="${PHPRT_LINK_MODE:-}"
  [ -n "$m" ] && { printf '%s\n' "$m"; return 0; }
  m="$(link_mode_get "${TREE:-}" 2>/dev/null || true)"
  printf '%s\n' "${m:-shared}"
}

link_is_static() { [ "$(link_mode)" = static ]; }

# ---------------------------------------------------------------------------
# LTO 方式：1 = PHP 和依赖都开 LTO；0 = 普通构建。
# 记录在 $TREE/.phprt-state/lto；PHPRT_LTO 是本次运行的覆盖值。
# ---------------------------------------------------------------------------
lto_mode_file_of() {  # $1=tree
  printf '%s/.phprt-state/lto\n' "$1"
}

lto_mode_get() {  # $1=tree，可省略用 $TREE
  local t="${1:-${TREE:-}}" f
  [ -n "$t" ] || return 1
  f="$(lto_mode_file_of "$t")"
  [ -f "$f" ] && cat "$f"
}

lto_mode_set() {  # $1=1/0 $2=tree（可省略用 $TREE）
  local m="$1" t="${2:-${TREE:-}}" f
  [ -n "$t" ] || return 1
  f="$(lto_mode_file_of "$t")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$m" > "$f"
}

# 本次覆盖 -> 树上记录 -> 0
lto_mode() {
  local m="${PHPRT_LTO:-}"
  [ -n "$m" ] && { printf '%s\n' "$m"; return 0; }
  m="$(lto_mode_get "${TREE:-}" 2>/dev/null || true)"
  printf '%s\n' "${m:-0}"
}

mode_file_of() {  # $1=tree $2=扩展名
  printf '%s/.phprt-state/mode/%s\n' "$1" "$2"
}

mode_get() {  # $1=扩展名；用当前 $TREE
  local f
  [ -n "${TREE:-}" ] || return 1
  f="$(mode_file_of "$TREE" "$1")"
  [ -f "$f" ] && cat "$f"
}

mode_set() {  # $1=扩展名 $2=mode；写当前 $TREE
  local f
  [ -n "${TREE:-}" ] || return 1
  f="$(mode_file_of "$TREE" "$1")"
  mkdir -p "$(dirname "$f")"
  printf '%s\n' "$2" > "$f"
}

# 本次运行的覆盖值（来自 --build / --static / --shared），格式 "a=shared b=static"
mode_override() {
  printf '%s\n' "${PHPRT_EXT_MODES:-}" | tr ' ' '\n' |
    sed -n "s/^$1=//p" | head -1
}

# 解析顺序：本次覆盖 -> 树上已记录 -> 树上实际状态 -> 契约 default。
# 实际状态要排在 default 前面：老树没有 mode 记录时，不能因为契约 default
# 变了就把已装扩展悄悄切模式（会多一次 PHP 重编）。
mode_of() {
  local n="$1" m sh
  m="$(mode_override "$n")"
  [ -n "$m" ] && { printf '%s\n' "$m"; return 0; }
  m="$(mode_get "$n" 2>/dev/null || true)"
  [ -n "$m" ] && { printf '%s\n' "$m"; return 0; }
  m="$(ext_current_mode "$TREE" "$n" 2>/dev/null || true)"
  if [ -n "$m" ]; then
    sh="$(ext_recipe_sh "$n")" || return 1
    # 老树可能还是契约不允许的方式（zlib 以前默认 shared）；
    # 这种情况按契约 default 迁移，不能把不支持的方式带进 build.sh。
    mode_supported "$sh" "$m" && { printf '%s\n' "$m"; return 0; }
  fi
  sh="$(ext_recipe_sh "$n")" || return 1
  recipe_default_mode "$sh"
}

# 已安装树里的实际模式：优先看记录，其次看 .so / php -m。
# 找不到该扩展时返回非 0。
ext_current_mode() {
  local tree="$1" name="$2" f m extdir ld
  f="$(mode_file_of "$tree" "$name")"
  if [ -f "$f" ]; then cat "$f"; return 0; fi
  extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
  if [ -n "$extdir" ]; then
    for m in $(ext_modules "$name"); do
      [ -f "$extdir/$m.so" ] && { printf 'shared\n'; return 0; }
    done
  fi
  ld="$tree/lib:$(ls -d "$tree"/deps/*/lib 2>/dev/null | tr '\n' ':')"
  for m in $(ext_modules "$name"); do
    # -n：不读 php.ini 和 conf.d。此时还能列出模块，说明它编进了 bin/php，
    # 而不是通过 extension= 或 .so 动态加载。
    # 不用 grep -q：pipefail 下 php 会因 SIGPIPE 退出，导致误判为未加载。
    if LD_LIBRARY_PATH="$ld" "$tree/bin/php" -n -m 2>/dev/null | grep -cix "$m" >/dev/null; then
      printf 'static\n'; return 0
    fi
  done
  return 1
}

# 临时把 switch 置 1 取 flags：shared builtin 的 phpize 参数和 PHP
# configure 参数是同一套，但 PHP 侧开关已经被置 0。
flags_with_switch() {
  local n="$1" sh
  sh="$(ext_recipe_sh "$n")" || return 1
  recipe_query_with_switch "$sh" 1 --flags
}

# 临时把 switch 置 1 取 needs：shared builtin 在 PHP 里是关的，
# 但独立编 .so 仍然需要它的依赖。
needs_with_switch() {
  local n="$1" sh
  sh="$(ext_recipe_sh "$n")" || return 1
  recipe_query_with_switch "$sh" 1 --needs
}

ext_modules() {  # $1=扩展名 -> 产出的 PHP 模块名（空格分隔）
  local sh
  sh="$(ext_recipe_sh "$1")" || sh="$(module_sh "$1")"
  recipe_modules_list "$sh"
}

# ---------------------------------------------------------------------------
# L1 静态校验：头字段、函数、最后一行 recipe_main、顶层构建代码
# ---------------------------------------------------------------------------
recipe_toplevel_offenses() {
  awk '
    /^[[:space:]]*(source|\.)[[:space:]].*common\.sh/ { started = 1; next }
    !started { next }
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/ { next }
    /^[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(\).*\{/ { exit }
    /^[[:space:]]*(load_config|set|export|local|declare|readonly|shopt)[[:space:]]/ { next }
    /^[[:space:]]*load_config[[:space:]]*$/ { next }
    /^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*=/ { next }
    { print NR ": " $0 }
  ' "$1"
}

# ---------------------------------------------------------------------------
# L1 静态校验与头部解析在 contract.sh：字段表、必填/禁止、值域、重复/未知键。
# 这里保留 recipe_toplevel_offenses（contract.sh 会调用），下面 source 契约层。
# ---------------------------------------------------------------------------
. "$PHPRT_RUNNER/contract.sh"

recipe_meta() {
  recipe_header "$1" | tr ' ' '\n' | grep '=' | sort
}

# recipe 的统一入口。$0 一定是 recipe 自己，所以可直接定位 bundle。
recipe_main() {
  local verb="${1:-}" sh kind name v
  sh="$(readlink -f "$0")"
  kind="$(recipe_kind_of "$sh")"
  name="$(recipe_name_of "$sh")"

  # 直接执行 recipe（不经过 driver）时，从所在目录反推 bundle
  if [ -z "${BUNDLE:-}" ]; then
    BUNDLE="$(cd "$(dirname "$sh")/.." && pwd)"
    export BUNDLE
  fi
  load_config
  recipe_validate "$sh"

  case "$verb" in
    --meta)    recipe_meta "$sh" ;;
    --opts)    recipe_has_func "$sh" opts && opts || true ;;
    --version) recipe_version "$sh" ;;
    --configure-args)
      recipe_has_func "$sh" configure_args && configure_args || true ;;
    --flags)
      recipe_has_func "$sh" flags || die "$kind/$name 没有 flags()"
      flags ;;
    --needs)
      if recipe_has_func "$sh" needs; then
        needs
      else
        recipe_needs_of "$sh"
      fi ;;
    --check)   ok "recipe $kind/$name 校验通过" ;;
    versions)  recipe_has_func "$sh" versions && versions || true ;;
    latest_version) recipe_has_func "$sh" latest_version && latest_version || true ;;
    build)
      recipe_has_func "$sh" build || die "$kind/$name 没有 build()"
      build ;;
    stage-static)
      recipe_has_func "$sh" stage_static || die "$kind/$name 没有 stage_static()"
      stage_static ;;
    enable)    recipe_has_func "$sh" enable && enable || true ;;
    verify)    recipe_has_func "$sh" verify && verify || true ;;
    ""|-h|--help)
      printf '用法: %s {--meta|--opts|--version|--configure-args|--flags|--needs|--check|build|stage-static|enable|verify}\n' "$name" ;;
    *) die "未知动词: $verb（$kind/$name）" ;;
  esac
}

ensure_base_files() {
  # 配置不再放在 .phprt 根下：php.ini / conf.d 都属于具体版本树（标准布局）。
  # 构建工作区按版本分：build-php-<版本>/{src,logs,tools,.lock}。
  # 源码包统一放在 .phprt/downloads，不跟构建目录走。
  [ -n "${BUILD_ROOT:-}" ] ||
    die "内部错误：ensure_base_files 之前没有设置 BUILD_ROOT"
  mkdir -p "$BUILD_ROOT/src" "$BUILD_ROOT/logs" "$BUILD_ROOT/tools"
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

write_myphp() {
  local tpl="$PHPRT_RUNNER/myphp.tmpl"
  [ -f "$tpl" ] || die "缺少模板: $tpl"
  cp -a "$tpl" "$PHPRT_PROJECT/myphp.tmp"
  chmod +x "$PHPRT_PROJECT/myphp.tmp"
  mv -f "$PHPRT_PROJECT/myphp.tmp" "$PHPRT_PROJECT/myphp"
}

# ---------------------------------------------------------------------------
# 扩展状态不落盘：直接问目标版本树的 PHP。
# `php -m` 是加载后的真值，用户手改 php.ini、删 .so、注释掉 extension= 都会
# 反映出来，phprt 不再维护 .phprt/project.conf。
# ---------------------------------------------------------------------------

# 用和 myphp 完全一样的环境跑某个版本树的 php（PHPRC / scan dir / extension_dir），
# 否则 php 会读编译期默认配置，得到的模块列表不准。
tree_php() {
  local ver="$1" tree ext="" d
  shift
  tree="$(tree_of "$ver")"
  [ -x "$tree/bin/php" ] || return 1
  for d in "$tree"/lib/php/extensions/*/; do
    [ -d "$d" ] && { ext="${d%/}"; break; }
  done
  local -a args=()
  [ -n "$ext" ] && args=(-d "extension_dir=$ext")
  PHPRC="$tree/etc/php.ini" \
  PHP_INI_SCAN_DIR="$tree/etc/conf.d" \
  SSL_CERT_FILE="$tree/etc/ssl/cert.pem" \
    "$tree/bin/php" "${args[@]}" "$@"
}

# 某版本实际加载的扩展（只返回 bundle 里认得的名字，按 recipe 的 modules 匹配）
tree_exts() {
  local ver="$1" e m loaded out=""
  [ -n "${BUNDLE:-}" ] || return 0
  loaded="$(tree_php "$ver" -m 2>/dev/null | tr '[:upper:]' '[:lower:]')" || return 0
  [ -n "$loaded" ] || return 0
  for e in $(all_ext_names); do
    for m in $(ext_modules "$e"); do
      if printf '%s\n' "$loaded" | grep -cix "$m" >/dev/null; then
        out="$out${out:+ }$e"
        break
      fi
    done
  done
  printf '%s\n' "$out"
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
  # 全 static 安装可能根本没有 lib/php/extensions/（没有任何 .so）
  extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
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
    if [ -n "$extdir" ] && [ -f "$extdir/$m.so" ]; then
      any_shared=1
    elif LD_LIBRARY_PATH="$ld" "$tree/bin/php" -m 2>/dev/null | grep -cix "$m" >/dev/null; then
      any_static=1
    else
      die "$m 没有编译；用 phprt install $ver --recompile --ext $name"
    fi
  done

  # ext recipe 声明了 ini 时由 recipe 自己写（可带扩展专属配置）
  local sh kind ini
  sh="$(ext_recipe_sh "$name")" || sh="$(module_sh "$name")"
  kind="$(recipe_kind_of "$sh")"
  ini="$(recipe_ini "$sh")"
  if [ "$any_shared" = 1 ] && [ "$kind" = ext ] && [ -n "$ini" ] && [ "$ini" != - ]; then
    bash "$sh" enable || die "扩展 $name 的 enable() 失败"
    ok "启用扩展 $name（$ini）"
    return 0
  fi

  if [ "$any_shared" = 1 ]; then
    mkdir -p "$inidir"
    {
      printf '; 由 phprt 生成（%s）\n' "$name"
      for m in $mods; do
        [ -f "$extdir/$m.so" ] || continue
        printf 'extension=%s.so\n' "$m"
      done
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

# 构建工作区按 PHP 版本分：项目 .phprt/build-php-<版本>/。删掉只影响该版本的
# 增量重建，不同版本的源码/对象互不干扰。PHPRT_WORK 可以改基目录。
# 版本号要等读完 bundle.conf 才知道，所以这里只提供函数，由调用方先设后建。
phprt_build_root() {
  printf '%s/build-php-%s\n' "${PHPRT_WORK:-$PHPRT_HOME}" "$1"
}

phprt_set_build_root() {
  BUILD_ROOT="$(phprt_build_root "$1")"
  SRCDIR="$BUILD_ROOT/src"
  LOGS="$BUILD_ROOT/logs"
  DOWNLOADS="${PHPRT_DOWNLOADS:-$PHPRT_HOME/downloads}"
  export BUILD_ROOT SRCDIR LOGS DOWNLOADS
}

# ---------------------------------------------------------------------------
# chroot 构建（phprt buildroot）
# 状态在 <project>/.phprt/buildroots/.config：ROOT_DEFAULT / BUILD_ENV。
# 默认宿主构建；--chroot / --host 或 PHPRT_BUILD_ENV 覆盖。
# ---------------------------------------------------------------------------

buildroot_conf_get() {  # $1=ROOT_DEFAULT|BUILD_ENV
  local key="$1" f="$PHPRT_HOME/buildroots/.config"
  [ -f "$f" ] || return 0
  # 文件由 buildroot.sh 生成，可信；用子 shell 避免污染当前变量
  ( . "$f"; eval "printf '%s\n' \"\${$key:-}\"" )
}

build_env_get() {
  local env="${PHPRT_BUILD_ENV:-}"
  [ -n "$env" ] && { printf '%s\n' "$env"; return 0; }
  env="$(buildroot_conf_get BUILD_ENV)"
  printf '%s\n' "${env:-host}"
}

buildroot_default_root() {
  local root
  root="$(buildroot_conf_get ROOT_DEFAULT)"
  [ -n "$root" ] || root="$PHPRT_HOME/buildroots/default"
  printf '%s\n' "$root"
}

buildroot_set_env() {  # $1=chroot|host
  local script="$PHPRT_REPO/scripts/buildroot.sh"
  [ -x "$script" ] || die "缺少 scripts/buildroot.sh（chroot 构建需要在 phprt 仓库里运行）"
  "$script" env "$1" || die "设置构建环境失败: $1"
}

# host: 直接执行；chroot: scripts/buildroot.sh exec 挂载后执行
BUILDROOT_JOB_PID=""
run_build_env() {
  if [ "$(build_env_get)" != chroot ]; then
    "$@"
    return
  fi

  local script="$PHPRT_REPO/scripts/buildroot.sh"
  [ -x "$script" ] || die "缺少 scripts/buildroot.sh（chroot 构建需要在 phprt 仓库里运行）"
  local root="${PHPRT_CHROOT_ROOT:-$(buildroot_default_root)}"
  [ -d "$root" ] ||
    die "chroot rootfs 不存在: $root（先跑 phprt buildroot create）"

  # build.sh 在 chroot 里重新读这些环境变量；先确保它们进入子进程环境
  local v
  for v in SELECTED_EXTS PHPRT_SEED PHPRT_FORCE_PHP PHPRT_CLEAN_SRC \
           PHPRT_EXT_MODES PHPRT_EXT_OPTS PHPRT_PHP_VERSION PHPRT_WORK \
           PHPRT_DOWNLOADS PHPRT_NO_DOWNLOAD JOBS OFFLINE https_proxy \
           HTTPS_PROXY http_proxy HTTP_PROXY NO_PROXY no_proxy GITHUB_TOKEN; do
    [ -n "${!v:-}" ] && export "$v"
  done

  # 项目外的构建区也必须按原绝对路径绑定；项目/仓库由 buildroot.sh 默认绑定
  local bind="${BUILD_ROOT:-}"
  if [ -n "${PHPRT_SEED:-}" ] && [ -d "$PHPRT_SEED" ]; then
    bind="$bind:$PHPRT_SEED"
  fi
  if [ -n "${DOWNLOADS:-}" ] && [ -d "$DOWNLOADS" ]; then
    bind="$bind:$DOWNLOADS"
  fi
  # 仓库/项目的 downloads 可能是指向仓库外的软链；把真实目录也绑进去
  local d
  for d in "$PHPRT_REPO/downloads" "$PHPRT_PROJECT/downloads"; do
    [ -d "$d" ] || continue
    bind="$bind:$(realpath -m -- "$d")"
  done

  # 后台跑 buildroot.sh，父进程收到 INT/TERM 时先转给子进程，让它按
  # 自己的 trap 卸载挂载，再退出。
  PHPRT_CHROOT_DETACH=1 \
  PHPRT_CHROOT_BINDS="${bind#:}" \
  PHPRT_PROJECT="$PHPRT_PROJECT" \
  PHPRT_HOME="$PHPRT_HOME" \
    "$script" exec "$root" -- "$@" &
  BUILDROOT_JOB_PID=$!
  trap 'kill -TERM "$BUILDROOT_JOB_PID" 2>/dev/null; wait "$BUILDROOT_JOB_PID" 2>/dev/null; exit 143' TERM
  trap 'kill -TERM "$BUILDROOT_JOB_PID" 2>/dev/null; wait "$BUILDROOT_JOB_PID" 2>/dev/null; exit 130' INT
  trap 'kill -TERM "$BUILDROOT_JOB_PID" 2>/dev/null; wait "$BUILDROOT_JOB_PID" 2>/dev/null; exit 129' HUP

  local rc=0
  wait "$BUILDROOT_JOB_PID" || rc=$?
  BUILDROOT_JOB_PID=""
  trap - INT TERM HUP
  return "$rc"
}


# recipe 子进程从环境继承这些路径。直接单独跑 recipe（不经过 driver）时，
# 只派生出 DOWNLOADS 的兜底位置，避免写进空路径。
if [ -n "${BUILD_ROOT:-}" ]; then
  SRCDIR="${SRCDIR:-$BUILD_ROOT/src}"
  LOGS="${LOGS:-$BUILD_ROOT/logs}"
  DOWNLOADS="${DOWNLOADS:-${PHPRT_DOWNLOADS:-$PHPRT_HOME/downloads}}"
  export SRCDIR LOGS DOWNLOADS
else
  DOWNLOADS="${DOWNLOADS:-${PHPRT_DOWNLOADS:-$PHPRT_HOME/downloads}}"
  export DOWNLOADS
fi

# 构建期 Perl shim：仓库自带，不再复制到项目。
VENDOR="${VENDOR:-$PHPRT_RUNNER/vendor}"
[ -d "$VENDOR" ] || die "缺少 $VENDOR（runner/vendor），构建工具无法运行"
export VENDOR

# 上游版本查询（upstream_latest_* / version_cmp 等），recipe 也能直接用。
# shellcheck disable=SC1091
. "$PHPRT_RUNNER/upstream.sh"

# 源码清单与下载
# shellcheck disable=SC1091
. "$PHPRT_RUNNER/sources.sh"

# 交互式扩展选择（纯 bash 自绘菜单）
# shellcheck disable=SC1091
. "$PHPRT_RUNNER/ui.sh"

# PECL 扩展的通用构建流程（ext_pecl_build），recipe 只写差异部分。
# shellcheck disable=SC1091
. "$PHPRT_RUNNER/recipe-ext.sh"

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
  local name var override
  name="$(printf '%s' "$1" | tr 'a-z-' 'A-Z_')"
  # 单模块覆盖 > 全局开关 > bundle.conf 的 LTO_<模块>。
  # --static-all-lto 用全局 PHPRT_LTO=1，同时打开 PHP 和依赖。
  override="PHPRT_LTO_$name"
  if [ -n "${!override:-}" ]; then
    [ "${!override}" = 1 ]
    return
  fi
  if [ -n "${PHPRT_LTO:-}" ]; then
    [ "$PHPRT_LTO" = 1 ]
    return
  fi
  var="LTO_$name"
  [ "${!var:-0}" = 1 ]
}

lto_state() { if lto_on "$1"; then printf 1; else printf 0; fi; }

# 依赖 LTO 的公共参数。recipe 用它们保留自己的 -O2/-O3 基线后再追加 LTO。
dep_lto_cflags()  { if lto_on deps; then printf '%s\n' "$LTO_CFLAGS";  fi; }
dep_lto_ldflags() { if lto_on deps; then printf '%s\n' "$LTO_LDFLAGS"; fi; }
dep_ar()          { if lto_on deps; then printf gcc-ar; else printf ar; fi; }
dep_ranlib()      { if lto_on deps; then printf gcc-ranlib; else printf ranlib; fi; }

# 精确版安装（install 8.5.11，pin 是 8.5.12）不复制 bundle：读完仓库 bundle 后，
# 只把 PHP 自身的下载信息换成目标版本，其它 pin 原样保留。
apply_php_override() {
  local v="${PHPRT_PHP_VERSION:-}"
  [ -n "$v" ] || return 0
  [ "$v" = "${PHP_VERSION:-}" ] && return 0
  PHP_VERSION="$v"
  PHP_ARCHIVE="php-$v.tar.xz"
  PHP_URL="https://www.php.net/distributions/php-$v.tar.xz"
  PHP_SRC_DIR="php-$v"
  # 精确版不是 pin，没有预置 SHA；依赖库本来也不校验，保持一致
  PHP_SHA256=""
  export PHP_VERSION PHP_ARCHIVE PHP_URL PHP_SRC_DIR PHP_SHA256
}

load_config() {
  [ -n "${BUNDLE:-}" ] || return 0
  load_bundle_conf "$BUNDLE"
  apply_php_override
}

# 先读 phprt 自带的 bundle 模板，再读项目自己的 builder-<系列>.conf。
# 项目文件可以只写覆盖项；删掉某行就自动回落到模板值。
builder_conf_of() {  # $1=系列
  printf '%s/builder-%s.conf\n' "$PHPRT_HOME" "$1"
}

load_bundle_conf() {  # $1=bundle 目录 [$2=系列]
  local b="${1:-${BUNDLE:-}}" series="${2:-}"
  [ -n "$b" ] || return 0
  [ -n "$series" ] || series="$(series_of "${PHP_VERSION:-0.0}")"
  # shellcheck disable=SC1090
  . "$b/bundle.conf"
  local p
  p="$(builder_conf_of "$series")"
  if [ -f "$p" ]; then
    # shellcheck disable=SC1090
    . "$p"
  fi
}

dep_dir() { printf '%s/%s-%s\n' "$DEPS" "$1" "$2"; }

# 统一从 recipe 注册表取安装位置（dep/tool/ext/builtin 都支持）。
dep_dir_of() { recipe_dir_of "$1"; }

# 模块的硬依赖 + 构建工具。条件依赖由 recipe 的 needs() 动态给出，所以这里
# 用子进程跑一次 `--needs`（纯查询，不下载不编译）。
module_needs() {
  local n="$1" sh out
  sh="$(module_sh "$n")"
  if recipe_has_func "$sh" needs; then
    out="$(bash "$sh" --needs || true)"
  else
    out="$(recipe_needs_of "$sh")"
  fi
  out="$out $(recipe_tools_of "$sh")"
  printf '%s\n' $out | awk 'NF && !seen[$0]++'
}

# 签名/构建状态的存放位置：dep 和 tool 在各自安装目录，ext/builtin/php 在树里
stamp_file() {
  local n="$1" d
  case "$n" in
    php) d="$TREE" ;;
    *)   d="$(recipe_dir_of "$n")" ;;
  esac
  printf '%s/.phprt-stamp-%s\n' "$d" "$n"
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
  local url="$1" dest="$2" expect="${3:-}" name
  name="$(basename "$dest")"
  ensure_source "$name" "$url" "$expect"
  [ -f "$dest" ] || copy_hard "$DOWNLOADS/$name" "$dest"
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
  local n d out="" rpath=1
  # 静态链接时 PHP 只使用 .a，不能写依赖目录的绝对 rpath；
  # 否则 verify 的 RPATH 检查和“可单独复制”都会失败。
  if link_is_static; then
    rpath=0
  else
    out="-Wl,--disable-new-dtags "
  fi
  for n in "$@"; do
    d="$(dep_dir_of "$n")"
    [ -d "$d/lib" ] && {
      out+="-L$d/lib "
      [ "$rpath" = 1 ] && out+="-Wl,-rpath,$d/lib "
    }
    [ -d "$d/lib64" ] && {
      out+="-L$d/lib64 "
      [ "$rpath" = 1 ] && out+="-Wl,-rpath,$d/lib64 "
    }
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

dep_names() {
  local f
  for f in "$BUNDLE"/deps/*.sh; do
    [ -f "$f" ] || continue
    basename "$f" .sh
  done
}

# 已构建的依赖名。名字以 recipe 注册表为准，不从目录名反推：
# 版本号可以带连字符（ImageMagick 7.1.2-35），`${b%-*}` 会截错。
enabled_dep_names() {
  local n d
  for n in $(dep_names); do
    d="$(dep_dir_of "$n")" || continue
    [ -d "$d" ] && printf '%s\n' "$n"
  done
  # 最后一个没编的依赖会让循环返回 1；调用方在 $(...) 里配 set -e 会直接退出。
  return 0
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
  local sh kind d
  sh="$(module_sh "$1")" || return 1
  kind="$(recipe_kind_of "$sh")"
  d="$(recipe_dir_of "$1")"
  case "$kind" in
    dep)
      [ -d "$d" ] || return 1
      find "$d" -maxdepth 3 \( -name '*.so' -o -name '*.so.*' -o -name '*.a' \) \
        -print -quit 2>/dev/null | grep -q . ;;
    tool)
      [ -d "$d" ] && [ -n "$(ls -A "$d" 2>/dev/null)" ] ;;
    *)  [ -d "$d" ] ;;
  esac
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

# 依赖的稳定身份：dep 用 SONAME 集合（同系列小版本升级不变），tool/ext 用
# recipe 内容 hash。
recipe_id() {
  local sh kind
  sh="$(module_sh "$1")"
  kind="$(recipe_kind_of "$sh")"
  case "$kind" in
    dep) dep_rt_id "$1" ;;
    *)   recipe_hash "$1" ;;
  esac
}

# 模块自己的开关状态（只取本模块的 switch，不扫全部 ENABLE_*：
# 否则加一个不相干的扩展会让所有依赖/扩展签名变化、全部重编）
recipe_switch_state() {
  local sh sw v
  sh="$(module_sh "$1")" || return 0
  sw="$(recipe_switch "$sh")"
  [ -n "$sw" ] || return 0
  v="${!sw:-0}"
  printf 'switch=%s:%s\n' "$sw" "$v"
  return 0
}

dep_sig() {
  local n="$1" x
  {
    printf 'recipe=%s\n' "$(recipe_hash "$n")"
    # 版本进签名：同版本目录残留（降级后再升级、手工塞目录）不能算已完成。
    # 消费者是否重编仍由 recipe_id 的 SONAME 列表决定，同 ABI 补丁不牵连。
    printf 'version=%s\n' "$(ext_version "$n")"
    # 只有 dep 需要 LTO；tool（perlshim/autoconf）不参与 PHP 链接。
    if [ "$(recipe_kind_of "$(module_sh "$n")")" = dep ] && lto_on deps; then
      printf 'lto=1\n'
      printf 'lto_flags=%s|%s\n' "$LTO_CFLAGS" "$LTO_LDFLAGS"
    fi
    recipe_switch_state "$n"
    # 条件依赖是否参与构建已经体现在 module_needs 的解析结果里。
    # 安装目录名（含版本）也要进签名：autotools 的 .la 里记的是绝对路径，
    # 依赖目录被 prune 后旧 .la 失效，消费者必须重编。
    for x in $(module_needs "$n"); do
      printf 'need=%s:%s:%s:%s\n' "$x" "$(recipe_id "$x")" "$(recipe_hash "$x")" \
        "$(basename "$(recipe_dir_of "$x")")"
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
  log "构建 $n"
  # 外层 dep_sig 已经判定要重编；必须绕过配方内部的 stamp，
  # 否则会出现「外层要重编、内层跳过」，随后把旧产物记成新签名。
  FORCE=1 bash "$(module_sh "$n")" build
  artifact_ok "$n" || die "依赖 $n 构建后没有产物"
  if lto_on deps && [ "$(recipe_kind_of "$(module_sh "$n")")" = dep ]; then
    local _a _d _seen=0
    _d="$(dep_dir_of "$n")"
    for _a in "$_d"/lib/*.a "$_d"/lib64/*.a; do
      [ -f "$_a" ] || continue
      _seen=1
      readelf -S "$_a" 2>/dev/null | grep -c '\.gnu\.lto' >/dev/null ||
        die "依赖 $n 的静态库没有 LTO 段（recipe 未接 LTO）: $_a"
    done
    # PHP 静态链接需要的依赖必须有 .a；这里提前报，避免到 PHP 链接阶段才发现
    case " ${PHPRT_PHP_DEPS:-} " in
      *" $n "*)
        [ "$_seen" = 1 ] ||
          die "依赖 $n 没有静态库（--static-all 需要 .a，见 docs/STATIC-ALL.md）" ;;
    esac
  fi
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
  local sh
  sh="$(module_sh "$1")"
  recipe_version "$sh"
}

ext_sig() {
  local n="$1" x
  {
    printf 'recipe=%s\n' "$(recipe_hash "$n")"
    recipe_switch_state "$n"
    printf 'version=%s\n' "$(ext_version "$n")"
    printf 'mode=%s\n' "$(mode_of "$n" 2>/dev/null || true)"
    printf 'php_version=%s\n' "${PHP_VERSION:-}"
    printf 'php_abi=%s\n' "$(php_abi_sig)"
    for x in $(module_needs "$n"); do
      printf 'dep=%s:%s:%s\n' "$x" "$(recipe_id "$x")" \
        "$(basename "$(recipe_dir_of "$x")")"
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

# wheel= 例外声明（只有 dep 用）：auto / none / exclude:a+b /
# alias:libfoo.so.N / singleton:a+b；多条用 ; 连接。
wheel_spec() {  # $1=dep 名字 -> 规范字符串（缺省 auto）
  local sh v
  sh="$(module_sh "$1" 2>/dev/null || true)"
  if [ -z "$sh" ]; then printf 'auto\n'; return 0; fi
  v="$(recipe_wheel "$sh")"
  printf '%s\n' "${v:-auto}"
}

wheel_directive() {  # $1=dep 名字 $2=指令名 -> 名字列表（空格分隔）
  local spec wd
  spec="$(wheel_spec "$1")"
  wd="$(printf '%s\n' "$spec" | tr ';' '\n' | sed -n "s/^$2://p")"
  [ -n "$wd" ] && printf '%s\n' "${wd//+/ }"
  return 0
}

# 每个 singleton 组单独一行（组内名字用空格分隔），用于组内版本一致性校验
wheel_groups() {  # $1=dep 名字
  wheel_spec "$1" | tr ';' '\n' | sed -n 's/^singleton://p' | tr '+' ' '
  return 0
}

wheelize_tree() {
  local tree="$1" flat="$1/lib/phprt"
  local f soname new old elf osoname onew dup depdir depname depver line rest spec
  need_cmds patchelf sha256sum
  mkdir -p "$flat"

  # 保留上一轮映射：跨 ABI 系列升级时运行时名会变，已改写过的 NEEDED 要能
  # 跟到新名字；同系列小版本升级名字不变，映射为空、零改写。
  local prev="$flat/wheel.map.prev" repl="$flat/.replace"
  if [ -f "$flat/wheel.map" ]; then cp -f "$flat/wheel.map" "$prev"; else : > "$prev"; fi
  : > "$flat/wheel.map"
  : > "$flat/exclude.map"
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
    depname="${depdir%-*}"
    depver="${depdir#*-}"

    # wheel= 例外：none 不允许产出共享库；exclude 复制都不做，只记排除清单
    spec="$(wheel_spec "$depname")"
    [ "$spec" != none ] ||
      die "wheel=none 的依赖 $depname 产出了共享库: $f"
    if printf '%s\n' "$(wheel_directive "$depname" exclude)" |
         tr ' ' '\n' | grep -qx "$soname"; then
      printf '%s|%s|%s\n' "$soname" "-" "$depver" >> "$flat/exclude.map"
      continue
    fi

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

  # alias：代码里 dlopen 原始 SONAME 时，在 lib/phprt 补一个指向 rt 名的软链
  local d alias_name target
  for d in "$tree"/deps/*; do
    [ -d "$d" ] || continue
    depname="$(basename "$d")"
    depname="${depname%-*}"
    for alias_name in $(wheel_directive "$depname" alias); do
      target="$(awk -F'|' -v s="$alias_name" '$1 == s { print $2; exit }' "$flat/wheel.map")"
      [ -n "$target" ] ||
        die "alias:$alias_name 在 wheel.map 里找不到对应的运行时库（$depname）"
      ln -sfn "$target" "$flat/$alias_name"
    done
  done

  ok "运行时库已整理到 $flat（$(wc -l < "$flat/wheel.map") 个）"
}

# 全树 ELF 的链接关系快照：相对路径 | SONAME | 有序 NEEDED | RPATH/RUNPATH。
# 构建前后各取一份做 diff，可用于断言消费方的动态链接关系零变化。
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
  # singleton 组（recipe 的 wheel=singleton:a+b）：组内每个 SONAME 恰好一份，
  # 且 map 里的版本列一致。不再写死 libssl/libcrypto，跨 ABI 系列也不用改校验。
  # 声明写的是库的基础名（libssl），map 里是完整 SONAME（libssl.so.3），按
  # “名字 + .so.” 前缀匹配。
  local sh grp g cnt ver0 ver1
  while IFS= read -r grp; do
    [ -n "$grp" ] || continue
    ver1=""
    for g in $grp; do
      cnt="$(awk -F'|' -v s="$g" '$1 ~ "^" s "\\.so\\." { n++ } END { print n+0 }' "$map")"
      [ "$cnt" = 1 ] || { warn "singleton $g 在 wheel.map 中不是恰好一份 ($cnt)"; return 1; }
      ver0="$(awk -F'|' -v s="$g" '$1 ~ "^" s "\\.so\\." { print $3; exit }' "$map")"
      [ -n "$ver1" ] || ver1="$ver0"
      [ "$ver0" = "$ver1" ] ||
        { warn "singleton 组版本不一致: $g=$ver0，同组应为 $ver1"; return 1; }
    done
  done <<EOF
$(for sh in "$BUNDLE"/deps/*.sh; do
    [ -f "$sh" ] || continue
    wheel_groups "$(recipe_name_of "$sh")"
  done)
EOF
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
