#!/usr/bin/env bash
# phprt：交互式扩展选择（纯 bash 自绘菜单）
#
# 渲染方式：
#   - 每帧整块一次 printf 写出，行尾用 \033[2K 清行，不清屏，因此按键时
#     没有“整块空白再重画”的闪烁；
#   - 默认同屏绘制：命令行和上面的输出一直可见，退出时只清菜单画布；
#     PHPRT_ALT_SCREEN=1 才切 alternate screen；
#   - 选中行加粗 + 背景色（NO_COLOR 时用反显）；
#   - 勾选标记三态：[x] 全选、[-] 部分子项关着、[ ] 未选中；
#   - 关闭自动换行（DECAWM），长行在右边界裁掉，不会把布局顶乱。
#
# 交互：
#   ↑↓ / Home / End / PgUp / PgDn 移动，Space 切换选中，Enter 展开/收起，
#   Esc 取消（在子项上是回到父行并收起）。
#   主菜单是“树形列表”：
#     - 扩展行：[x]/[-]/[ ] + 名字 + ▼/▶（有 opts() 才有箭头）
#     - 展开后子项缩进排在下面，固定项始终 [x]、灰色、不可切换
#     - 最后一行「✔ 确认并继续」：回车完成
#
# 数据来源只有 recipe 的 opts() / DEFAULT_EXTS，不写死扩展名。
# 非终端、--yes、--print-urls、PHPRT_SELECT=0 时由调用方回退到默认集。

# ---------------------------------------------------------------------------
# 会话缓存：扩展列表和 opts() 元数据只取一次；按键时只重算被改动的那一行。
# opens 元数据要跑 `bash <recipe> --opts`，34 个扩展每次都取会有两百多毫秒的
# 空白帧，所以必须缓存。
# ---------------------------------------------------------------------------
declare -A UI_HAS_OPTS=()   # 扩展 -> 0/1（有没有 opts()）
declare -A UI_OPTS_META=()   # 扩展 -> opts() 原始输出
declare -A UI_ROW_CACHE=()   # 扩展 -> 主菜单行文本
declare -A UI_EXPANDED=()    # 扩展 -> 0/1（展开子项）
declare -A UI_OPT_STATE=()   # "扩展:key" -> on/off/fixed
declare -A UI_CATEGORY=()    # 扩展 -> 分类名
UI_EXT_LIST=()

UI_TTY=""
UI_ALT=0
UI_SM=""
UI_RM=""
UI_MENU_LIVE=0
UI_MENU_CANVAS=0
UI_KEY=""
UI_MENU_RESULT=""
UI_MENU_KEY=""
UI_ACTION_KEY=""
UI_INTERRUPT=0
UI_DRAW_HEADER=""
UI_DRAW_FOOTER=""
UI_DRAW_IDX=0
UI_DRAW_OFFSET=0
UI_DRAW_VIEW=0
UI_DRAW_TOTAL=0
UI_META_TEXT=""
UI_ROW_TEXT=""
UI_ROW_DIM=0
UI_MENU_LABELS=()
UI_MENU_VALUES=()
UI_MENU_DIM=()
UI_MENU_SKIP=()   # 1 = 不可选中的行（分类标题）

UI_RESET=$'\033[0m'
UI_DIM=$'\033[2m'
UI_CYAN=$'\033[36m'
UI_BOLD=$'\033[1m'
UI_CAT=""
UI_SEL=""

ui_select_available() {
  [ -t 0 ] && [ -t 1 ] &&
    [ "${PHPRT_SELECT:-1}" != 0 ] &&
    { [ -t 2 ] || ui_tty_open; }
}

# /dev/tty 的存在不等于能打开：没有控制终端时 open 会失败（ENXIO）。
ui_tty_open() { (exec 9>/dev/tty) 2>/dev/null; }

ui_tty_path() {
  if ui_tty_open; then
    printf '/dev/tty'
    return 0
  fi
  if [ -t 2 ]; then
    printf '/dev/stderr'
    return 0
  fi
  return 1
}

