#!/usr/bin/env bash
# =============================================================================
#  phprt 安装器 —— 用户唯一需要记住的命令
#
#  用法:
#    ./install.sh                     交互式：选安装位置 → 选 PHP 版本 → 选 php.ini
#    ./install.sh --yes               全用默认值（全局 ~/.phprt + 最新版本 + 默认 ini）
#    ./install.sh --local             装到当前目录下的 .phprt（只给这个项目用）
#    ./install.sh --prefix /opt/php   装到指定目录
#    ./install.sh --php 8.5.12        指定版本（见 lib/versions/）
#    ./install.sh --ini dev           用内置 ini 预设（default / dev / prod）
#    ./install.sh --ini-local         把预设复制成项目里的 ./php.ini 再指向它
#    ./install.sh --ini /path/php.ini 用已有的 ini 文件
#    ./install.sh --no-entry          不生成项目入口 ./myphp
#    ./install.sh --keep-src          保留 build/ 里解压出来的源码（--rebuild 更快）
#    ./install.sh --offline           只用手里的下载包，缺包直接报错
#    ./install.sh --force             已装的版本也重新编译
#    ./install.sh --jobs 8            并行度
#
#  做的事：装 PHP 到安装根 → 在当前目录生成 ./myphp（自包含的启动器）
#  （./myphp 里固化版本和 ini，就是本项目的 PHP 运行环境）
# =============================================================================
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"

MODE=""
PREFIX_OPT=""
PHP_OPT=""
INI_OPT=""
INI_LOCAL=0
NO_APP=0
KEEP_SRC=0
ASSUME_YES=0
FORCE=0
OFFLINE=0
JOBS_OPT=""

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) awk 'NR>1 && /^#/ {sub(/^# ?/, ""); print; next} NR>1 {exit}' "$0"; exit 0 ;;
    --global)    MODE=global; shift ;;
    --local)     MODE=local; shift ;;
    --prefix)    PREFIX_OPT="${2:-}"; shift 2 ;;
    --prefix=*)  PREFIX_OPT="${1#*=}"; shift ;;
    --php)       PHP_OPT="${2:-}"; shift 2 ;;
    --php=*)     PHP_OPT="${1#*=}"; shift ;;
    --ini)       INI_OPT="${2:-}"; shift 2 ;;
    --ini=*)     INI_OPT="${1#*=}"; shift ;;
    --ini-local) INI_LOCAL=1; shift ;;
    --no-app|--no-entry) NO_APP=1; shift ;;
    --keep-src)  KEEP_SRC=1; shift ;;
    -y|--yes)    ASSUME_YES=1; shift ;;
    --force)     FORCE=1; shift ;;
    --offline)   OFFLINE=1; shift ;;
    --jobs)      JOBS_OPT="${2:-}"; shift 2 ;;
    --jobs=*)    JOBS_OPT="${1#*=}"; shift ;;
    *) printf '未知参数: %s（--help 查看用法）\n' "$1" >&2; exit 2 ;;
  esac
done

if [ "$ASSUME_YES" != 1 ] && [ ! -t 0 ]; then
  printf '非交互环境：请加 --yes 或明确给出 --global/--local/--php/--ini\n' >&2
  exit 2
fi

# ---------------------------------------------------------------- 小工具 ----
menu() { # menu <标题> <默认序号> <选项...> -> 输出选中的序号（从 1 开始）
  local title="$1" def="$2"; shift 2
  local opts=("$@") i sel
  if [ "$ASSUME_YES" = 1 ] || [ ! -t 0 ]; then printf '%s' "$def"; return; fi
  printf '\n%s\n' "$title" >&2
  for i in "${!opts[@]}"; do printf '  %d) %s\n' "$((i + 1))" "${opts[$i]}" >&2; done
  while :; do
    printf '  选择 [%s]: ' "$def" >&2
    IFS= read -r sel || sel="$def"
    sel="${sel:-$def}"
    case "$sel" in
      *[!0-9]*) printf '  请输入数字\n' >&2 ;;
      *) if [ "$sel" -ge 1 ] && [ "$sel" -le "${#opts[@]}" ]; then printf '%s' "$sel"; return; fi
         printf '  超出范围\n' >&2 ;;
    esac
  done
}

