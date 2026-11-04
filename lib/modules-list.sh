#!/usr/bin/env bash
# =============================================================================
#  列出所有“节点”：类别 / 名字 / 开关 / 依赖 / 状态
#
#    builtin  PHP 内置扩展（由 php.sh 收集参数，不单独构建）
#    deps     独立库（安装到 <安装根>/deps/）
#    ext      PECL 扩展（安装到 php-<版本>/）
#    app      其它产物（composer.phar）
#
#  一个 ENABLE_ 开关必须恰好被这里的一个文件认领（verify 的 5d 会检查）。
#  用法: phprt modules   （或 ./build.sh --modules）
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/common.sh"
load_config

switch_of() { # switch_of <文件>
  sed -n 's/^# *phprt:.*[[:space:]]switch=\([^[:space:]]*\).*/\1/p' "$1" | head -1
}

rows() {
  printf '类别\t名字\t开关\t依赖\t状态\n'

  local f n sw needs st
  for f in "$MODULES_BUILTIN"/*.sh; do
    [ -f "$f" ] || continue
    n="$(basename "$f" .sh)"
    sw="$(switch_of "$f")"
    needs="$(bash "$f" --needs | tr '\n' ' ')"
    needs="${needs% }"
    if [ -n "$sw" ] && [ "${!sw:-0}" = 1 ]; then st=开; else st=关; fi
    printf 'builtin\t%s\t%s\t%s\t%s\n' "$n" "${sw:--}" "${needs:--}" "$st"
  done

  for f in "$MODULES_DEPS"/*.sh; do
    [ -f "$f" ] || continue
    n="$(basename "$f" .sh)"
    sw="$(switch_of "$f")"
    needs="$(module_needs "$n" | tr ' ' ',')"
    if [ -f "$(stamp_file "$n")" ]; then st=已装; else st=未装; fi
    printf 'deps\t%s\t%s\t%s\t%s\n' "$n" "${sw:--}" "${needs:--}" "$st"
  done

  for f in "$MODULES_EXT"/*.sh; do
    [ -f "$f" ] || continue
    n="$(basename "$f" .sh)"
    sw="$(switch_of "$f")"
    needs="$(module_needs "$n" | tr ' ' ',')"
    if ls "$PREFIX"/etc/conf.d/*-"$n".ini >/dev/null 2>&1; then st=已装; else st=未装; fi
    printf 'ext\t%s\t%s\t%s\t%s\n' "$n" "${sw:--}" "${needs:--}" "$st"
  done

  sw="$(switch_of "$MODULES/composer.sh")"
  if [ -f "$PHPRT_HOME/composer.phar" ]; then st=已装; else st=未装; fi
  printf 'app\t%s\t%s\t%s\t%s\n' composer "${sw:--}" - "$st"
}

if command -v column >/dev/null 2>&1; then
  rows | column -t -s $'\t'
else
  rows | awk -F'\t' '{ printf "%-8s %-10s %-16s %-30s %s\n", $1, $2, $3, $4, $5 }'
fi