# ---------------------------------------------------------------------------
# 选中集合
# ---------------------------------------------------------------------------
ui_ext_enabled() {  # $1=扩展名
  case " ${UI_DRAFT_EXTS:-} " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

ui_count_selected() {
  local c=0 e
  for e in ${UI_DRAFT_EXTS:-}; do c=$((c + 1)); done
  printf '%s' "$c"
}

ui_ext_enable() {  # $1=扩展名
  ui_ext_enabled "$1" && return 0
  UI_DRAFT_EXTS="${UI_DRAFT_EXTS:-}${UI_DRAFT_EXTS:+ }$1"
  unset "UI_ROW_CACHE[$1]"
}

ui_ext_toggle() {  # $1=扩展名
  local name="$1" e out=""
  if ui_ext_enabled "$name"; then
    for e in ${UI_DRAFT_EXTS:-}; do
      [ "$e" = "$name" ] && continue
      out="${out}${out:+ }$e"
    done
    UI_DRAFT_EXTS="$out"
  else
    UI_DRAFT_EXTS="${UI_DRAFT_EXTS:-}${UI_DRAFT_EXTS:+ }$name"
  fi
  unset "UI_ROW_CACHE[$name]"
}

# ---------------------------------------------------------------------------
# recipe opts() 元数据
# ---------------------------------------------------------------------------
ui_opts_has_key() {  # $1=逗号列表 $2=key
  case ",$1," in
    *",$2,"*) return 0 ;;
  esac
  return 1
}

# 读取（并缓存）一个扩展的 opts() 输出到 UI_META_TEXT。
# 缓存只写当前 shell；在 $( ) 里调用时读得到已有缓存，只是写不回去。
ui_load_meta() {  # $1=扩展名
  local name="$1" sh m key state label
  UI_META_TEXT=""
  if [ -n "${UI_HAS_OPTS[$name]+set}" ]; then
    if [ "${UI_HAS_OPTS[$name]}" = 1 ]; then
      UI_META_TEXT="${UI_OPTS_META[$name]}"
    fi
  else
    sh="$(ext_recipe_sh "$name" 2>/dev/null || true)"
    if [ -n "$sh" ] && recipe_has_func "$sh" opts; then
      m="$(recipe_opts_meta "$sh" || true)"
      UI_HAS_OPTS[$name]=1
      UI_OPTS_META[$name]="$m"
      UI_META_TEXT="$m"
    else
      UI_HAS_OPTS[$name]=0
    fi
  fi
  while IFS=$'\t' read -r key state label; do
    [ -n "$key" ] && UI_OPT_STATE["$name:$key"]="$state"
  done <<< "$UI_META_TEXT"
  return 0
}

ui_ext_has_opts() {  # $1=扩展名
  ui_load_meta "$1"
  [ -n "$UI_META_TEXT" ]
}

ui_cache_ready() {
  [ "${#UI_EXT_LIST[@]}" -gt 0 ]
}

ui_cache_build() {
  local name cat rank ord sh
  UI_EXT_LIST=()
  UI_CATEGORY=()
  # 先按分类顺序排，再按分类内的 menu_order 和扩展名排；分类来自 recipe 的 category=。
  while IFS=$'\t' read -r rank ord name cat; do
    [ -n "$name" ] || continue
    UI_EXT_LIST+=("$name")
    UI_CATEGORY[$name]="$cat"
  done < <(
    for name in $(all_ext_names); do
      sh="$(ext_recipe_sh "$name")" || continue
      cat="$(ext_category_of "$sh")"
      rank="$(ext_category_rank "$cat")"
      ord="$(ext_menu_order "$sh")"
      printf '%s\t%04d\t%s\t%s\n' "$rank" "$ord" "$name" "$cat"
    done | sort -t$'\t' -k1,1n -k2,2n -k3,3
  )
  for name in "${UI_EXT_LIST[@]}"; do
    ui_load_meta "$name"
  done
}

