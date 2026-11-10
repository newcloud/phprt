#!/usr/bin/env bash
# recipe 契约层：唯一字段表、头部解析、L1 校验。
#
# 这里只认 docs/RECIPE.md 的字段矩阵；driver 的其他文件通过 accessor 取字段，
# 不再各自解析头部。opts()/wheel= 的 parser 仍放在 common.sh，后续阶段再收口。

# key|scope|required|validator|default
#   scope    = 允许出现的 kind，* 表示全部
#   required = 必填的 kind，* 表示全部
#   validator= 值域/格式校验器；空表示只做 scope/required
PHPRT_CONTRACT_SPEC=(
  "contract|*|*|contract|"
  "kind|*||kind|"
  "name|*||name|"
  "switch|builtin ext|builtin ext|switch|"
  "version|ext dep tool|ext dep tool|version_var|"
  "modules|builtin ext|builtin|module_list|"
  "type|builtin ext|builtin ext|modes|"
  "default|builtin ext|builtin ext|default_in_type|"
  "ini|builtin ext|builtin ext|ini|"
  "tools|ext dep tool|ext dep tool|resolvable_list|"
  "needs|builtin ext dep|ext dep|resolvable_list|"
  "optional|ext dep|dep|resolvable_list|"
  "wheel|dep||wheel|auto"
  "upstream|*||upstream|"
  "desc|*||no_space|"
  "category|builtin ext|builtin ext|category|"
  "menu_order|builtin ext||uint|1000"
  "note|*||note_slugs|"
)

# 解析缓存：key 是 "$file|$key"，按文件保存，循环多个 recipe 不会互相覆盖。
declare -gA PR_META=()
declare -gA PR_HAS=()
declare -gA PR_LOADED=()
declare -gA PR_KEYS=()
declare -gA PR_DUP=()
declare -gA PR_BAD=()

_recipe_parse() {
  local f="$1" hdr tok k v
  [ -n "${PR_LOADED[$f]+x}" ] && return 0
  PR_LOADED[$f]=1
  hdr="$(recipe_header "$f")"
  [ -n "$hdr" ] || return 0

  local -a toks=()
  read -r -a toks <<< "$hdr"
  for tok in "${toks[@]}"; do
    case "$tok" in
      *=*) ;;
      *) PR_BAD[$f]="${PR_BAD[$f]:-}${PR_BAD[$f]:+ }$tok"; continue ;;
    esac
    k="${tok%%=*}"
    v="${tok#*=}"
    case "$k" in
      [A-Za-z_]*) ;;
      *) PR_BAD[$f]="${PR_BAD[$f]:-}${PR_BAD[$f]:+ }$tok"; continue ;;
    esac
    if [ -n "${PR_HAS[$f|$k]+x}" ]; then
      PR_DUP[$f]="${PR_DUP[$f]:-}${PR_DUP[$f]:+ }$k"
      continue
    fi
    PR_HAS[$f|$k]=1
    PR_META[$f|$k]="$v"
    PR_KEYS[$f]="${PR_KEYS[$f]:-}${PR_KEYS[$f]:+ }$k"
  done
}

recipe_meta_load() { _recipe_parse "$1"; }

recipe_has_key() {  # $1=文件 $2=键
  _recipe_parse "$1"
  [ -n "${PR_HAS[$1|$2]+x}" ]
}

recipe_field() {  # $1=文件 $2=键
  _recipe_parse "$1"
  [ -n "${PR_HAS[$1|$2]+x}" ] && printf '%s\n' "${PR_META[$1|$2]}"
  return 0
}

_recipe_spec_row() {  # $1=key -> 设置 SPEC_SCOPE / SPEC_REQ / SPEC_VAL / SPEC_DEF
  local row k s r v d
  for row in "${PHPRT_CONTRACT_SPEC[@]}"; do
    IFS='|' read -r k s r v d <<< "$row"
    if [ "$k" = "$1" ]; then
      SPEC_SCOPE="$s"; SPEC_REQ="$r"; SPEC_VAL="$v"; SPEC_DEF="$d"
      return 0
    fi
  done
  return 1
}

_field_in_list() {  # $1=空格分隔列表 $2=值；* 匹配全部
  [ "$1" = "*" ] && return 0
  case " $1 " in
    *" $2 "*) return 0 ;;
  esac
  return 1
}

_recipe_allowed_for_kind() {  # $1=kind
  local row k s r v d out=""
  for row in "${PHPRT_CONTRACT_SPEC[@]}"; do
    IFS='|' read -r k s r v d <<< "$row"
    _field_in_list "$s" "$1" && out="${out}${out:+ }$k"
  done
  printf '%s\n' "$out"
}

