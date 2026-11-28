#!/usr/bin/env bash
# 本地验收：默认安装不编 mbstring / pcntl / gettext，之后用 add 以 shared 加入。
#
# 用法：
#   tests/local-ci.sh                  # 新建临时根，成功后删除
#   KEEP=1 tests/local-ci.sh           # 保留安装树，便于排查
#   ROOT=/tmp/xxx tests/local-ci.sh    # 指定空目录；不会被自动删除
#   SERIES=8.5 tests/local-ci.sh       # 换系列
#   OFFLINE=1 tests/local-ci.sh        # 只用下载缓存
#   DOWNLOAD_DIR=/path tests/local-ci.sh
#
# 下载缓存默认取仓库旁边的 downloads（存在时），否则由 phprt 用默认目录。

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERIES="${SERIES:-8.6}"
COMPONENTS=(mbstring pcntl gettext)
KEEP="${KEEP:-0}"

if [ -n "${ROOT:-}" ]; then
  mkdir -p "$ROOT"
  if [ -n "$(ls -A "$ROOT")" ]; then
    echo "ROOT 不是空目录：$ROOT" >&2
    exit 2
  fi
  CREATED=0
else
  ROOT="$(mktemp -d "${TMPDIR:-/tmp}/phprt-local-ci-XXXXXX")"
  CREATED=1
fi

DOWNLOAD_DIR="${DOWNLOAD_DIR:-}"
if [ -z "$DOWNLOAD_DIR" ] && [ -d "$REPO/../downloads" ]; then
  DOWNLOAD_DIR="$(cd "$REPO/../downloads" && pwd)"
fi

GLOBAL_ARGS=(--root "$ROOT" --recipes "$REPO/recipes")
[ -n "$DOWNLOAD_DIR" ] && GLOBAL_ARGS+=(--download-dir "$DOWNLOAD_DIR")
DOWNLOAD_ARGS=(--no-refresh)
[ "${OFFLINE:-0}" = "1" ] && DOWNLOAD_ARGS+=(--offline)

phprt() {
  ( cd "$REPO" && cargo run --release -q -- "${GLOBAL_ARGS[@]}" "$@" )
}

cleanup() {
  local rc=$?
  if [ "$rc" -eq 0 ] && [ "$CREATED" = "1" ] && [ "$KEEP" != "1" ]; then
    rm -rf "$ROOT"
  else
    echo "安装树：$ROOT" >&2
  fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
has_module() { "$1" -m 2>/dev/null | grep -qx "$2"; }
has_module_without_ini() { "$1" -n -m 2>/dev/null | grep -qx "$2"; }

step "安装 php@$SERIES，禁用 ${COMPONENTS[*]}"
phprt install "php@$SERIES" \
  --disable-default-component mbstring \
  --disable-default-component pcntl \
  --disable-default-component gettext \
  "${DOWNLOAD_ARGS[@]}"

ENTRY="$ROOT/myphp-$SERIES"
[ -x "$ENTRY" ] || fail "入口不存在：$ENTRY"
ENV_OUT="$("$ENTRY" --env)"
PHP_BIN="$(sed -n 's/^PHP=//p' <<<"$ENV_OUT")"
PHPRC="$(sed -n 's/^PHPRC=//p' <<<"$ENV_OUT")"
EXT_DIR="$(sed -n 's/^EXT_DIR=//p' <<<"$ENV_OUT")"
[ -n "$PHP_BIN" ] && [ -n "$PHPRC" ] || fail "解析入口 --env 失败"
TREE="$(cd "$(dirname "$PHPRC")/.." && pwd)"

step "确认初始安装没有 ${COMPONENTS[*]}"
for name in "${COMPONENTS[@]}"; do
  has_module "$ENTRY" "$name" && fail "初始安装不应该有 $name（php -m）"
  has_module_without_ini "$PHP_BIN" "$name" &&
    fail "初始二进制里不应该有 $name（php -n 能看到说明是静态编入）"
done
"$PHP_BIN" -i 2>/dev/null | grep -q -- '--disable-mbstring' ||
  fail "configure 里没有 --disable-mbstring"
"$PHP_BIN" -i 2>/dev/null | grep -q -- '--disable-pcntl' ||
  fail "configure 里没有 --disable-pcntl"
"$PHP_BIN" -i 2>/dev/null | grep -q -- '--without-gettext' ||
  fail "configure 里没有 --without-gettext"

for name in "${COMPONENTS[@]}"; do
  step "add $name（shared）"
  phprt add "php@$SERIES" "$name=mode:shared" "${DOWNLOAD_ARGS[@]}"

  [ -f "$EXT_DIR/$name.so" ] || fail "缺少 $name.so：$EXT_DIR/$name.so"
  [ -f "$TREE/etc/conf.d/20-$name.ini" ] || fail "缺少 20-$name.ini"
  has_module "$ENTRY" "$name" || fail "php -m 里没有 $name"
  has_module_without_ini "$PHP_BIN" "$name" &&
    fail "$name 在 php -n 里也能加载，不是动态库"
done

step "确认扩展函数可用"
"$ENTRY" -r 'exit(
  function_exists("mb_strlen") &&
  function_exists("pcntl_fork") &&
  function_exists("gettext") ? 0 : 1
);' || fail "动态加载后扩展函数不可用"

echo
echo "PASS: php@$SERIES 初始不编 ${COMPONENTS[*]}，add 后以 shared 加载"