ui_opts_current() {  # $1=扩展名 -> 当前启用的 key 列表
  local ext="$1" key state label out=""
  if out="$(ext_opts_get "${UI_DRAFT_OPTS:-}" "$ext" 2>/dev/null)"; then
    printf '%s' "$out"
    return 0
  fi
  ui_load_meta "$ext"
  while IFS=$'\t' read -r key state label; do
    [ -n "$key" ] || continue
    case "$state" in
      on|fixed) out="${out}${out:+,}$key" ;;
    esac
  done <<< "$UI_META_TEXT"
  printf '%s' "$out"
}

ui_opts_with_fixed() {  # $1=扩展名 $2=key 列表 -> 补 fixed，并按 recipe 顺序
  local ext="$1" list="$2" key state label out="" picked
  ui_load_meta "$ext"
  picked=",$(printf '%s' "$list" | tr '\n' ',' | sed 's/,$//'),"
  while IFS=$'\t' read -r key state label; do
    [ -n "$key" ] || continue
    if [ "$state" != fixed ]; then
      case "$picked" in
        *",$key,"*) ;;
        *) continue ;;
      esac
    fi
    case ",$out," in
      *",$key,"*) ;;
      *) out="${out}${out:+,}$key" ;;
    esac
  done <<< "$UI_META_TEXT"
  printf '%s' "$out"
}

ui_opts_enable() {  # $1=扩展名 $2=key（只在缺失时加上，不切换）
  local ext="$1" key="$2" list out
  list="$(ui_opts_current "$ext")"
  ui_opts_has_key "$list" "$key" && return 0
  out="$(ui_opts_with_fixed "$ext" "$list,$key")"
  UI_DRAFT_OPTS="$(ui_opts_set "${UI_DRAFT_OPTS:-}" "$ext" "$out")"
  unset "UI_ROW_CACHE[$ext]"
}