_recipe_validate_field() {  # $1=file $2=key $3=value $4=kind
  local f="$1" k="$2" v="$3" kind="$4" t d
  case "${SPEC_VAL:-}" in
    "") ;;
    contract)
      [ "$v" = v2 ] || die "recipe 不是 contract=v2: $f（当前: ${v:-无}）" ;;
    kind)
      [ "$v" = "$(recipe_kind_of "$f")" ] ||
        die "recipe kind 与目录不一致: $f（kind=$v，目录=$(recipe_kind_of "$f")）" ;;
    name)
      [ "$v" = "$(recipe_name_of "$f")" ] ||
        die "recipe name 与文件名不一致: $f（name=$v）" ;;
    switch)
      printf '%s' "$v" | grep -qE '^ENABLE_[A-Z0-9_]+$' ||
        die "switch 必须是 ENABLE_[A-Z0-9_]+: $v（$f）"
      # ext 的 switch 由 driver 设置环境变量，不要求 recipe 体内引用。
      if [ "$kind" = builtin ]; then
        grep -v '^# *phprt:' "$f" | grep -Fq "$v" ||
          die "switch $v 没有在 recipe 里使用: $f"
      fi ;;
    version_var)
      grep -qE "^[[:space:]]*$v=" "$f" ||
        grep -qE "^$v=" "${BUNDLE:-}/bundle.conf" ||
        die "version 变量 $v 既不在 recipe 也不在 bundle.conf: $f" ;;
    module_list)
      [ -n "$v" ] || die "recipe 缺少 modules: $f"
      for t in ${v//,/ }; do
        printf '%s' "$t" | grep -qE '^[a-z][a-z0-9_]*$' ||
          die "modules 里的 $t 不合法（应为小写模块名）: $f"
      done ;;
    modes)
      case "$v" in
        static|shared|static,shared|shared,static) ;;
        *) die "recipe 的 type 不合法: $v（应为 static、shared 或 static,shared）
文件: $f" ;;
      esac ;;
    default_in_type)
      t="$(recipe_field "$f" type)"
      case " $(printf '%s' "$t" | tr ',' ' ') " in
        *" $v "*) ;;
        *) die "recipe 的 default=$v 不在 type=$t 里: $f" ;;
      esac ;;
    ini)
      case "$v" in
        -) ;;
        [0-9][0-9]-*.ini) ;;
        *) die "recipe 的 ini 字段不合法: ${v:-空}（应为 - 或 20-<name>.ini）
文件: $f" ;;
      esac ;;
    resolvable_list)
      for t in ${v//,/ }; do
        module_sh "$t" >/dev/null || die "$k 里的 $t 找不到 recipe: $f"
      done ;;
    wheel)
      local wl item names
      wl="${v:-auto}"
      while IFS= read -r item; do
        [ -n "$item" ] || continue
        case "$item" in
          auto|none) ;;
          exclude:*|singleton:*)
            names="${item#*:}"
            printf '%s' "$names" | grep -qE '^[A-Za-z0-9_.+-]+(\+[A-Za-z0-9_.+-]+)*$' ||
              die "wheel $item 不合法（不能带路径，名字用 + 分隔）: $f" ;;
          alias:*)
            printf '%s' "${item#alias:}" | grep -qE '^[A-Za-z0-9_.+-]+\.so(\.[0-9]+)+$' ||
              die "wheel alias 必须是 *.so.N 形式: $item（$f）" ;;
          *) die "wheel 指令不认识: $item（$f）" ;;
        esac
      done < <(printf '%s\n' "$wl" | tr ';' '\n') ;;
    upstream)
      case "$v" in
        none) ;;
        github:*)
          printf '%s' "${v#github:}" | grep -qE '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' ||
            die "upstream=github: 后面必须是 owner/repo: $v（$f）" ;;
        pecl:*)
          printf '%s' "${v#pecl:}" | grep -qE '^[A-Za-z0-9_.-]+$' ||
            die "upstream=pecl: 后面必须是包名: $v（$f）" ;;
        phpnet:*)
          printf '%s' "${v#phpnet:}" | grep -qE '^[0-9]+\.[0-9]+$' ||
            die "upstream=phpnet: 后面必须是 大版本.小版本: $v（$f）" ;;
        *) die "未知的 upstream 类型: $v（$f）
可选: github:owner/repo、pecl:包名、phpnet:x.y、none" ;;
      esac ;;
    no_space)
      printf '%s' "$v" | grep -q '[[:space:]]' &&
        die "$k 不能包含空格（头部字段用空格分隔）: $f" ;;
    category)
      [ -n "${PHPRT_CATEGORY_LABELS[$v]+x}" ] ||
        die "category 不合法: $v（$f）
可选: ${PHPRT_EXT_CATEGORIES[*]}
示例: category=Database" ;;
    uint)
      case "$v" in
        ''|*[!0-9]*) die "$k 必须是非负整数: $v（$f）" ;;
      esac ;;
    note_slugs)
      for t in ${v//+/ }; do
        printf '%s' "$t" | grep -qE '^[a-z0-9][a-z0-9-]*$' ||
          die "note slug 不合法: $t（$f）"
        if [ -f "${BUNDLE:-}/notes.conf" ]; then
          grep -qE "^$t=" "${BUNDLE}/notes.conf" ||
            die "note slug 未在 notes.conf 定义: $t（$f）"
        fi
      done ;;
    *)
      die "内部错误：未知字段校验器 ${SPEC_VAL}（$f）" ;;
  esac
}