ask() { # ask <提示> <默认值> -> 输出回答
  local q="$1" def="${2:-}" ans
  if [ "$ASSUME_YES" = 1 ] || [ ! -t 0 ]; then printf '%s' "$def"; return; fi
  printf '%s' "$q" >&2
  IFS= read -r ans || ans=""
  printf '%s' "${ans:-$def}"
}

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
good() { printf '\033[1;32m  ok\033[0m %s\n' "$*"; }
bad()  { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

printf '\nphprt 安装器  ·  源码: %s\n' "$HERE"
printf '%s\n' '────────────────────────────────────────────────────────'

# ------------------------------------------------------------ 1. 安装根 ----
if [ -n "$PREFIX_OPT" ]; then
  PHPRT_HOME="$PREFIX_OPT"
elif [ "$MODE" = local ]; then
  PHPRT_HOME="$PWD/.phprt"
elif [ "$MODE" = global ]; then
  PHPRT_HOME="$HOME/.phprt"
else
  sel="$(menu "安装到哪儿？" 1 \
    "$HOME/.phprt            全局，所有项目共用" \
    "$PWD/.phprt             只本项目，删目录即卸载" \
    "自定义路径")"
  case "$sel" in
    1) PHPRT_HOME="$HOME/.phprt" ;;
    2) PHPRT_HOME="$PWD/.phprt" ;;
    3) PHPRT_HOME="$(ask '  路径: ' "$HOME/.phprt")" ;;
  esac