ui_opts_toggle() {  # $1=扩展名 $2=key
  local ext="$1" key="$2" list out="" k
  list="$(ui_opts_current "$ext")"
  if ui_opts_has_key "$list" "$key"; then
    for k in ${list//,/ }; do
      [ "$k" = "$key" ] && continue
      out="${out}${out:+,}$k"
    done
  else
    out="${list}${list:+,}$key"
  fi
  out="$(ui_opts_with_fixed "$ext" "$out")"
  UI_DRAFT_OPTS="$(ui_opts_set "${UI_DRAFT_OPTS:-}" "$ext" "$out")"
  unset "UI_ROW_CACHE[$ext]"
}

ui_opts_set() {  # $1=spec $2=扩展名 $3=key 列表 -> 更新 spec
  local spec="$1" ext="$2" list="$3" pair out=""
  list="$(printf '%s' "$list" | tr '\n' ',' | sed 's/,$//')"
  for pair in $spec; do
    [ "${pair%%=*}" = "$ext" ] && continue
    out="${out}${out:+ }$pair"
  done
  out="${out}${out:+ }$ext=$list"
  printf '%s' "$out"
}

ui_opts_prune() {  # $1=spec $2=扩展列表 -> 只保留仍被选中的扩展
  local spec="$1" exts="$2" e list out=""
  for e in $exts; do
    if list="$(ext_opts_get "$spec" "$e" 2>/dev/null)"; then
      out="${out}${out:+ }$e=$list"
    fi
  done
  printf '%s' "$out"
}

# ---------------------------------------------------------------------------
# 主菜单父行文本：`[-] gd ▼`（三态标记 + 名字 + 展开箭头）。
# 勾选标记：
#   [x] 选中；有子项时要求所有非固定子项都开
#   [-] 选中，但至少有一个非固定子项关着
#   [ ] 未选中
# ---------------------------------------------------------------------------
ui_build_row() {  # $1=扩展名 -> UI_ROW_TEXT（并写入 UI_ROW_CACHE）
  local name="$1" key state olabel current="" line arrow=""
  local enabled=0 mark="[ ]" total=0 on=0
  if [ -n "${UI_ROW_CACHE[$name]+set}" ]; then
    UI_ROW_TEXT="${UI_ROW_CACHE[$name]}"
    return 0
  fi
  ui_ext_enabled "$name" && enabled=1
  ui_load_meta "$name"
  if [ -z "$UI_META_TEXT" ]; then
    if [ "$enabled" = 1 ]; then
      mark="[x]"
    fi
    line="$mark $name"
  else
    if [ "$enabled" = 1 ]; then
      current="$(ui_opts_current "$name")"
    fi
    while IFS=$'\t' read -r key state olabel; do
      [ -n "$key" ] || continue
      [ "$state" = fixed ] && continue
      total=$((total + 1))
      if [ "$enabled" = 1 ] && ui_opts_has_key "$current" "$key"; then
        on=$((on + 1))
      fi
    done <<< "$UI_META_TEXT"
    if [ "$enabled" = 1 ]; then
      if [ "$total" -gt 0 ] && [ "$on" -lt "$total" ]; then
        mark="[-]"
      else
        mark="[x]"
      fi
    fi
    if [ "${UI_EXPANDED[$name]:-0}" = 1 ]; then arrow=" ▼"; else arrow=" ▶"; fi
    line="$mark $name$arrow"
  fi
  UI_ROW_TEXT="$line"
  UI_ROW_CACHE[$name]="$line"
}

# 子项行：`    [x] jpeg` / `    [x] png (固定)`。
# 固定项（png）始终是 [x]，并且标成灰色、不可开关。
# 参数：$1=扩展名 $2=key $3=state $4=当前启用的 key 列表
ui_opt_row() {  # -> UI_ROW_TEXT / UI_ROW_DIM
  local name="$1" key="$2" state="$3" current="$4" mark="[ ]"
  UI_ROW_DIM=0
  if [ "$state" = fixed ]; then
    UI_ROW_TEXT="      [x] $key (固定)"
    UI_ROW_DIM=1
    return 0
  fi
  if ui_ext_enabled "$name" && ui_opts_has_key "$current" "$key"; then
    mark="[x]"
  fi
  UI_ROW_TEXT="      $mark $key"
}

ui_ext_row_label() {  # $1=扩展名 -> 主菜单一行（兼容函数形式）
  ui_build_row "$1"
  printf '%s' "$UI_ROW_TEXT"
}

# ---------------------------------------------------------------------------
# 终端绘制
# ---------------------------------------------------------------------------
ui_init_colors() {
  if [ -n "${NO_COLOR:-}" ] || [ "${TERM:-dumb}" = dumb ]; then
    UI_SEL=$'\033[1;7m'          # 无法确定颜色支持时用反显
    UI_CAT=$'\033[1m'            # 无颜色支持时用加粗区分分类
  else
    UI_SEL=$'\033[1;97;44m'      # 加粗、亮白字、蓝底
    UI_CAT=$'\033[1;36m'         # 分类标题：亮青色，和普通白字拉开、但不用暗色
  fi
}

# 默认同屏绘制：命令行和上面的历史输出保持可见，退出时只清菜单画布。
# PHPRT_ALT_SCREEN=1 才切 alternate screen。
ui_screen_enter() {
  [ -n "$UI_TTY" ] || return 1
  ui_init_colors
  if [ "${PHPRT_ALT_SCREEN:-0}" = 1 ]; then
    UI_SM="$(tput smcup 2>/dev/null || true)"
    UI_RM="$(tput rmcup 2>/dev/null || true)"
    if [ -n "$UI_SM" ]; then
      UI_ALT=1
      printf '%s\033[H\033[2J\033[?25l\033[?7l' "$UI_SM" > "$UI_TTY"
      UI_MENU_LIVE=1
      return 0
    fi
  fi
  UI_SM=""
  UI_RM=""
  UI_ALT=0
  printf '\033[?25l\033[?7l' > "$UI_TTY"
  UI_MENU_LIVE=1
}

ui_screen_leave() {
  [ -n "$UI_TTY" ] || return 0
  if [ "$UI_ALT" = 1 ]; then
    printf '\033[?7h\033[?25h%s' "$UI_RM" > "$UI_TTY"
  else
    printf '\033[?7h\033[?25h\r\033[0J' > "$UI_TTY"
  fi
  UI_ALT=0
}

ui_menu_abort() {
  if [ "${UI_MENU_LIVE:-0}" = 1 ]; then
    ui_screen_leave
    UI_MENU_LIVE=0
  fi
}

# 整帧一次写出：每行 \r + \033[2K + 内容，最后一次上移回到首行。
ui_menu_draw() {
  local -a lines=()
  local i j total="$UI_DRAW_TOTAL" view="$UI_DRAW_VIEW"
  local off="$UI_DRAW_OFFSET" idx="$UI_DRAW_IDX"

  lines+=("$UI_DIM$UI_DRAW_HEADER$UI_RESET")
  for ((i = 0; i < view; i++)); do
    j=$((off + i))
    if [ "$j" -ge "$total" ]; then
      lines+=("")
    elif [ "$j" -eq "$idx" ]; then
      lines+=("$UI_SEL> ${UI_MENU_LABELS[$j]}  $UI_RESET")
    elif [ "${UI_MENU_SKIP[$j]:-0}" = 1 ]; then
      if [ -z "${UI_MENU_LABELS[$j]}" ]; then
        lines+=("")
      else
        lines+=("$UI_CAT${UI_MENU_LABELS[$j]}$UI_RESET")
      fi
    elif [ "${UI_MENU_DIM[$j]:-0}" = 1 ]; then
      lines+=("$UI_DIM  ${UI_MENU_LABELS[$j]}$UI_RESET")
    else
      lines+=("  ${UI_MENU_LABELS[$j]}")
    fi
  done
  # 确认行下面留一行空隙，别让底栏贴着确认行
  lines+=("")
  lines+=("$UI_DIM$((idx + 1))/$total · $UI_DRAW_FOOTER$UI_RESET")

  local frame=$'\r' n=${#lines[@]} k
  for ((k = 0; k < n; k++)); do
    frame+=$'\033[2K'"${lines[k]}"
    if [ "$k" -lt $((n - 1)) ]; then
      frame+=$'\n'
    fi
  done
  # 清掉画布下方残留（收起的子项比上一帧少时），和本帧一起一次写出
  frame+=$'\033[0J\033['"$((n - 1))"'A'
  printf '%s' "$frame" > "$UI_TTY"
  UI_MENU_CANVAS=$n
}

# 读一个按键。方向键是 ESC [ A..D；单独 ESC 用 50ms 超时和序列区分。
ui_key_read() {
  local k r s num tail
  UI_KEY="other"
  if ! IFS= read -rsn1 -r k; then
    UI_KEY=eof
    return 0
  fi
  case "$k" in
    # read -n1 遇到行分隔符（回车）会返回空串，不是 \r
    ''|$'\r'|$'\n') UI_KEY=enter ;;
    $'\x03')     UI_KEY=int ;;
    $'\x04')     UI_KEY=eof ;;
    ' ')         UI_KEY=space ;;
    $'\x1b')
      if ! IFS= read -rsn1 -t 0.05 -r r; then
        UI_KEY=esc
        return 0
      fi
      if [ "$r" != '[' ]; then
        UI_KEY=esc
        return 0
      fi
      if ! IFS= read -rsn1 -t 0.05 -r s; then
        UI_KEY=esc
        return 0
      fi
      case "$s" in
        A) UI_KEY=up ;;
        B) UI_KEY=down ;;
        C) UI_KEY=right ;;
        D) UI_KEY=left ;;
        H) UI_KEY=home ;;
        F) UI_KEY=end ;;
        *)
          case "$s" in
            [1-8])
              num="$s"
              tail=""
              while IFS= read -rsn1 -t 0.05 -r s; do
                tail+="$s"
                [ "$s" = '~' ] && break
              done
              case "$num" in
                1|7) UI_KEY=home ;;
                4|8) UI_KEY=end ;;
                3)   UI_KEY=delete ;;
                5)   UI_KEY=pageup ;;
                6)   UI_KEY=pagedown ;;
                *)   UI_KEY=other ;;
              esac
              ;;
            *) UI_KEY=other ;;
          esac
          ;;
      esac
      ;;
    *) UI_KEY=other ;;
  esac
  return 0
}

