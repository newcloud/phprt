#!/usr/bin/env bash
# 契约层的最小回归测试：不依赖网络、不读写项目 .phprt。
set -uo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)"
RUNNER="$ROOT/runner"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/phprt-contract.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

BUNDLE="$TMP/bundle"
mkdir -p "$BUNDLE"/{builtin,ext,deps,tools}
cp "$ROOT/bundles/php8.5/bundle.conf" "$BUNDLE/bundle.conf"
cp "$ROOT/bundles/php8.5/aliases.conf" "$BUNDLE/aliases.conf"
cp "$ROOT/bundles/php8.5/notes.conf" "$BUNDLE/notes.conf"
for d in deps ext tools builtin; do
  for f in "$ROOT"/bundles/php8.5/"$d"/*.sh; do
    [ -f "$f" ] || continue
    ln -s "$f" "$BUNDLE/$d/$(basename "$f")"
  done
done

fail=0
total=0

write_recipe() {  # $1=path $2=header-extra $3=body
  local f="$1" extra="$2" body="$3" kind name
  kind="$(basename "$(dirname "$f")")"
  case "$kind" in deps) kind=dep ;; tools) kind=tool ;; esac
  name="$(basename "$f" .sh)"
  {
    printf '#!/usr/bin/env bash\n'
    printf '# phprt: contract=v2 kind=%s name=%s %s\n' "$kind" "$name" "$extra"
    printf 'set -euo pipefail\n'
    printf 'source "${PHPRT_RUNNER:?}/common.sh"\n'
    printf '%s\n' "$body"
    printf 'recipe_main "$@"\n'
  } > "$f"
}

run_case() {  # $1=名称 $2=ok|fail $3=文件 $4=期望输出片段
  local name="$1" expect="$2" file="$3" needle="${4:-}" out rc
  total=$((total + 1))
  if out="$(PHPRT_RUNNER="$RUNNER" BUNDLE="$BUNDLE" \
      bash -c 'source "$1/common.sh"; recipe_validate "$2"' _ "$RUNNER" "$file" 2>&1)"; then
    rc=0
  else
    rc=$?
  fi
  if [ "$expect" = ok ]; then
    if [ "$rc" != 0 ]; then
      printf '  ✗ %s（应通过，实际 rc=%s）\n%s\n' "$name" "$rc" "$out"
      fail=1
    else
      printf '  ok %s\n' "$name"
    fi
  else
    if [ "$rc" = 0 ]; then
      printf '  ✗ %s（应失败，实际通过）\n' "$name"
      fail=1
    elif [ -n "$needle" ] && ! printf '%s' "$out" | grep -q "$needle"; then
      printf '  ✗ %s（输出不含 %s）\n%s\n' "$name" "$needle" "$out"
      fail=1
    else
      printf '  ok %s\n' "$name"
    fi
  fi
}

# 合法 builtin
f="$BUNDLE/builtin/ok.sh"
write_recipe "$f" \
  'switch=ENABLE_OK modules=ok ini=- type=static default=static category=Other' \
  'flags() { if [ "${ENABLE_OK:-0}" = 1 ]; then printf "%s\n" --enable-ok; else printf "%s\n" --disable-ok; fi; }
needs() { return 0; }'
run_case "valid builtin" ok "$f"

# 缺 modules
f="$BUNDLE/builtin/missing.sh"
write_recipe "$f" \
  'switch=ENABLE_MISSING ini=- type=static default=static category=Other' \
  'flags() { if [ "${ENABLE_MISSING:-0}" = 1 ]; then printf "%s\n" --enable-missing; else printf "%s\n" --disable-missing; fi; }
needs() { return 0; }'
run_case "missing modules" fail "$f" '缺少必填字段'

# 未知字段
f="$BUNDLE/builtin/unknown.sh"
write_recipe "$f" \
  'switch=ENABLE_UNKNOWN modules=unknown ini=- type=static default=static category=Other bogus=1' \
  'flags() { if [ "${ENABLE_UNKNOWN:-0}" = 1 ]; then printf "%s\n" --enable-unknown; else printf "%s\n" --disable-unknown; fi; }
needs() { return 0; }'
run_case "unknown field" fail "$f" '未知字段'

# 重复字段
f="$BUNDLE/builtin/dup.sh"
write_recipe "$f" \
  'switch=ENABLE_DUP modules=dup ini=- type=static default=static category=Other category=Other' \
  'flags() { if [ "${ENABLE_DUP:-0}" = 1 ]; then printf "%s\n" --enable-dup; else printf "%s\n" --disable-dup; fi; }
needs() { return 0; }'
run_case "duplicate field" fail "$f" '重复字段'

# dep 上写 category
f="$BUNDLE/deps/badcat.sh"
write_recipe "$f" \
  'version=ZLIB_VERSION needs= optional= tools= upstream=none category=Other' \
  'build() { :; }'
run_case "dep category" fail "$f" '不允许字段'

# 非法 menu_order
f="$BUNDLE/builtin/badmenu.sh"
write_recipe "$f" \
  'switch=ENABLE_BADMENU modules=badmenu ini=- type=static default=static category=Other menu_order=x' \
  'flags() { if [ "${ENABLE_BADMENU:-0}" = 1 ]; then printf "%s\n" --enable-badmenu; else printf "%s\n" --disable-badmenu; fi; }
needs() { return 0; }'
run_case "bad menu_order" fail "$f" '非负整数'

# 未定义 note
f="$BUNDLE/builtin/badnote.sh"
write_recipe "$f" \
  'switch=ENABLE_BADNOTE modules=badnote ini=- type=static default=static category=Other note=no-such-note' \
  'flags() { if [ "${ENABLE_BADNOTE:-0}" = 1 ]; then printf "%s\n" --enable-badnote; else printf "%s\n" --disable-badnote; fi; }
needs() { return 0; }'
run_case "unknown note" fail "$f" 'notes.conf'

# 末行不是 recipe_main
f="$BUNDLE/builtin/badlast.sh"
write_recipe "$f" \
  'switch=ENABLE_BADLAST modules=badlast ini=- type=static default=static category=Other' \
  'flags() { if [ "${ENABLE_BADLAST:-0}" = 1 ]; then printf "%s\n" --enable-badlast; else printf "%s\n" --disable-badlast; fi; }
needs() { return 0; }'
printf ':\n' >> "$f"
run_case "last line" fail "$f" '最后一行'

printf '\n检查了 %d 个契约 fixture\n' "$total"
if [ "$fail" = 1 ]; then
  printf '结果: 失败\n' >&2
  exit 1
fi
printf '结果: 通过\n'