fi
case "$PHPRT_HOME" in
  /*) ;;
  *) PHPRT_HOME="$PWD/$PHPRT_HOME" ;;
esac
PHPRT_HOME="${PHPRT_HOME%/}"
command -v realpath >/dev/null 2>&1 && PHPRT_HOME="$(realpath -m "$PHPRT_HOME")"
export PHPRT_HOME

# ------------------------------------------------ 2. 版本清单与选择 ----
mapfile -t V_FILES < <(ls -1 "$HERE"/lib/versions/php-*.conf 2>/dev/null | sort -Vr)
[ "${#V_FILES[@]}" -gt 0 ] || bad "lib/versions/ 里没有版本清单"

V_OPTS=()
for f in "${V_FILES[@]}"; do
  ver="$(sed -n 's/^PHP_VERSION=//p' "$f" | tr -d '"' | head -1)"
  label="$(sed -n 's/^PHP_LABEL=//p' "$f" | tr -d '"' | head -1)"
  mark=""
  [ -x "$PHPRT_HOME/php-$ver/bin/php" ] && mark="  [已安装]"
  V_OPTS+=("$ver    $label$mark")
done

if [ -n "$PHP_OPT" ]; then
  PHP_VERSION="$PHP_OPT"
  else_sel=0
  for i in "${!V_FILES[@]}"; do
    ver="$(sed -n 's/^PHP_VERSION=//p' "${V_FILES[$i]}" | tr -d '"' | head -1)"
    [ "$ver" = "$PHP_VERSION" ] && { else_sel=$((i + 1)); break; }
  done
  [ "$else_sel" != 0 ] || bad "没有 PHP $PHP_OPT 的清单（见 lib/versions/）"
  V_FILE="${V_FILES[$((else_sel - 1))]}"
else
  sel="$(menu "PHP 版本" 1 "${V_OPTS[@]}")"
  V_FILE="${V_FILES[$((sel - 1))]}"
  PHP_VERSION="$(sed -n 's/^PHP_VERSION=//p' "$V_FILE" | tr -d '"' | head -1)"
fi
good "PHP $PHP_VERSION"

# ------------------------------------------------------------ 3. php.ini ----
INI_PRESET="default"
INI_VALUE=""
if [ -n "$INI_OPT" ]; then
  case "$INI_OPT" in
    default|dev|prod) INI_PRESET="$INI_OPT" ;;
    *) INI_PRESET=""; INI_VALUE="$INI_OPT" ;;
  esac
  [ "$INI_LOCAL" = 1 ] && INI_PRESET="${INI_PRESET:-default}"
else
  sel="$(menu "php.ini 用哪个？" 1 \
    "默认（安装根里的 php<版本>_default.ini）" \
    "dev    显示所有错误、关 opcache" \
    "prod   不显示错误、开 opcache" \
    "用已有的 ini 文件（输入路径）" \
    "在项目里新建一份 php.ini（复制 dev 预设，之后自己改）")"
  case "$sel" in
    1) INI_PRESET="default" ;;
    2) INI_PRESET="dev" ;;
    3) INI_PRESET="prod" ;;
    4) INI_PRESET=""; INI_VALUE="$(ask '  ini 路径: ' '')" ;;
    5) INI_PRESET="dev"; INI_LOCAL=1 ;;
  esac
fi

# --------------------------------------------------- 4. 合成 config.conf ----
say "写配置 -> $PHPRT_HOME/config.conf"
mkdir -p "$PHPRT_HOME"
{
  printf '# 由 install.sh 生成：lib/versions/%s + config.conf\n' "$(basename "$V_FILE")"
  cat "$V_FILE"
  printf '\n'
  cat "$HERE/config.conf"
} > "$PHPRT_HOME/config.conf.tmp"
mv -f "$PHPRT_HOME/config.conf.tmp" "$PHPRT_HOME/config.conf"
good "PHP $PHP_VERSION，开关见 $PHPRT_HOME/config.conf"

# ------------------------------------------------------ 5. 预下载包播种 ----
seed_downloads() {
  local src="$HERE/downloads" dst="$PHPRT_HOME/downloads" f b n=0
  [ -d "$src" ] || return 0
  mkdir -p "$dst"
  for f in "$src"/*; do
    [ -f "$f" ] || continue
    b="$(basename "$f")"
    [ "$b" = "SHA256SUMS" ] && continue
    [ "$b" = "README.md" ] && continue
    [ -s "$f" ] || continue
    if [ -f "$dst/$b" ] && [ "$(stat -c %s "$dst/$b")" = "$(stat -c %s "$f")" ]; then
      continue
    fi
    if [ "$(stat -c %d "$f")" = "$(stat -c %d "$dst")" ]; then
      ln -f "$f" "$dst/$b" 2>/dev/null || cp -a "$f" "$dst/$b"
    else
      cp -a "$f" "$dst/$b"
    fi
    n=$((n + 1))
  done
  # 校验清单：本地已有的不覆盖，缺的行补上
  if [ -f "$src/SHA256SUMS" ]; then
    if [ -f "$dst/SHA256SUMS" ]; then
      while IFS= read -r line; do
        [ -n "$line" ] || continue
        grep -qxF "$line" "$dst/SHA256SUMS" || printf '%s\n' "$line" >> "$dst/SHA256SUMS"
      done < "$src/SHA256SUMS"
    else
      cp -a "$src/SHA256SUMS" "$dst/SHA256SUMS"
    fi
  fi
  printf '%s' "$n"
}

if [ -d "$HERE/downloads" ]; then
  n="$(seed_downloads)"
  if [ "$n" -gt 0 ]; then
    good "预下载包：硬链接/复制 $n 个到 $PHPRT_HOME/downloads"
  else
    good "预下载包：已就绪（无需复制）"
  fi
fi

# --------------------------------------------------------------- 6. 编译 ----

BUILD_ARGS=()
[ "$OFFLINE" = 1 ] && BUILD_ARGS+=(--offline)
[ -n "$JOBS_OPT" ] && BUILD_ARGS+=(--jobs "$JOBS_OPT")

INSTALLED=0
[ -x "$PHPRT_HOME/php-$PHP_VERSION/bin/php" ] \
  && [ -f "$PHPRT_HOME/php-$PHP_VERSION/.phprt-stamp-php" ] && INSTALLED=1

DO_BUILD=1
if [ "$INSTALLED" = 1 ] && [ "$FORCE" != 1 ]; then
  say "PHP $PHP_VERSION 已安装（$PHPRT_HOME/php-$PHP_VERSION）"
  ans="$(ask '  要重新编译吗? [y/N]: ' 'N')"
  case "$ans" in
    y|Y|yes|YES) DO_BUILD=1; BUILD_ARGS+=(--force) ;;
    *) DO_BUILD=0 ;;
  esac
fi

if [ "$DO_BUILD" = 1 ]; then
  PHPRT_HOME="$PHPRT_HOME" bash "$HERE/build.sh" ${BUILD_ARGS[@]+"${BUILD_ARGS[@]}"} \
    || bad "编译失败（见 $PHPRT_HOME/logs/）"
else
  good "跳过编译（--force 可强编）"
  [ -x "$PHPRT_HOME/php-$PHP_VERSION/bin/php" ] || bad "PHP 本体丢失: $PHPRT_HOME/php-$PHP_VERSION/bin/php"
fi

# 装完把解压的源码清掉（约占 1G；下载包和编译结果都保留）
if [ "$KEEP_SRC" != 1 ] && [ -d "$PHPRT_HOME/build" ] && [ -n "$(ls -A "$PHPRT_HOME/build" 2>/dev/null)" ]; then
  rm -rf "${PHPRT_HOME:?}/build"/*
  good "已清理源码目录 build/（加 --keep-src 可保留）"
fi

# --------------------------------------------------------- 7. 项目入口 ----
PROJECT="$PWD"
if [ "$NO_APP" = 1 ]; then
  say "跳过项目入口 ./myphp（--no-entry）"
elif [ "$PROJECT" = "$HERE" ]; then
  printf '\n提示: 你现在在 phprt 源码目录里，没有生成 ./myphp。\n'
  printf '      到你的项目目录执行： %s/phprt init\n' "$HERE"
else
  # 安装根在项目内就用相对路径，项目整体搬走也能用
  case "$PHPRT_HOME/" in
    "$PROJECT"/*) ROOT_VALUE="./${PHPRT_HOME#"$PROJECT"/}" ;;
    *)            ROOT_VALUE="$PHPRT_HOME" ;;
  esac

  # 决定 ini：预设 → 安装根里的 ini；项目内新建 → ./php.ini；外部路径 → 原样
  INI_VALUE_FINAL=""
  preset="${INI_PRESET:-default}"
  if [ "$INI_LOCAL" = 1 ]; then
    src="$HERE/ini/$preset.ini"
    [ -f "$src" ] || src="$PHPRT_HOME/php${PHP_VERSION}_${preset}.ini"
    [ -f "$src" ] || bad "找不到预设 ini: $preset"
    cp -f "$src" "$PROJECT/php.ini"
    INI_VALUE_FINAL="./php.ini"
    good "项目 php.ini -> $PROJECT/php.ini（来自 $preset）"
  elif [ -n "$INI_VALUE" ]; then
    [ -f "$INI_VALUE" ] || bad "ini 文件不存在: $INI_VALUE"
    INI_VALUE_FINAL="$INI_VALUE"
  else
    INI_VALUE_FINAL="$PHPRT_HOME/php${PHP_VERSION}_${preset}.ini"
    [ -f "$INI_VALUE_FINAL" ] || bad "缺少 $INI_VALUE_FINAL（先跑一次 build）"
  fi

  # ini 落在项目里就写相对路径（和 PHPRT_ROOT 保持一致），项目整体搬走也能用
  case "$INI_VALUE_FINAL/" in
    "$PROJECT"/*) INI_VALUE_FINAL="./${INI_VALUE_FINAL#"$PROJECT"/}" ;;
  esac

  APP="$PROJECT/myphp"
  if [ -L "$APP" ]; then
    # phprt link 建的软链：直接换成生成的脚本
    rm -f "$APP"
    good "把原来的 ./myphp 软链换成本项目的生成脚本"
  elif [ -f "$APP" ] && ! head -3 "$APP" | grep -q '由 phprt 生成'; then
    [ "$FORCE" = 1 ] || bad "$APP 已存在且不是 phprt 生成的（加 --force 覆盖）"
  fi

  tpl="$(cat "$HERE/lib/myphp-project.tmpl")"
  tpl="${tpl//@PHPRT_ROOT@/$ROOT_VALUE}"
  tpl="${tpl//@PHP_VERSION@/$PHP_VERSION}"
  tpl="${tpl//@PHP_INI@/$INI_VALUE_FINAL}"
  tpl="${tpl//@PHP_ARGS@/}"
  printf '%s\n' "$tpl" > "$APP.tmp"
  chmod +x "$APP.tmp"
  mv -f "$APP.tmp" "$APP"
  good "项目入口 -> $APP"
fi

# ------------------------------------------------------------- 8. 自检 ----
say "自检"
if [ -f "$PROJECT/myphp" ]; then
  ( cd "$PROJECT" && ./myphp -r 'echo PHP_VERSION, "\n";' > /dev/null ) \
    || bad "./myphp 跑不起来"
  ( cd "$PROJECT" && ./myphp -v | head -1 | sed 's/^/    /' )
  ext_count="$( cd "$PROJECT" && ./myphp -m | grep -c . || true )"
  printf '    扩展 %s 个\n' "$ext_count"
  ( cd "$PROJECT" && ./myphp -r 'echo "    ini=", php_ini_loaded_file(), "\n";' )
else
  bash "$HERE/phprt" exec --php "$PHP_VERSION" -- -v | head -1 | sed 's/^/    /'
fi

printf '\n\033[1;32m完成\033[0m\n'
printf '  安装根:  %s\n' "$PHPRT_HOME"
[ -f "$PROJECT/myphp" ] && printf '  项目入口: %s/myphp\n' "$PROJECT"
printf '\n  下一步:  ./myphp -v\n'
printf '  管理:    %s/phprt list\n' "$HERE"
printf '\n'