# 视口顶部压在分类标题的下一行时，把标题拉进来，避免分组失去上下文。
ui_offset_with_header() {  # $1=offset -> 调整后的 offset
  local o="$1"
  # 光标在视口顶部时，把紧邻的分类标题一起带进来；标题上面可能有空行，
  # 连续跳过空行，直到遇到标题（非空 skip 行）或普通行。
  while [ "$o" -gt 0 ] && [ "${UI_MENU_SKIP[$((o - 1))]:-0}" = 1 ]; do
    o=$((o - 1))
    [ -n "${UI_MENU_LABELS[$o]}" ] && break
  done
  printf '%s' "$o"
}

ui_menu_run() {  # $1=标题 $2=底栏 $3=初始索引；结果放 UI_MENU_RESULT + UI_MENU_KEY
  local header="$1" footer="$2" idx="${3:-0}"
  local n=${#UI_MENU_LABELS[@]}
  if [ "$n" -lt 1 ]; then
    return 1
  fi
  case "$idx" in ''|*[!0-9]*) idx=0 ;; esac
  if [ "$idx" -ge "$n" ]; then idx=$((n - 1)); fi

  local term_h
  term_h="$(tput lines 2>/dev/null || true)"
  case "$term_h" in ''|*[!0-9]*) term_h="${LINES:-24}" ;; esac
  case "$term_h" in ''|*[!0-9]*) term_h=24 ;; esac
  if [ "$term_h" -lt 8 ]; then term_h=8; fi

  # 同屏绘制时给命令行和提示各留一行，避免菜单把上面的内容顶走；
  # alternate screen 从第 1 行开始画，只需要头尾两行。
  local reserve=4
  if [ "${UI_ALT:-0}" = 1 ]; then
    reserve=2
  fi
  local view=$((term_h - reserve))
  if [ "$view" -gt "$n" ]; then view=$n; fi
  if [ "$view" -lt 3 ]; then view=3; fi
  if [ "$view" -gt "$n" ]; then view=$n; fi

  local offset=$((idx - view + 1))
  if [ "$offset" -lt 0 ]; then offset=0; fi

  # 可选中行（跳过分类标题）。光标只在 sel 里移动，标题行不参与导航。
  local -a sel=()
  local i nsel pos=0
  for i in "${!UI_MENU_LABELS[@]}"; do
    [ "${UI_MENU_SKIP[$i]:-0}" = 1 ] || sel+=("$i")
  done
  nsel=${#sel[@]}
  if [ "$nsel" -lt 1 ]; then
    return 1
  fi
  for i in "${!sel[@]}"; do
    if [ "${sel[$i]}" = "$idx" ]; then pos=$i; break; fi
  done
  idx="${sel[$pos]}"

  local rc=0
  while :; do
    UI_DRAW_HEADER="$header"
    UI_DRAW_FOOTER="$footer"
    UI_DRAW_IDX=$idx
    UI_DRAW_OFFSET=$offset
    UI_DRAW_VIEW=$view
    UI_DRAW_TOTAL=$n
    ui_menu_draw

    ui_key_read
    case "$UI_KEY" in
      up)
        pos=$(( (pos - 1 + nsel) % nsel ))
        idx="${sel[$pos]}"
        ;;
      down)
        pos=$(( (pos + 1) % nsel ))
        idx="${sel[$pos]}"
        ;;
      home)
        pos=0
        idx="${sel[$pos]}"
        ;;
      end)
        pos=$((nsel - 1))
        idx="${sel[$pos]}"
        ;;
      pageup)
        pos=$((pos - view))
        if [ "$pos" -lt 0 ]; then pos=0; fi
        idx="${sel[$pos]}"
        ;;
      pagedown)
        pos=$((pos + view))
        if [ "$pos" -ge "$nsel" ]; then pos=$((nsel - 1)); fi
        idx="${sel[$pos]}"
        ;;
      enter|space)
        UI_MENU_KEY="$UI_KEY"
        UI_MENU_RESULT="${UI_MENU_VALUES[$idx]}"
        break
        ;;
      esc|eof)
        UI_MENU_KEY="$UI_KEY"
        UI_MENU_RESULT="${UI_MENU_VALUES[$idx]}"
        break
        ;;
      int)
        UI_INTERRUPT=1
        UI_MENU_KEY=int
        rc=1
        break
        ;;
      *)
        continue
        ;;
    esac

    if [ "$idx" -lt "$offset" ]; then
      offset="$(ui_offset_with_header "$idx")"
    fi
    if [ "$idx" -ge $((offset + view)) ]; then
      offset=$((idx - view + 1))
    fi
  done

  return "$rc"
}

