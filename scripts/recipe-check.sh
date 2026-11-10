#!/usr/bin/env bash
# 开发用契约检查：新增或修改 bundles/ 下的 recipe 后跑一次，
# 全部通过才算添加成功。这是仓库开发工具，不是 phprt 的用户命令。
#
# 用法：
#   scripts/recipe-check.sh              # 静态检查，不联网
#   scripts/recipe-check.sh --deep       # 额外联网确认 upstream 可查、pin 在版本列表里
#   scripts/recipe-check.sh --deep 8.5   # 只检查指定系列
#
# 退出码：0 通过 / 1 契约或 pin 有问题 / 2 参数错误 / 3 上游查询失败。
set -uo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

# 复用 recipe 契约校验和上游查询，不再经由 phprt 命令入口
# shellcheck source=/dev/null
. "$ROOT/runner/common.sh"
# shellcheck source=/dev/null
. "$ROOT/runner/upstream.sh"
set +eu

deep=0
series=()
for a in "$@"; do
  case "$a" in
    --deep) deep=1 ;;
    -h|--help)
      sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    -*) printf '未知参数: %s\n' "$a" >&2; exit 2 ;;
    *)  series+=("$a") ;;
  esac
done

if [ ${#series[@]} -eq 0 ]; then
  while IFS= read -r s; do
    [ -n "$s" ] && series+=("$s")
  done < <(ls -d "$ROOT"/bundles/php* 2>/dev/null | sed 's#.*/bundles/php##')
fi
[ ${#series[@]} -gt 0 ] || { printf '错误: bundles/php* 下没有 bundle\n' >&2; exit 1; }

if [ "$deep" = 1 ] && [ "${OFFLINE:-0}" = 1 ]; then
  printf '错误: --deep 需要联网，不能和 OFFLINE=1 一起用\n' >&2
  exit 2
fi

fail=0
net=0
total=0

for s in "${series[@]}"; do
  bundle="$ROOT/bundles/php$s"
  if [ ! -d "$bundle" ]; then
    printf '错误: 没有 bundle: %s\n' "$bundle" >&2
    exit 2
  fi
  printf '== php%s ==\n' "$s"

  export BUNDLE="$bundle"
  # shellcheck disable=SC1090
  . "$bundle/bundle.conf"

  for kind in builtin deps ext tools; do
    for f in "$bundle/$kind"/*.sh; do
      [ -f "$f" ] || continue
      name="$(basename "$f" .sh)"
      total=$((total + 1))

      if ! bash -n "$f" 2>/dev/null; then
        printf '  ✗ %s/%s 语法错误\n' "$kind" "$name"
        bash -n "$f" >/dev/null || true
        fail=1
        continue
      fi
      if ! out="$(bash "$f" --check 2>&1)"; then
        printf '  ✗ %s/%s 契约校验失败\n' "$kind" "$name"
        printf '%s\n' "$out" | sed 's/^/      /'
        fail=1
        continue
      fi
      if recipe_has_func "$f" opts; then
        if ! out="$(bash "$f" --opts 2>&1)"; then
          printf '  ✗ %s/%s opts() 执行失败\n' "$kind" "$name"
          printf '%s\n' "$out" | sed 's/^/      /'
          fail=1
          continue
        fi
        if ! printf '%s\n' "$out" | awk -F'\t' '
          NF == 0 { next }
          NF < 3 || $1 !~ /^[a-z][a-z0-9_]*$/ || $2 !~ /^(on|off|fixed)$/ || seen[$1]++ {
            exit 1
          }
        '; then
          printf '  ✗ %s/%s opts() 输出格式错误（需要 key<TAB>state<TAB>label）\n' "$kind" "$name"
          fail=1
          continue
        fi
        osw okey ostate olabel ovar obad=0
        osw="$(recipe_field "$f" switch)"
        while IFS=$'\t' read -r okey ostate olabel; do
          [ -n "$okey" ] || continue
          [ "$ostate" = fixed ] && continue
          ovar="$(ext_opt_var "$osw" "$okey")"
          if ! grep -q "$ovar" "$f"; then
            printf '  ✗ %s/%s opts() 的 %s 没有在 recipe 里使用 %s\n' \
              "$kind" "$name" "$okey" "$ovar"
            obad=1
          fi
        done <<< "$out"
        if [ "$obad" = 1 ]; then
          fail=1
          continue
        fi
      fi
      if [ "$deep" != 1 ] || [ "$kind" = builtin ]; then
        printf '  ok %s/%s\n' "$kind" "$name"
        continue
      fi

      var="$(recipe_field "$f" version)"
      cur="${!var:-}"
      if [ "$(recipe_field "$f" upstream)" = none ] &&
         ! recipe_has_func "$f" versions && ! recipe_has_func "$f" latest_version; then
        printf '  ok %s/%s（upstream=none，跳过版本检查）\n' "$kind" "$name"
        continue
      fi
      vers="$(module_versions "$f" || true)"
      if [ -z "$vers" ]; then
        printf '  ✗ %s/%s 上游查不到版本（%s）\n' "$kind" "$name" \
          "$(recipe_field "$f" upstream)"
        net=1
      elif ! printf '%s\n' "$vers" | grep -qx "$cur"; then
        printf '  ✗ %s/%s pin %s 不在上游版本列表\n' "$kind" "$name" "$cur"
        fail=1
      else
        printf '  ok %s/%s pin %s\n' "$kind" "$name" "$cur"
      fi
    done
  done

  if [ "$deep" = 1 ]; then
    # php.net 的接口按具体版本查也能返回该版本，用 pin 自己确认它存在
    vers="$(upstream_versions_phpnet "$PHP_VERSION" 2>/dev/null || true)"
    if [ -z "$vers" ]; then
      printf '  ✗ core/php 上游查不到版本（%s）\n' "${PHP_UPSTREAM:-未设置}"
      net=1
    elif ! printf '%s\n' "$vers" | grep -qx "$PHP_VERSION"; then
      printf '  ✗ core/php pin %s 在上游不存在\n' "$PHP_VERSION"
      fail=1
    else
      printf '  ok core/php pin %s\n' "$PHP_VERSION"
    fi
  fi
done

printf '\n检查了 %d 个 recipe 文件\n' "$total"
if [ "$fail" = 1 ]; then
  printf '结果: 失败（契约或 pin 不符合规范）\n' >&2
  exit 1
fi
if [ "$net" = 1 ]; then
  printf '结果: 上游查询失败（网络或来源问题），未能完成 --deep 检查\n' >&2
  exit 3
fi
printf '结果: 通过\n'