recipe_validate() {
  local sh="$1" kind name hdr key row missing="" dup bad
  local scope req val dflt has allowed required
  local -a spec_keys=()

  kind="$(recipe_kind_of "$sh")"
  name="$(recipe_name_of "$sh")"
  hdr="$(recipe_header "$sh")"
  [ -n "$hdr" ] || die "recipe 缺少 phprt 头: $sh
规范: docs/RECIPE.md#3-头部字段"

  _recipe_parse "$sh"
  bad="${PR_BAD[$sh]:-}"
  [ -z "$bad" ] || die "recipe 头部有无法解析的字段: $bad（格式 key=value）
文件: $sh"
  dup="${PR_DUP[$sh]:-}"
  [ -z "$dup" ] || die "recipe 头部重复字段: $dup
文件: $sh"

  for key in ${PR_KEYS[$sh]:-}; do
    _recipe_spec_row "$key" || die "recipe 有未知字段: $key
文件: $sh
规范: docs/RECIPE.md#3-头部字段"
    spec_keys+=("$key")
  done

  for row in "${PHPRT_CONTRACT_SPEC[@]}"; do
    IFS='|' read -r key scope req val dflt <<< "$row"
    recipe_has_key "$sh" "$key" && has=1 || has=0
    _field_in_list "$scope" "$kind" && allowed=1 || allowed=0
    _field_in_list "$req" "$kind" && required=1 || required=0

    if [ "$has" = 1 ] && [ "$allowed" = 0 ]; then
      die "$kind recipe 不允许字段: $key（允许: $(_recipe_allowed_for_kind "$kind")）
文件: $sh"
    fi
    if [ "$has" = 0 ] && [ "$required" = 1 ]; then
      missing="$missing $key"
      continue
    fi
    if [ "$has" = 1 ]; then
      SPEC_SCOPE="$scope"; SPEC_REQ="$req"; SPEC_VAL="$val"; SPEC_DEF="$dflt"
      _recipe_validate_field "$sh" "$key" "$(recipe_field "$sh" "$key")" "$kind"
    fi
  done

  [ -z "$missing" ] || die "recipe 缺少必填字段:$missing
文件: $sh
规范: docs/RECIPE.md#31-字段矩阵"

  # kind/name 可省略；写了必须和路径一致（validator 已查，这里覆盖省略的情况）
  local q
  q="$(recipe_field "$sh" kind)"
  [ -z "$q" ] || [ "$q" = "$kind" ] || die "recipe kind 与目录不一致: $sh"
  q="$(recipe_field "$sh" name)"
  [ -z "$q" ] || [ "$q" = "$name" ] || die "recipe name 与文件名不一致: $sh"

  # 上游来源：只有 builtin 可以完全没有；其它 kind 需要 upstream 或 versions()
  local up
  up="$(recipe_field "$sh" upstream)"
  if [ "$kind" != builtin ]; then
    [ -n "$up" ] || recipe_has_func "$sh" versions || recipe_has_func "$sh" latest_version ||
      die "$kind/$name 没有版本来源：加 upstream=（或 upstream=none），或实现 versions()
文件: $sh
规范: docs/RECIPE.md 的「版本查询」一节"
  fi

  # ext 的 ini 不是 - 时必须实现 enable()
  if [ "$kind" = ext ]; then
    local ini
    ini="$(recipe_field "$sh" ini)"
    [ "$ini" = - ] || recipe_has_func "$sh" enable ||
      die "ext/$name 声明了 ini=$ini 但没有 enable()
文件: $sh"
  fi

  # 函数
  case "$kind" in
    builtin)
      recipe_has_func "$sh" flags || die "builtin/$name 缺少 flags()
文件: $sh"
      recipe_has_func "$sh" needs || die "builtin/$name 缺少 needs()
文件: $sh" ;;
    ext|dep|tool)
      recipe_has_func "$sh" build || die "$kind/$name 缺少 build()
文件: $sh" ;;
  esac

  # 最后一行必须是 recipe_main "$@"
  local last
  last="$(awk 'NF { line=$0 } END { print line }' "$sh")"
  if ! printf '%s\n' "$last" | grep -qE '^recipe_main[[:space:]]+"\$@"[[:space:]]*$'; then
    die "recipe 最后一行必须是 recipe_main \"\$@\"
文件: $sh
模板: docs/RECIPE.md#8-模板"
  fi

  local offenses
  offenses="$(recipe_toplevel_offenses "$sh")"
  [ -z "$offenses" ] || die "recipe 顶层出现构建代码（必须放进 build()/flags()/needs()）:
$offenses
文件: $sh"
}