# ---------------------------------------------------------------------------
# 主菜单
# ---------------------------------------------------------------------------
ui_main_menu() {  # 输入 UI_MAIN_CURSOR；输出 UI_MENU_ACTION + UI_MENU_KEY
  local name key state olabel i idx=0 total cur
  if ! ui_cache_ready; then
    ui_cache_build
  fi

  UI_MENU_LABELS=()
  UI_MENU_VALUES=()
  UI_MENU_DIM=()
  UI_MENU_SKIP=()
  local last_cat="__none__" cat
  for name in "${UI_EXT_LIST[@]}"; do
    cat="${UI_CATEGORY[$name]:-Other}"
    if [ "$cat" != "$last_cat" ]; then
      # 分类之间空一行（第一组除外），层级更清楚
      if [ "$last_cat" != "__none__" ]; then
        UI_MENU_LABELS+=("")
        UI_MENU_VALUES+=("__blank__")
        UI_MENU_DIM+=(0)
        UI_MENU_SKIP+=(1)
      fi
      UI_MENU_LABELS+=("$(ext_category_label "$cat")")
      UI_MENU_VALUES+=("__header__")
      UI_MENU_DIM+=(0)
      UI_MENU_SKIP+=(1)
      last_cat="$cat"
    fi
    ui_build_row "$name"
    UI_MENU_LABELS+=("$UI_ROW_TEXT")
    UI_MENU_VALUES+=("ext:$name")
    UI_MENU_DIM+=(0)
    UI_MENU_SKIP+=(0)

    if [ "${UI_EXPANDED[$name]:-0}" != 1 ]; then
      continue
    fi
    ui_load_meta "$name"
    [ -n "$UI_META_TEXT" ] || continue
    cur="$(ui_opts_current "$name")"
    while IFS=$'\t' read -r key state olabel; do
      [ -n "$key" ] || continue
      ui_opt_row "$name" "$key" "$state" "$cur"
      UI_MENU_LABELS+=("$UI_ROW_TEXT")
      UI_MENU_VALUES+=("opt:$name:$key")
      UI_MENU_DIM+=("$UI_ROW_DIM")
      UI_MENU_SKIP+=(0)
    done <<< "$UI_META_TEXT"
  done
  # 确认行和上面的列表留一个空行
  UI_MENU_LABELS+=("")
  UI_MENU_VALUES+=("__blank__")
  UI_MENU_DIM+=(0)
  UI_MENU_SKIP+=(1)
  UI_MENU_LABELS+=("✔ 确认并继续（$(ui_count_selected) 个扩展）")
  UI_MENU_VALUES+=("__confirm__")
  UI_MENU_DIM+=(0)
  UI_MENU_SKIP+=(0)
  total=${#UI_MENU_VALUES[@]}

  for i in "${!UI_MENU_VALUES[@]}"; do
    if [ "${UI_MENU_VALUES[$i]}" = "${UI_MAIN_CURSOR:-__confirm__}" ]; then
      idx=$i
      break
    fi
  done

  ui_menu_run \
    "选择要编译的扩展 · Space 切换 · Enter 展开/收起" \
    "Esc 取消 · Home/End 跳转" "$idx" || return 1
  UI_MENU_ACTION="$UI_MENU_RESULT"
  UI_ACTION_KEY="$UI_MENU_KEY"
  return 0
}

ui_ext_select() {  # $1=系列 $2=具体版本 $3=tree $4=初始扩展 $5=初始 opts
  local cursor="__confirm__" out rest ext key state
  local init_exts="${4:-}" init_opts="${5:-}"

  init_exts="$(printf '%s' "$init_exts" | tr ',' ' ' | tr -s ' ')"
  init_exts="${init_exts# }"
  init_exts="${init_exts% }"

  UI_TTY="$(ui_tty_path)" || return 1
  [ -n "$UI_TTY" ] || return 1

  UI_DRAFT_EXTS="$init_exts"
  UI_DRAFT_OPTS="$init_opts"
  UI_INTERRUPT=0
  UI_ROW_CACHE=()
  UI_EXPANDED=()
  UI_HAS_OPTS=()
  UI_OPTS_META=()
  UI_OPT_STATE=()
  UI_CATEGORY=()
  UI_EXT_LIST=()
  ui_cache_build

  # 屏幕（光标 / 自动换行 / 可选 alternate screen）在整个选择过程只进出一次，
  # 每次按键只在同一块画布上重绘，不会反复闪屏。
  ui_screen_enter || return 1
  local prev_traps
  prev_traps="$(trap -p INT TERM EXIT)"
  # Ctrl-C / kill：先恢复终端再退出，避免光标和屏幕留在半截状态
  trap 'ui_menu_abort; printf "\n已取消\n" >&2; exit 130' INT TERM
  trap 'ui_menu_abort' EXIT

  local rc=0
  while :; do
    UI_MAIN_CURSOR="$cursor"
    if ! ui_main_menu; then
      rc=1
      break
    fi
    out="$UI_MENU_ACTION"

    case "$UI_ACTION_KEY" in
      space)
        case "$out" in
          ext:*)
            ui_ext_toggle "${out#ext:}"
            cursor="$out"
            ;;
          opt:*)
            rest="${out#opt:}"
            ext="${rest%%:*}"
            key="${rest#*:}"
            # 固定子项不能关
            if [ "${UI_OPT_STATE[$ext:$key]:-}" != fixed ]; then
              if ui_ext_enabled "$ext"; then
                ui_opts_toggle "$ext" "$key"
              else
                # 父扩展没选中时子项显示 [ ]，这次按 Space 应该是打开
                ui_ext_enable "$ext"
                ui_opts_enable "$ext" "$key"
              fi
            fi
            cursor="$out"
            ;;
        esac
        ;;
      enter)
        case "$out" in
          __confirm__)
            break
            ;;
          ext:*)
            ext="${out#ext:}"
            if ui_ext_has_opts "$ext"; then
              if [ "${UI_EXPANDED[$ext]:-0}" = 1 ]; then
                UI_EXPANDED[$ext]=0
              else
                UI_EXPANDED[$ext]=1
              fi
              unset "UI_ROW_CACHE[$ext]"
            fi
            cursor="$out"
            ;;
        esac
        ;;
      esc)
        case "$out" in
          opt:*)
            # 子项上 Esc：回到父行并收起
            rest="${out#opt:}"
            ext="${rest%%:*}"
            UI_EXPANDED[$ext]=0
            unset "UI_ROW_CACHE[$ext]"
            cursor="ext:$ext"
            ;;
          ext:*)
            # 展开的父行上 Esc：先收起，再按一次才取消
            ext="${out#ext:}"
            if [ "${UI_EXPANDED[$ext]:-0}" = 1 ]; then
              UI_EXPANDED[$ext]=0
              unset "UI_ROW_CACHE[$ext]"
              cursor="ext:$ext"
            else
              rc=1
              break
            fi
            ;;
          *)
            rc=1
            break
            ;;
        esac
        ;;
      eof)
        rc=1
        break
        ;;
    esac
  done

  ui_menu_abort
  trap - INT TERM EXIT
  eval "$prev_traps"
  [ "$rc" -eq 0 ] || return 1

  UI_SELECTED_EXTS="$UI_DRAFT_EXTS"
  UI_SELECTED_OPTS="$(ui_opts_prune "$UI_DRAFT_OPTS" "$UI_DRAFT_EXTS")"
  if [ "${UI_SELECT_QUIET:-0}" != 1 ]; then
    printf '\n已选择扩展: %s\n' "${UI_SELECTED_EXTS:-（无，只编译 PHP）}"
    if [ -n "$UI_SELECTED_OPTS" ]; then
      printf '子选项: %s\n' "$UI_SELECTED_OPTS"
    fi
  fi
  return 0
}
