#!/usr/bin/env bash
# phprt：命令实现

usage() {
  cat <<'EOF'
phprt —— 为项目安装一套隔离的 PHP 运行环境

用法:
  phprt install <系列|版本> [选项]
      系列写法（8.5）用 bundle.conf 的 pin；精确写法（8.5.12）装指定版本。

      扩展选择:
      --ext "a b"          选择扩展；不写时终端里用菜单交互选择
      --opt "gd=jpeg,webp" 设置扩展的子选项；需要和 --ext 一起用
                           未列出的非 fixed 子项关闭，fixed 子项始终开启

      编译方式:
      --build "tidy=static,curl=shared"
                           逐项指定；优先级高于 --static/--shared
      --static             所有支持 static 的扩展都编入 PHP；
                           type=shared 的（如 xdebug）保持 shared 并提示
      --shared             所有支持 shared 的扩展都编成 .so；
                           type=static 的（如 libxml2）保持 static 并提示
      --static-all         扩展和第三方依赖都静态链接进 bin/php；
                           type=shared 的扩展（如 xdebug）自动排除。
                           glibc 仍动态，产物可单独复制运行。
                           当前支持：openssl zlib curl sqlite3 mbstring
      --static-all-lto     在 --static-all 基础上给 PHP 本体和依赖都开 LTO；
                           状态记录在 .phprt-state/lto。
      不指定时按 recipe 的 default=（builtin 默认 static，PECL 默认 shared）。

      其他:
      --plan               只打印构建计划（依赖、PHP、static/shared 分组），不构建
      --downloads          只把源码下载到 .phprt/downloads，不构建；
                           不写 --ext 时先进入选择界面，选完再下载
                           （--download / --predownload 同义）
      --downloads-print-urls
                           只打印源码 URL，不下载、不构建
      --download-jobs N    全局下载并发数（默认 10）
      --download-host-jobs N
                           同一站点并发数（默认 2）
      --timeout N          单个文件最长下载秒数（默认 900）
      --proxy URL          本次下载使用的代理
      --ui auto|plain|progress
                           下载界面；默认终端里用进度框
      --offline            只用 .phprt/downloads，不联网
      --recompile          强制重编 PHP（依赖和扩展签名保留）
      --clean-src          清空已解压源码，重新解压后再编
      --yes                对提示一律回答“是”

      构建环境:
      --chroot             在 phprt buildroot 的 chroot 里构建（并记为默认）
      --host               切回宿主构建（并记为默认）
      --plan               只打印构建计划，不进入 chroot

  phprt download <系列|版本> [--ext "a b"] [--print-urls]
      只把构建需要的源码包下载到 <项目>/.phprt/downloads。
      不编译，不创建版本树和构建目录。扩展默认取 DEFAULT_EXTS。

      --ext "a b"             选择扩展；会自动带上依赖和构建工具
      --print-urls            只打印实际缓存文件名和 URL（#deps-名字 文件）
      --download-jobs N       全局下载并发数（默认 10）
      --download-host-jobs N
                              同一站点并发数（默认 2）
      --timeout N             单个文件最长下载秒数（默认 900）
      --proxy URL             本次下载使用的代理
      --ui auto|plain|progress
                              下载界面；默认终端里用进度框
      文件已存在会跳过；中断后再次运行会从断点续传。

  phprt update <依赖> [版本]
      依赖库的同系列安全更新（如 openssl 3.5.9 -> 3.5.10）。
      省略版本时查询上游最新安全版本；--dry-run 只打印计划。
      只更新一个依赖；受影响的 PHP/扩展按签名重建。
      --downloads          只下载目标版本的源码，不修改配置、不构建
                           （--download / --predownload 同义）
      --downloads-print-urls
                           只打印目标源码 URL
      --download-jobs N / --download-host-jobs N / --timeout N / --proxy URL
                           全局并发、单站点并发、单文件超时、代理

      构建环境:
      --chroot / --host    指定构建环境（并记为默认）

  phprt add ext <名字> --shared|--static [--opt "gd=jpeg,webp"] [--downloads|--downloads-print-urls]
      在当前版本树里加一个扩展。必须先读契约再显式选方式：

        --shared   编译成 <名字>.so，不重编 PHP，可随时停用
        --static   编入 PHP 二进制，需要重新编译 PHP，不能单独停用

      契约里 type=shared 的（如 xdebug）只能 --shared；
      type=static 的（如 libxml2）只能 --static。
      已经是同一种方式、又没带 --opt 时直接跳过；带 --opt 会按新子选项重编。
      切到另一种方式同样会重编 PHP。
      --opt 只改点名的扩展，树里其它扩展的子选项保持不变。
      不带参数会打印契约支持的方式并退出。

  phprt remove ext <名字>
      停用 shared 扩展（删对应的 conf.d ini）。
      static 扩展需要重编 PHP 才能去掉，命令只提示。

  phprt use <系列|版本>
      切换 current 软链。系列写法切到该系列已装的最高版本。

  phprt buildroot <create|enter|exec|unmount|remove|status|list|use|env>
      chroot 构建环境；rootfs 默认在 <项目>/.phprt/buildroots/。
      详见 phprt buildroot --help。

  phprt show [--tree [扩展]]
      显示当前 PHP 版本、安装路径、php.ini、已启用扩展（含 static/shared）。
      --tree        额外打印 PHP 本体和每个扩展的运行时库依赖树
      --tree <名>   只看一个扩展的依赖树

  phprt current
      只打印当前版本号。

  phprt list
      列出已安装的版本，当前版本前面标 *。

  phprt select [系列|版本]
      交互选择要编译的扩展和子选项。↑↓ 移动，Home/End 跳转，Space 切换选中，
      Enter 展开/收起子项（在最后一行「确认并继续」上回车完成）。结果打印到 stdout：
      扩展: ... / 子选项: ...
      --default 不进入菜单，直接打印 DEFAULT_EXTS。
      非终端时也走 --default 的行为。

  phprt ext list [系列] [选项]
      列出某个 PHP 系列的全部可用扩展，不要求已经安装。
      不写系列时用当前版本；没有 current 且仓库只有一个 bundle 时自动用它。

      --php <系列>      指定系列，如 --php 8.5
      --default         只看 DEFAULT_EXTS
      --installed       只看当前树已启用的扩展
      --builtin         只看 builtin
      --pecl            只看 PECL
      --names           只输出扩展名，一行一个
      --json            输出 JSON

  phprt outdated [--all] [--json] [系列]
      检查上游版本。默认只查当前 php.ini 启用的扩展及其依赖；
      --all 查全部已接入模块；--json 输出机器可读结果。
      退出码：0 查询成功，3 网络或上游不可用。

  phprt verify
      自检：PHP、扩展、动态库、RPATH、配置路径、CA、wheel 完整性。

contract（recipe 头部）:
  type=static|shared|static,shared   支持哪些编译方式
  default=static|shared               install 不带 --build/--static/--shared 时的默认
  default 必须出现在 type 里。

环境变量:
  PHPRT_REPO      phprt 仓库位置（默认是脚本所在仓库）
  PHPRT_WORK      构建区基目录（默认 <项目>/.phprt）
  OFFLINE=1       不联网（outdated 会直接报告未查询）
  https_proxy / HTTPS_PROXY / http_proxy / HTTP_PROXY
                  查询和下载使用的代理；未设置时直连
  PHPRT_ARIA2=0   禁用 aria2c，强制用 curl
  PHPRT_ARIA2_CONNECTIONS=4
                  每个文件用 aria2c 下载时的连接数
  PHPRT_SELECT=0  禁用交互菜单；TTY 下退回简单的文本提示

扩展清单由 bundles/php<系列>/{builtin,ext}/*.sh 决定；新增一个 recipe
文件就会出现在可选清单里，不需要改 driver。

示例:
  phprt install 8.5
  phprt install 8.5.12
  phprt install 8.5 --ext "openssl curl zip swoole"
  phprt install 8.5 --ext "gd" --opt gd=jpeg,png,webp,freetype
  phprt install 8.5 --ext "tidy curl" --build "tidy=static,curl=shared"
  phprt install 8.5 --ext "tidy redis xdebug" --static
  phprt install 8.5 --static-all --ext "openssl zlib curl sqlite3 mbstring"
  phprt install 8.5 --static-all-lto --ext "openssl zlib curl sqlite3 mbstring"
  phprt add ext tidy --shared
  phprt add ext gmp --static
  phprt add ext redis --shared
  phprt update openssl
  phprt update openssl 3.5.10 --dry-run
  phprt outdated --all --json
  phprt use 8.5
  phprt ext list 8.5
  phprt ext list --php 8.5 --names
  phprt show --tree curl
  phprt buildroot create
  phprt install 8.5 --chroot --ext "curl redis"
EOF
}

repo_root() {
  printf '%s\n' "${PHPRT_REPO:-}"
}

# 取系列 bundle（bundles/php<系列>，如 php8.5）。recipe 以 phprt 仓库为准；
# 项目里不再保存 runner/bundle 副本。旧项目残留的副本只读兼容，不刷新。
# 精确版与 pin 不同时不复制 bundle：build.sh 读完 bundle.conf 后用
# apply_php_override 换掉 PHP 自身的版本信息。
bundle_for() {
  local series="$1" repo dst
  repo="$(repo_root)"
  if [ -n "$repo" ] && [ -d "$repo/bundles/php$series" ]; then
    printf '%s\n' "$repo/bundles/php$series"
    return 0
  fi
  dst="$PHPRT_HOME/bundles/php$series"
  if [ -d "$dst" ]; then
    warn "使用项目内旧 bundle 副本: $dst（建议从 phprt 仓库运行）"
    printf '%s\n' "$dst"
    return 0
  fi
  return 1
}

normalize_exts() {
  printf '%s' "$1" | tr ',' ' ' | tr -s ' '
}

# 只准备源码，不编译。参数与 install 的下载部分保持一致。
cmd_download() {
  local arg="" exts="" print_urls=0 ext_given=0 opt_spec=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --php)       arg="${2:-}"; shift 2 ;;
      --php=*)     arg="${1#*=}"; shift ;;
      --ext)       exts="${2:-}"; ext_given=1; shift 2 ;;
      --ext=*)     exts="${1#*=}"; ext_given=1; shift ;;
      --opt)       opt_spec="${opt_spec:+$opt_spec }${2:-}"; shift 2 ;;
      --opt=*)     opt_spec="${opt_spec:+$opt_spec }${1#*=}"; shift ;;
      --print-urls|--urls) print_urls=1; shift ;;
      --download-jobs) export PHPRT_DOWNLOAD_JOBS="${2:-}"; shift 2 ;;
      --download-jobs=*) export PHPRT_DOWNLOAD_JOBS="${1#*=}"; shift ;;
      --download-host-jobs) export PHPRT_DOWNLOAD_HOST_JOBS="${2:-}"; shift 2 ;;
      --download-host-jobs=*) export PHPRT_DOWNLOAD_HOST_JOBS="${1#*=}"; shift ;;
      --timeout) export PHPRT_DOWNLOAD_TIMEOUT="${2:-}"; shift 2 ;;
      --timeout=*) export PHPRT_DOWNLOAD_TIMEOUT="${1#*=}"; shift ;;
      --ui) export PHPRT_UI="${2:-}"; shift 2 ;;
      --ui=*) export PHPRT_UI="${1#*=}"; shift ;;
      --proxy)     export https_proxy="${2:-}" http_proxy="${2:-}" HTTPS_PROXY="${2:-}" HTTP_PROXY="${2:-}"; shift 2 ;;
      --proxy=*)   export https_proxy="${1#*=}" http_proxy="${1#*=}" HTTPS_PROXY="${1#*=}" HTTP_PROXY="${1#*=}"; shift ;;
      -h|--help)
        printf '用法: phprt download <系列|版本> [--ext "a b"] [--opt "gd=jpeg,webp"] [--print-urls] [--download-jobs N] [--download-host-jobs N] [--timeout N] [--proxy URL]\n'
        return 0 ;;
      -*) die "未知参数: $1" ;;
      *)
        if [ -z "$arg" ]; then arg="$1"; else die "多余参数: $1"; fi
        shift ;;
    esac
  done
  [ -n "$arg" ] || die "用法: phprt download <系列|版本>（如 download 8.5）"

  local exact=0 series
  case "$arg" in
    *.*.*) exact=1 ;;
  esac
  if [ "$exact" = 1 ]; then
    printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-dev)?$' ||
      die "版本号格式不对: $arg（精确版如 8.5.12）"
    series="$(series_of "$arg")"
  else
    printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+$' ||
      die "系列格式不对: $arg（应为 major.minor，如 8.5）"
    series="$arg"
  fi

  local bundle
  bundle="$(bundle_for "$series")" ||
    die "找不到 bundle: php$series（需要 phprt 仓库或 PHPRT_REPO）"
  export BUNDLE="$bundle"
  load_bundle_conf "$bundle" "$series"

  if [ "$exact" = 1 ]; then
    export PHPRT_PHP_VERSION="$arg"
    apply_php_override
  fi

  DOWNLOADS="${PHPRT_DOWNLOADS:-$PHPRT_HOME/downloads}"
  export DOWNLOADS

  if [ -z "$exts" ]; then
    exts="${DEFAULT_EXTS:-}"
  fi
  exts="$(normalize_exts "$exts")"
  if [ -n "$opt_spec" ]; then
    [ "$ext_given" = 1 ] || die "--opt 需要和 --ext 一起用"
    validate_ext_opts "$exts" "$opt_spec"
    export PHPRT_EXT_OPTS="$opt_spec"
  fi
  load_tree_ext_opts "$(tree_of "$PHP_VERSION")"

  local e mode_env=""
  for e in $exts; do
    ext_recipe_sh "$e" >/dev/null 2>&1 || die "未知扩展: $e$(ext_name_hint "$e")"
    mode_env+="${mode_env:+ }$e=$(recipe_default_mode "$(ext_recipe_sh "$e")")"
  done
  export PHPRT_EXT_MODES="$mode_env" SELECTED_EXTS="$exts"

  if [ "$print_urls" = 1 ]; then
    source_plan "$exts"
    source_plan_print_urls
    return 0
  fi

  prefetch_sources "$exts" 1
}

# 安装 / 重建 PHP。
# 系列写法（8.5）自动解析到该系列最新小版本：非离线先查 php.net，
# 本地已有不旧于目标的树就停止；离线或查不到用 bundle.conf 的 pin。
# 精确写法（8.5.12）直接装指定版本。
# 解析 install 的编译方式。
# 优先级：--build name=mode > --static/--shared > 契约 default
# 输出： "tidy=static curl=shared"
resolve_install_modes() {
  local exts="$1" spec="${2:-}" global="${3:-}" tree="${4:-}"
  local item n m e sh types dflt out="" key actual
  local -A ov=()

  local -a items=()
  IFS=',' read -r -a items <<< "$spec"
  for item in "${items[@]}"; do
    [ -n "$item" ] || continue
    case "$item" in
      *=*) n="${item%%=*}"; m="${item#*=}" ;;
      *) die "--build 格式应为 名字=static|shared: $item" ;;
    esac
    case " $exts " in
      *" $n "*) ;;
      *) die "--build 的 $n 不在 --ext 列表里" ;;
    esac
    case "$m" in
      static|shared) ;;
      *) die "--build $n=$m 不合法（static 或 shared）" ;;
    esac
    ov[$n]="$m"
  done

  for e in $exts; do
    sh="$(ext_recipe_sh "$e")" || continue
    types="$(recipe_types "$sh")"
    dflt="$(recipe_default_mode "$sh")"
    m=""
    if [ -n "${ov[$e]:-}" ]; then
      m="${ov[$e]}"
      mode_supported "$sh" "$m" || die "$e 不支持 $m（type=$types）"
    elif [ -n "$global" ]; then
      if mode_supported "$sh" "$global"; then
        m="$global"
      else
        m="$dflt"
        warn "$e 不支持 $global，按 $m 编译（type=$types）"
      fi
    else
      # 已装树优先沿用实际方式，避免契约 default 改了就把老树悄悄切过去
      actual=""
      [ -n "$tree" ] && actual="$(ext_current_mode "$tree" "$e" 2>/dev/null || true)"
      if [ -n "$actual" ] && ! mode_supported "$sh" "$actual"; then
        actual=""
      fi
      m="${actual:-$dflt}"
    fi
    out="$out${out:+ }$e=$m"
  done
  printf '%s\n' "$out"
}

# 树是否构建完成：有 ok 状态；老版本没有状态文件时按已有 bin/php 视为完成
tree_build_complete() {  # $1=tree -> 0/1
  local tree="$1" status=""
  [ -d "$tree" ] || return 1
  status="$(cat "$tree/BUILD-STATUS" 2>/dev/null || true)"
  if [ -x "$tree/bin/php" ] && {
       [ "$status" = ok ] ||
       { [ ! -f "$tree/BUILD-STATUS" ] && [ ! -f "$tree/.build-exts" ]; }
     }; then
    return 0
  fi
  return 1
}

# 工具链预检：缺什么一次性报完，别让用户选完扩展、下完源码才发现
# $1=full（编译全套）| download（只下载）
preflight_cmds() {
  local c missing=""
  if [ "${1:-full}" = download ]; then
    for c in curl; do
      command -v "$c" >/dev/null 2>&1 || missing="$missing $c"
    done
    [ -z "$missing" ] || die "缺少命令:${missing}（下载需要 curl；装 aria2 可加速）"
    return 0
  fi
  for c in gcc g++ make cmake pkg-config patchelf tar xz unzip ar m4 perl curl; do
    command -v "$c" >/dev/null 2>&1 || missing="$missing $c"
  done
  [ -z "$missing" ] ||
    die "缺少命令:${missing}（安装: sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config patchelf tar xz unzip binutils m4 perl curl）"
}

cmd_install() {
  local arg="" exts="" recompile=0 assume_yes=0 plan=0 clean_src=0
  local build_spec="" global_mode="" static_all=0 lto=0 saw_shared=0
  local chroot_mode="" env_set=0
  local downloads_only=0 print_urls=0 offline=0
  local ext_given=0 opt_spec=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --offline)   offline=1; shift ;;
      --ext)       exts="${2:-}"; ext_given=1; shift 2 ;;
      --ext=*)     exts="${1#*=}"; ext_given=1; shift ;;
      --opt)       opt_spec="${opt_spec:+$opt_spec }${2:-}"; shift 2 ;;
      --opt=*)     opt_spec="${opt_spec:+$opt_spec }${1#*=}"; shift ;;
      --build)     build_spec="${build_spec:+$build_spec,}${2:-}"; shift 2 ;;
      --build=*)   build_spec="${build_spec:+$build_spec,}${1#*=}"; shift ;;
      --static)    global_mode=static; shift ;;
      --static-all) static_all=1; lto=0; global_mode=static; shift ;;
      --static-all-lto) static_all=1; lto=1; global_mode=static; shift ;;
      --shared)    global_mode=shared; saw_shared=1; shift ;;
      --downloads|--download|--predownload) downloads_only=1; shift ;;
      --downloads-print-urls|--print-urls|--urls) print_urls=1; shift ;;
      --download-jobs) export PHPRT_DOWNLOAD_JOBS="${2:-}"; shift 2 ;;
      --download-jobs=*) export PHPRT_DOWNLOAD_JOBS="${1#*=}"; shift ;;
      --download-host-jobs) export PHPRT_DOWNLOAD_HOST_JOBS="${2:-}"; shift 2 ;;
      --download-host-jobs=*) export PHPRT_DOWNLOAD_HOST_JOBS="${1#*=}"; shift ;;
      --timeout) export PHPRT_DOWNLOAD_TIMEOUT="${2:-}"; shift 2 ;;
      --timeout=*) export PHPRT_DOWNLOAD_TIMEOUT="${1#*=}"; shift ;;
      --ui) export PHPRT_UI="${2:-}"; shift 2 ;;
      --ui=*) export PHPRT_UI="${1#*=}"; shift ;;
      --proxy)     export https_proxy="${2:-}" http_proxy="${2:-}" HTTPS_PROXY="${2:-}" HTTP_PROXY="${2:-}"; shift 2 ;;
      --proxy=*)   export https_proxy="${1#*=}" http_proxy="${1#*=}" HTTPS_PROXY="${1#*=}" HTTP_PROXY="${1#*=}"; shift ;;
      --chroot)    chroot_mode=chroot; env_set=1; shift ;;
      --host)      chroot_mode=host; env_set=1; shift ;;
      --recompile) recompile=1; shift ;;
      --plan)      plan=1; shift ;;
      --clean-src) clean_src=1; shift ;;
      --yes|-y)    assume_yes=1; shift ;;
      -h|--help)   usage; return 0 ;;
      -*)          die "未知参数: $1" ;;
      *)           arg="$1"; shift ;;
    esac
  done
  [ -n "$arg" ] || die "用法: phprt install <系列|版本>（如 install 8.5 / install 8.5.12）"
  [ "$offline" = 1 ] && export OFFLINE=1
  [ "$env_set" = 1 ] && buildroot_set_env "$chroot_mode"

  if [ "$static_all" = 1 ]; then
    [ "$saw_shared" != 1 ] || die "--static-all / --static-all-lto 和 --shared 不能同时用"
    case ",$build_spec," in
      *,*=shared,*) die "全静态模式下不能有 --build 名字=shared" ;;
    esac
  fi

  local exact=0
  case "$arg" in
    *.*.*) exact=1 ;;
  esac
  local series
  if [ "$exact" = 1 ]; then
    printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-dev)?$' ||
      die "版本号格式不对: $arg（精确版如 8.5.12）"
    series="$(series_of "$arg")"
  else
    printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+$' ||
      die "系列格式不对: $arg（应为 major.minor，如 8.5）"
    series="$arg"
  fi

  local bundle pin concrete imax
  bundle="$(bundle_for "$series")" ||
    die "找不到 bundle: php$series（需要 phprt 仓库或 PHPRT_REPO）"

  # shellcheck disable=SC1090
  load_bundle_conf "$bundle" "$series"
  export BUNDLE="$bundle"
  pin="$PHP_VERSION"

  # 系列写法直接用 pin（不联网查 php.net）；精确写法装指定版本。
  # 精确版的版本号通过环境变量传给 build.sh / recipe，不复制 bundle 目录。
  concrete="$pin"
  [ "$exact" = 1 ] && concrete="$arg"
  export PHPRT_PHP_VERSION="$concrete"
  apply_php_override
  phprt_set_build_root "$concrete"

  if [ "$exact" != 1 ]; then
    # 本地已有不旧于目标的同系列树：停止（不降级、不重复装）
    # --recompile / --plan / --ext 是明确的构建意图，不拦
    imax="$(installed_series_max "$series")"
    if [ -n "$imax" ] &&
       [ "$(printf '%s\n%s\n' "$imax" "$concrete" | sort -V | tail -1)" = "$imax" ] &&
       tree_build_complete "$(tree_of "$imax")" &&
       [ "$recompile" != 1 ] && [ "$plan" != 1 ] && [ -z "$exts" ] &&
       [ "$static_all" != 1 ] && [ -z "$build_spec" ] && [ -z "$global_mode" ] &&
       [ "$downloads_only" != 1 ] && [ "$print_urls" != 1 ]; then
      if [ "$imax" = "$concrete" ]; then
        ok "PHP $concrete 已安装，无需重复安装"
        printf '    重新编译: phprt install %s --recompile\n' "$concrete"
      else
        ok "本地已装 PHP $imax（目标 $concrete 更旧），不降级"
      fi
      [ "$imax" = "$(current_version 2>/dev/null || true)" ] ||
        printf '    切换: phprt use %s\n' "$series"
      return 0
    fi
  fi

  local tree complete=0
  tree="$(tree_of "$concrete")"

  # 工具链预检：选扩展 / 下载之前就把缺的命令一次性报出来
  if [ "$plan" = 1 ] || [ "$print_urls" = 1 ]; then
    :  # 只打印计划 / 链接，不碰工具链
  elif [ "$downloads_only" = 1 ]; then
    preflight_cmds download
  else
    preflight_cmds full
  fi

  # 新树必须看到 ok；老版本没有状态文件时按已有 bin/php 视为完成
  if tree_build_complete "$tree"; then
    complete=1
  fi

  # 精确版已装齐：停止（系列版走到这里说明目标高于已装最高版，不会重入）
  if [ "$complete" = 1 ] && [ "$exact" = 1 ] &&
     [ "$recompile" != 1 ] && [ "$plan" != 1 ] && [ -z "$exts" ] &&
     [ "$static_all" != 1 ] && [ -z "$build_spec" ] && [ -z "$global_mode" ] &&
     [ "$downloads_only" != 1 ] && [ "$print_urls" != 1 ]; then
    ok "PHP $concrete 已安装"
    printf '    重新编译: phprt install %s --recompile\n' "$concrete"
    [ "$concrete" = "$(current_version 2>/dev/null || true)" ] ||
      printf '    切换: phprt use %s\n' "$concrete"
    return 0
  fi

  [ -d "$tree" ] && [ "$complete" != 1 ] &&
    warn "上次构建未完成，继续；已完成的依赖和 PHP 会跳过"

  if [ -z "$exts" ]; then
    if [ -f "$tree/BUILD-STATUS" ] && [ "$(cat "$tree/BUILD-STATUS")" != ok ] &&
       [ -f "$tree/.build-exts" ]; then
      # 上次没装完：沿用那次的选择
      exts="$(cat "$tree/.build-exts")"
    else
      # 默认沿用当前版本实际启用的扩展（问 PHP，不看状态文件）
      local cur
      cur="$(current_version 2>/dev/null || true)"
      [ -n "$cur" ] && exts="$(tree_exts "$cur")"
    fi
    if [ -z "$exts" ] && [ -f "$tree/BUILD-PLAN" ]; then
      exts="$(sed -n 's/^EXTENSIONS="\(.*\)"$/\1/p' "$tree/BUILD-PLAN" | head -1)"
    fi
    if [ -z "$exts" ] && [ -t 0 ] && [ "$assume_yes" != 1 ] &&
       [ "$downloads_only" != 1 ] && [ "$print_urls" != 1 ] &&
       ! ui_select_available; then
      printf '\n可选扩展: %s\n默认: %s\n' \
        "$(BUNDLE="$bundle" all_ext_names | paste -sd' ' -)" "${DEFAULT_EXTS:-}"
      local ans=""
      read -r -p "编译哪些扩展 [${DEFAULT_EXTS:-}]: " ans || ans=""
      exts="${ans:-${DEFAULT_EXTS:-}}"
    fi
    [ -n "$exts" ] || exts="${DEFAULT_EXTS:-}"
  fi

  # 没写 --ext 时用内置菜单交互选择（含 --downloads：先选扩展，再下载）；
  # --yes / 非终端 / --print-urls 保持原来的默认扩展行为，避免脚本和 CI
  # 卡在菜单上。
  if [ "$ext_given" != 1 ] && [ "$assume_yes" != 1 ] &&
     [ "$print_urls" != 1 ] && ui_select_available; then
    local init_opts=""
    [ -f "$tree/.build-opts" ] && init_opts="$(tr '\n' ' ' < "$tree/.build-opts")"
    init_opts="${init_opts% }"
    if ui_ext_select "$series" "$concrete" "$tree" "${exts:-${DEFAULT_EXTS:-}}" "$init_opts"; then
      exts="$UI_SELECTED_EXTS"
      PHPRT_EXT_OPTS="$UI_SELECTED_OPTS"
      export PHPRT_EXT_OPTS
    else
      printf '已取消\n' >&2
      return 130
    fi
  fi

  if [ "$exts" = "none" ]; then
    exts=""
  fi
  exts="$(normalize_exts "$exts")"

  if [ -n "$opt_spec" ]; then
    [ "$ext_given" = 1 ] ||
      die "--opt 需要和 --ext 一起用（交互选择时直接在菜单里改子选项）"
    validate_ext_opts "$exts" "$opt_spec"
    export PHPRT_EXT_OPTS="$opt_spec"
  fi
  load_tree_ext_opts "$tree"

  # --static-all 只编契约里支持 static 的扩展；shared-only 直接排除。
  if [ "$static_all" = 1 ] && [ -n "$exts" ]; then
    local kept="" _e _sh
    for _e in $exts; do
      _sh="$(ext_recipe_sh "$_e")" || die "未知扩展: $_e"
      if mode_supported "$_sh" static; then
        kept="$kept${kept:+ }$_e"
      else
        warn "$_e 只支持 shared，--static-all 下已排除（type=$(recipe_types "$_sh")）"
      fi
    done
    exts="$kept"
  fi

  # bundle 声明的最小必需扩展（REQUIRED_EXTS，可选）
  local req
  for req in ${REQUIRED_EXTS:-}; do
    case " $exts " in
      *" $req "*) ;;
      *) die "PHP $concrete 必须包含扩展 $req（bundle.conf 的 REQUIRED_EXTS）" ;;
    esac
  done

  # 链接方式只在显式指定时覆盖；不指定就沿用树上记录（新树默认 shared）
  if [ "$static_all" = 1 ]; then
    export PHPRT_LINK_MODE=static
    # --static-all 显式关 LTO，--static-all-lto 显式开；两者共用同一状态文件
    export PHPRT_LTO="$lto"
  elif [ "$global_mode" = shared ]; then
    export PHPRT_LINK_MODE=shared
    export PHPRT_LTO=0
  else
    unset PHPRT_LINK_MODE PHPRT_LTO
  fi

  # 编译方式：--build > --static/--shared > 契约 default
  local mode_env=""
  mode_env="$(resolve_install_modes "$exts" "$build_spec" "$global_mode" "$tree")"
  export PHPRT_EXT_MODES="$mode_env" SELECTED_EXTS="$exts"

  # 下载与构建分开：先把源码备齐，构建阶段不再联网。
  if [ "$print_urls" = 1 ]; then
    source_plan "$exts"
    source_plan_print_urls
    return 0
  fi
  if [ "$downloads_only" = 1 ]; then
    prefetch_sources "$exts" 0 || return 1
    # 离线安装是可闭环的：把这次实际生效的选择打印成可直接保存的命令
    local hint="phprt install $concrete"
    if [ -n "$exts" ]; then
      hint="$hint --ext \"$exts\""
    fi
    if [ -n "${PHPRT_EXT_OPTS:-}" ]; then
      hint="$hint --opt \"$PHPRT_EXT_OPTS\""
    fi
    hint="$hint --offline"
    printf '    源码已下载到 .phprt/downloads\n'
    printf '    下次安装（离线）: %s\n' "$hint"
    return 0
  fi
  if [ "$plan" != 1 ]; then
    prefetch_sources "$exts" 0 || die "源码下载失败；未进入构建"
  fi

  # 明确的 --recompile 只强制重编 PHP，依赖和扩展签名保留
  local force_php=0
  [ "$recompile" = 1 ] && force_php=1

  log "构建 PHP $concrete"
  printf '    扩展: %s\n' "${exts:-（无）}"

  local build_bundle="$bundle"

  if [ "$plan" = 1 ]; then
    SELECTED_EXTS="$exts" \
    PHPRT_SEED="${PHPRT_SEED:-}" \
    PHPRT_PLAN=1 \
      bash "$PHPRT_RUNNER/build.sh" \
        --bundle "$build_bundle" \
        --tree "$tree" \
        --exts "$exts" --plan
    return 0
  fi

  # 构建锁：和 add 互斥；锁放在对应版本的 build-php-<版本> 里
  need_cmds flock
  ensure_base_files
  exec 9>"$BUILD_ROOT/.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"

  if ! SELECTED_EXTS="$exts" \
       PHPRT_SEED="${PHPRT_SEED:-}" \
       PHPRT_FORCE_PHP="$force_php" \
       PHPRT_CLEAN_SRC="$clean_src" \
       PHPRT_NO_DOWNLOAD=1 \
       run_build_env bash "$PHPRT_RUNNER/build.sh" \
         --bundle "$build_bundle" \
         --tree "$tree" \
         --exts "$exts"; then
    die "构建失败；已完成的依赖、PHP 和扩展会保留，重试会从失败处继续"
  fi

  if [ ! -x "$tree/bin/php" ]; then
    die "构建结束但找不到 $tree/bin/php"
  fi

  # build.sh 已经把状态写成 ok；启用 ini / 生成 myphp 还没做。
  # 先改成 activating，这一步失败时下次 install 会重新启用而不是直接跳过。
  printf 'activating\n' > "$tree/BUILD-STATUS"

  # 记录本次选择的编译方式：后续 install/add 不再重复询问
  local pair
  for pair in $mode_env; do
    TREE="$tree" mode_set "${pair%%=*}" "${pair#*=}"
  done
  if [ -n "${PHPRT_LINK_MODE:-}" ]; then
    link_mode_set "$PHPRT_LINK_MODE" "$tree"
  else
    link_mode_set "$(link_mode_get "$tree" 2>/dev/null || printf shared)" "$tree"
  fi
  if [ -n "${PHPRT_LTO:-}" ]; then
    lto_mode_set "$PHPRT_LTO" "$tree"
  else
    lto_mode_set "$(lto_mode_get "$tree" 2>/dev/null || printf 0)" "$tree"
  fi

  activate_tree "$concrete" "$exts"
  printf 'ok\n' > "$tree/BUILD-STATUS"
  ok "PHP $concrete 就绪 -> $tree"
  printf '    使用示例: ./myphp -r '\''var_dump(phpinfo());'\''\n'
  if [ "$PWD" != "$PHPRT_PROJECT" ]; then
    printf '    项目目录: %s\n' "$PHPRT_PROJECT"
  fi
}

# ---- phprt update：依赖库的同系列安全更新 --------------------------------

# 当前树里某个依赖的实际安装版本（deps/<名>-<版本>）
installed_dep_version() {
  local tree="$1" dep="$2" d v found=""
  for d in "$tree/deps/$dep"-*; do
    [ -d "$d" ] || continue
    v="${d##*/}"
    v="${v#"$dep"-}"
    [ -n "$v" ] || continue
    if [ -n "$found" ]; then
      printf '错误: %s 有多个版本目录（%s / %s），先清理 %s/deps/\n' \
        "$dep" "$found" "$v" "$tree" >&2
      return 1
    fi
    found="$v"
  done
  [ -n "$found" ] || return 1
  printf '%s\n' "$found"
}

# 下载新版本源码包（此时配置里的 SHA 还是旧版本，不能按新版本校验）。
# 续传、代理和失败报告统一走 sources.sh。
download_update_source() {
  local url="$1" dest="$2" name="$3" ver="$4" label="${5:-deps}"
  mkdir -p "$DOWNLOADS"
  if source_cached_ok "$dest" "" "$url"; then
    ok "[$label] $name $ver  已存在"
    return 0
  fi
  log "[$label] $name $ver  下载中"
  printf '      %s\n' "$url"
  if ! source_curl_download "$url" "$dest"; then
    warn "[$label] $name $ver 失败（文件保留，下次运行可继续）"
    printf '    %s\n' "$url" >&2
    return 1
  fi
  if ! source_verify_file "$dest" "" "$url"; then
    warn "[$label] $name $ver 下载不完整"
    return 1
  fi
  ok "[$label] $name $ver 完成（$(source_human_size "$dest")）"
}

cmd_update() {
  local dep="" want="" dry=0 chroot_mode="" env_set=0 a
  local downloads_only=0 print_urls=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run) dry=1; shift ;;
      --downloads|--download|--predownload) downloads_only=1; shift ;;
      --downloads-print-urls|--print-urls|--urls) print_urls=1; shift ;;
      --download-jobs) export PHPRT_DOWNLOAD_JOBS="${2:-}"; shift 2 ;;
      --download-jobs=*) export PHPRT_DOWNLOAD_JOBS="${1#*=}"; shift ;;
      --download-host-jobs) export PHPRT_DOWNLOAD_HOST_JOBS="${2:-}"; shift 2 ;;
      --download-host-jobs=*) export PHPRT_DOWNLOAD_HOST_JOBS="${1#*=}"; shift ;;
      --timeout) export PHPRT_DOWNLOAD_TIMEOUT="${2:-}"; shift 2 ;;
      --timeout=*) export PHPRT_DOWNLOAD_TIMEOUT="${1#*=}"; shift ;;
      --ui) export PHPRT_UI="${2:-}"; shift 2 ;;
      --ui=*) export PHPRT_UI="${1#*=}"; shift ;;
      --proxy)   export https_proxy="${2:-}" http_proxy="${2:-}" HTTPS_PROXY="${2:-}" HTTP_PROXY="${2:-}"; shift 2 ;;
      --proxy=*) export https_proxy="${1#*=}" http_proxy="${1#*=}" HTTPS_PROXY="${1#*=}" HTTP_PROXY="${1#*=}"; shift ;;
      --chroot)  chroot_mode=chroot; env_set=1; shift ;;
      --host)    chroot_mode=host; env_set=1; shift ;;
      -h|--help)
        printf '用法: phprt update <依赖> [版本] [--dry-run] [--downloads] [--downloads-print-urls] [--chroot|--host]\n'
        return 0
        ;;
      -*) die "未知参数: $1" ;;
      *)
        if [ -z "$dep" ]; then
          dep="$1"
        elif [ -z "$want" ]; then
          want="$1"
        else
          die "多余参数: $1"
        fi
        shift
        ;;
    esac
  done
  [ -n "$dep" ] || die "用法: phprt update <依赖> [版本] [--dry-run]"
  [ "$env_set" = 1 ] && buildroot_set_env "$chroot_mode"

  local ver tree bundle sh vvar uvar svar cur vers series conf
  ver="$(current_version)" || die "还没有安装 PHP（先跑 phprt install <系列>）"
  tree="$(tree_of "$ver")"
  [ -x "$tree/bin/php" ] || die "树不存在或未构建: $tree"
  [ "$(cat "$tree/BUILD-STATUS" 2>/dev/null || true)" = ok ] ||
    die "上次构建未完成，先跑 phprt install $ver 修好树"

  series="$(series_of "$ver")"
  bundle="$(bundle_for "$series")" ||
    die "找不到 bundle: php$series"
  export BUNDLE="$bundle"
  load_bundle_conf "$bundle" "$series"
  export PHPRT_PHP_VERSION="$ver"
  phprt_set_build_root "$ver"
  load_tree_ext_opts "$tree"

  sh="$bundle/deps/$dep.sh"
  [ -f "$sh" ] || die "未知依赖: $dep（$bundle/deps/ 里没有）"
  [ "$(recipe_kind_of "$sh")" = dep ] || die "$dep 不是依赖库"
  vvar="$(recipe_version_var "$sh")"
  [ -n "$vvar" ] || die "$dep 的 recipe 没有 version="
  uvar="${vvar%_VERSION}_URL"
  svar="${vvar%_VERSION}_SHA256"
  cur="$(installed_dep_version "$tree" "$dep")" ||
    die "当前树里没有依赖 $dep（$tree/deps/）"

  if [ -z "$want" ]; then
    [ "${OFFLINE:-0}" = 1 ] &&
      die "离线模式不能查询上游；请指定版本: phprt update $dep <版本>"
    vers="$(module_versions "$sh" 2>/dev/null || true)"
    [ -n "$vers" ] ||
      die "查不到 $dep 的上游版本（网络或来源问题）；可指定版本: phprt update $dep <版本>"
    want="$(printf '%s\n' "$vers" | version_safe_max "$cur")"
    if [ -z "$want" ]; then
      ok "$dep $cur 已是最新安全版本"
      return 0
    fi
  fi

  if [ "$want" = "$cur" ]; then
    ok "$dep $cur 已是最新"
    return 0
  fi
  version_is_safe_update "$cur" "$want" ||
    die "只允许同系列安全更新：$cur -> $want（跨系列请用 phprt install）"

  local conf oldurl newurl archive dest sha bak olddir status
  conf="$(builder_conf_of "$series")"
  if [ ! -f "$conf" ]; then
    conf="$bundle/bundle.conf"
  fi
  oldurl="${!uvar:-}"
  [ -n "$oldurl" ] || die "配置里没有 $uvar"
  case "$oldurl" in
    *"$cur"*) newurl="${oldurl//$cur/$want}" ;;
    *) die "URL 里找不到版本 $cur，不能自动改: $oldurl" ;;
  esac
  archive="$(source_file_of_url "$newurl")"
  dest="$DOWNLOADS/$archive"

  if [ "$print_urls" = 1 ]; then
    printf '#deps-%s %s\n%s\n' "$dep" "$archive" "$newurl"
    return 0
  fi

  printf '依赖: %s\n' "$dep"
  printf '  当前: %s\n' "$cur"
  printf '  目标: %s（同系列安全更新）\n' "$want"
  printf '  源码: %s\n' "$newurl"
  if [ "$dry" = 1 ]; then
    if [ -s "$dest" ]; then
      printf '  缓存: 已存在\n'
    else
      printf '  缓存: 需要下载\n'
    fi
    printf '  构建: 按当前扩展选择重建受影响的 PHP/扩展\n'
    return 0
  fi

  download_update_source "$newurl" "$dest" "$dep" "$want" deps ||
    die "源码下载失败；未修改配置"
  [ "$downloads_only" = 1 ] && return 0
  sha="$(sha256sum "$dest" | cut -d' ' -f1)"

  bak="$(mktemp -d "$BUILD_ROOT/update-backup.XXXXXX")"
  cp -f "$conf" "$bak/bundle.conf"
  olddir="$tree/deps/$dep-$cur"
  [ -d "$olddir" ] && mv "$olddir" "$bak/deps-old"
  cp -a "$tree/lib/phprt" "$bak/flat"
  [ -d "$tree/.phprt-state" ] && cp -a "$tree/.phprt-state" "$bak/state"
  status="$(cat "$tree/BUILD-STATUS" 2>/dev/null || true)"

  rollback_update() {
    local reason="$1"
    cp -f "$bak/bundle.conf" "$conf"
    rm -rf "$tree/deps/$dep-$want"
    [ -d "$bak/deps-old" ] && mv "$bak/deps-old" "$olddir"
    rm -rf "$tree/lib/phprt"
    [ -d "$bak/flat" ] && mv "$bak/flat" "$tree/lib/phprt"
    rm -rf "$tree/.phprt-state"
    [ -d "$bak/state" ] && mv "$bak/state" "$tree/.phprt-state"
    if [ -n "$status" ]; then
      printf '%s\n' "$status" > "$tree/BUILD-STATUS"
    else
      rm -f "$tree/BUILD-STATUS"
    fi
    rm -rf "$bak"
    die "$reason；已回滚到 $dep $cur"
  }

  if grep -q "^$vvar=" "$conf"; then
    sed -i "s|^$vvar=.*|$vvar=$want|" "$conf" || rollback_update "改 $vvar 失败"
  else
    printf '%s=%s\n' "$vvar" "$want" >> "$conf" || rollback_update "写 $vvar 失败"
  fi
  if grep -q "^$uvar=" "$conf"; then
    sed -i "s|^\\($uvar=\"\\)[^\"]*\"|\\1$newurl\"|" "$conf" ||
      rollback_update "改 $uvar 失败"
  else
    printf '%s="%s"\n' "$uvar" "$newurl" >> "$conf" || rollback_update "写 $uvar 失败"
  fi
  if grep -q "^$svar=" "$conf"; then
    sed -i "s|^$svar=.*|$svar=$sha|" "$conf" || rollback_update "改 $svar 失败"
  else
    printf '%s=%s\n' "$svar" "$sha" >> "$conf" || rollback_update "写 $svar 失败"
  fi
  # shellcheck disable=SC1090
  . "$conf"

  local exts="" e em mode_env=""
  exts="$(tree_exts "$ver")"
  for e in $exts; do
    em="$(mode_of "$e" 2>/dev/null || true)"
    [ -n "$em" ] || em="$(recipe_default_mode "$(ext_recipe_sh "$e")")"
    mode_env="$mode_env${mode_env:+ }$e=$em"
  done
  export PHPRT_EXT_MODES="$mode_env" SELECTED_EXTS="$exts"
  export PHPRT_SEED="${PHPRT_SEED:-}"
  export PHPRT_FORCE_PHP=0 PHPRT_CLEAN_SRC=0

  prefetch_sources "$exts" 0 || rollback_update "源码下载失败"

  need_cmds flock
  exec 9>"$BUILD_ROOT/.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"

  log "更新依赖 $dep: $cur -> $want"
  if ! PHPRT_NO_DOWNLOAD=1 run_build_env bash "$PHPRT_RUNNER/build.sh" \
         --bundle "$bundle" --tree "$tree" --exts "$exts"; then
    rollback_update "构建失败"
  fi

  [ -x "$tree/bin/php" ] || rollback_update "构建结束但找不到 bin/php"
  rm -rf "$bak"
  ok "依赖 $dep 已更新: $cur -> $want"
  printf '    树: %s\n' "$tree"
}

# 把 current 切到 concrete 并同步入口（install 与 use 共用）。
# apply=1 时按 exts 重写 conf.d（install 用）；use 只切版本，不动用户的 ini。
activate_tree() {
  local concrete="$1" exts="$2" apply="${3:-1}" tree old e bundle
  tree="$(tree_of "$concrete")"
  switch_current "$concrete"
  # recipe 注册表按版本覆盖走（use 不需要构建目录，不在这里建）
  export PHPRT_PHP_VERSION="$concrete"
  # bundle 里是 recipe 注册表；driver 的注册表函数都靠 BUNDLE 找文件
  bundle="$(bundle_for "$(series_of "$concrete")")" ||
    die "缺少 bundle: php$(series_of "$concrete")"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" DEPS="$tree/deps"
  ensure_tree_config "$tree"
  ensure_tree_ssl "$tree"
  if [ "$apply" = 1 ]; then
    # 去掉本次未选中的扩展 ini（静态内置无法卸载，但至少不再启用）
    for old in $(all_ext_names); do
      case " $exts " in
        *" $old "*) ;;
        *) rm -f "$tree/etc/conf.d"/*-"$old".ini ;;
      esac
    done
    for e in $exts; do
      enable_ext "$e" || warn "扩展 $e 没有启用"
    done
  fi
  write_myphp
}

# 切到已装树：系列写法取该系列已装最高版，精确写法指定树
cmd_use() {
  local arg="${1:-}" series concrete tree
  [ -n "$arg" ] || die "用法: phprt use <系列|版本>（如 use 8.5 / use 8.5.12）"
  series="$(series_of "$arg")"
  case "$arg" in
    *.*.*)
      printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-dev)?$' ||
        die "版本号格式不对: $arg（精确版如 8.5.12）"
      concrete="$arg" ;;
    *)
      printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+$' ||
        die "系列格式不对: $arg（应为 major.minor，如 8.5）"
      concrete="$(installed_series_max "$series")"
      [ -n "$concrete" ] || die "还没安装 PHP $series（先跑 phprt install $series）" ;;
  esac
  tree="$(tree_of "$concrete")"
  [ -x "$tree/bin/php" ] || die "树不存在或未构建: $tree"
  # 只切软链：扩展以目标树自己的 ini / php -m 为准，不拿别的版本覆盖
  activate_tree "$concrete" "" 0
  ok "切换到 PHP $concrete"
}

# ---- show --tree：扩展 / PHP 本体的运行时库依赖树 --------------------------

declare -A PHPRT_ELF_NEEDED_CACHE=()
declare -A PHPRT_DEP_RT_CACHE=()

elf_needed() {  # $1=ELF -> 每行一个 NEEDED（带缓存）
  local f="$1" out
  if [ -n "${PHPRT_ELF_NEEDED_CACHE[$f]+x}" ]; then
    out="${PHPRT_ELF_NEEDED_CACHE[$f]}"
  else
    out="$(readelf -d "$f" 2>/dev/null |
      sed -n 's/.*(NEEDED).*\[\(.*\)\]$/\1/p')"
    PHPRT_ELF_NEEDED_CACHE[$f]="$out"
  fi
  [ -n "$out" ] && printf '%s\n' "$out"
}

ext_so_file() {  # $1=tree $2=扩展名 -> 共享扩展的 .so
  local tree="$1" e="$2" extdir m
  extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
  [ -n "$extdir" ] || return 1
  for m in $(ext_modules "$e"); do
    [ -f "$extdir/$m.so" ] && { printf '%s\n' "$extdir/$m.so"; return 0; }
  done
  return 1
}

# 扩展 recipe 的 needs（不能走 module_sh：curl/zlib 等会先命中同名 dep）
ext_needs() {
  local sh
  sh="$(ext_recipe_sh "$1")" || return 0
  _recipe_needs_only_sh "$sh" | awk 'NF && !seen[$0]++'
}

# dep 安装目录里的入口 .so -> 树内 lib/phprt 下的 rt 文件名
dep_runtime_libs() {
  local tree="$1" dep="$2" wheelmap d so soname rt key out
  key="$tree|$dep"
  if [ -n "${PHPRT_DEP_RT_CACHE[$key]+x}" ]; then
    out="${PHPRT_DEP_RT_CACHE[$key]}"
    [ -n "$out" ] && printf '%s\n' "$out"
    return 0
  fi
  wheelmap="$tree/lib/phprt/wheel.map"
  out="$(
    for d in "$tree"/deps/"$dep"-*/lib "$tree"/deps/"$dep"-*/lib64; do
      [ -d "$d" ] || continue
      for so in "$d"/*.so*; do
        [ -f "$so" ] && [ ! -L "$so" ] || continue
        soname="$(readelf -d "$so" 2>/dev/null |
          sed -n 's/.*(SONAME).*\[\(.*\)\]$/\1/p' | head -1)"
        [ -n "$soname" ] || continue
        rt=""
        [ -f "$wheelmap" ] &&
          rt="$(awk -F'|' -v s="$soname" '$1 == s { print $2; exit }' "$wheelmap")"
        [ -n "$rt" ] || rt="$soname"
        [ -f "$tree/lib/phprt/$rt" ] && printf '%s\n' "$rt"
      done
    done | awk 'NF && !seen[$0]++'
  )"
  PHPRT_DEP_RT_CACHE[$key]="$out"
  [ -n "$out" ] && printf '%s\n' "$out"
}

# bin/php 能到达的树内运行时库（不含外部/系统库）
php_runtime_closure() {
  local tree="$1" cur x out=" " queue=()
  while IFS= read -r cur; do
    [ -n "$cur" ] && queue+=("$cur")
  done < <(elf_needed "$tree/bin/php")
  while [ ${#queue[@]} -gt 0 ]; do
    cur="${queue[0]}"
    queue=("${queue[@]:1}")
    case " $out " in *" $cur "*) continue ;; esac
    [ -f "$tree/lib/phprt/$cur" ] || continue
    out+="$cur "
    while IFS= read -r x; do
      [ -n "$x" ] && queue+=("$x")
    done < <(elf_needed "$tree/lib/phprt/$cur")
  done
  printf '%s\n' "$out"
}

# 递归打印一个库：$4=1 表示它是同级最后一个
show_lib_branch() {
  local tree="$1" lib="$2" prefix="$3" last="$4" seen="$5"
  local branch child_prefix
  if [ "$last" = 1 ]; then branch="└── "; else branch="├── "; fi
  if [ ! -f "$tree/lib/phprt/$lib" ]; then
    printf '%s%s%s（外部）\n' "$prefix" "$branch" "$lib"
    return 0
  fi
  case "$seen" in
    *"|$lib|"*) printf '%s%s%s（循环）\n' "$prefix" "$branch" "$lib"; return 0 ;;
  esac
  printf '%s%s%s\n' "$prefix" "$branch" "$lib"
  if [ "$last" = 1 ]; then child_prefix="$prefix    "; else child_prefix="$prefix│   "; fi

  local -a internal=() external=()
  local x
  while IFS= read -r x; do
    [ -n "$x" ] || continue
    if [ -f "$tree/lib/phprt/$x" ]; then
      internal+=("$x")
    else
      external+=("$x")
    fi
  done < <(elf_needed "$tree/lib/phprt/$lib")

  local total=${#internal[@]} i=0 child_last=0
  [ ${#external[@]} -gt 0 ] && total=$((total + 1))
  for x in "${internal[@]}"; do
    i=$((i + 1))
    child_last=0
    [ "$i" -eq "$total" ] && child_last=1
    show_lib_branch "$tree" "$x" "$child_prefix" "$child_last" "$seen|$lib|"
  done
  if [ ${#external[@]} -gt 0 ]; then
    printf '%s└── 外部: %s\n' "$child_prefix" \
      "$(printf '%s\n' "${external[@]}" | paste -sd' ' -)"
  fi
}

show_ext_link_tree() {  # $1=tree $2=扩展名 $3=static|shared|unknown $4=php 闭包
  local tree="$1" e="$2" mode="$3"
  local so dep rt x closed="${4:-}" roots=() internal=() external=()
  case "$mode" in
    shared)
      if so="$(ext_so_file "$tree" "$e")"; then
        printf '      %s（共享模块）: %s\n' "$e" "$(basename "$so")"
        while IFS= read -r x; do
          [ -n "$x" ] && roots+=("$x")
        done < <(elf_needed "$so")
      else
        printf '      %s（共享模块，找不到 .so）\n' "$e"
        return 0
      fi
      ;;
    static)
      printf '      %s（静态链接，编入 bin/php）\n' "$e"
      [ -n "$closed" ] || closed="$(php_runtime_closure "$tree")"
      # 全静态树（--static-all）没有树内动态库，不必再跑 recipe 的 needs
      if [ -z "${closed// /}" ]; then
        printf '        └── 无树内动态库依赖\n'
        return 0
      fi
      while IFS= read -r dep; do
        [ -n "$dep" ] || continue
        while IFS= read -r rt; do
          [ -n "$rt" ] || continue
          case " $closed " in *" $rt "*) roots+=("$rt") ;; esac
        done < <(dep_runtime_libs "$tree" "$dep")
      done < <(ext_needs "$e")
      ;;
    *)
      printf '      %s（状态未知）\n' "$e"
      return 0
      ;;
  esac

  # 去重，保持顺序
  local seen=" " r
  local -a uniq=()
  for r in "${roots[@]}"; do
    case "$seen" in *" $r "*) continue ;; esac
    seen+="$r "
    uniq+=("$r")
  done
  roots=("${uniq[@]}")

  for r in "${roots[@]}"; do
    if [ -f "$tree/lib/phprt/$r" ]; then
      internal+=("$r")
    else
      external+=("$r")
    fi
  done

  if [ ${#internal[@]} -eq 0 ] && [ ${#external[@]} -eq 0 ]; then
    printf '        └── 无树内动态库依赖\n'
    return 0
  fi

  local total=${#internal[@]} i=0 child_last=0
  [ ${#external[@]} -gt 0 ] && total=$((total + 1))
  for r in "${internal[@]}"; do
    i=$((i + 1))
    child_last=0
    [ "$i" -eq "$total" ] && child_last=1
    show_lib_branch "$tree" "$r" "        " "$child_last" "|"
  done
  if [ ${#external[@]} -gt 0 ]; then
    printf '        └── 外部: %s\n' "$(printf '%s\n' "${external[@]}" | paste -sd' ' -)"
  fi
}

show_php_link_tree() {  # $1=tree
  local tree="$1" lib internal=() external=()
  while IFS= read -r lib; do
    [ -n "$lib" ] || continue
    if [ -f "$tree/lib/phprt/$lib" ]; then
      internal+=("$lib")
    else
      external+=("$lib")
    fi
  done < <(elf_needed "$tree/bin/php")

  printf '    PHP 本体（bin/php）:\n'
  local total=${#internal[@]} i=0 last=0
  [ ${#external[@]} -gt 0 ] && total=$((total + 1))
  for lib in "${internal[@]}"; do
    i=$((i + 1))
    last=0
    [ "$i" -eq "$total" ] && last=1
    if [ "$last" = 1 ]; then
      printf '      └── %s\n' "$lib"
    else
      printf '      ├── %s\n' "$lib"
    fi
  done
  if [ ${#external[@]} -gt 0 ]; then
    printf '      └── 外部: %s\n' "$(printf '%s\n' "${external[@]}" | paste -sd' ' -)"
  fi
  [ ${#internal[@]} -gt 0 ] || [ ${#external[@]} -gt 0 ] ||
    printf '      └── 无动态库\n'
}

cmd_show() {
  local tree_all=0 tree_one=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --tree)
        tree_all=1
        shift
        if [ $# -gt 0 ] && [ "${1#-}" = "$1" ]; then
          tree_one="$1"
          shift
        fi
        ;;
      --tree=*)
        tree_all=1
        tree_one="${1#--tree=}"
        shift
        ;;
      -h|--help)
        printf '用法: phprt show [--tree [扩展]]\n'
        return 0
        ;;
      *)
        die "未知参数: $1（show 只接受 --tree [扩展]）"
        ;;
    esac
  done

  local ver tree bundle
  ver="$(current_version)" || die "还没有安装 PHP（先跑 phprt install <系列>）"
  tree="$(tree_of "$ver")"
  printf 'PHP %s\n' "$ver"
  printf '    路径: %s\n' "$tree"
  printf '    php.ini: %s\n' "$tree/etc/php.ini"
  local lm lt
  lm="$(link_mode_get "$tree" 2>/dev/null || true)"
  lt="$(lto_mode_get "$tree" 2>/dev/null || true)"
  if [ "${lm:-shared}" = static ] && [ "${lt:-0}" = 1 ]; then
    printf '    链接方式: static（LTO）\n'
  else
    printf '    链接方式: %s\n' "${lm:-shared}"
  fi
  bundle="$(bundle_for "$(series_of "$ver")")" || true
  if [ -n "$bundle" ]; then
    export BUNDLE="$bundle"
    # 扩展状态按 php -m 推导（用户手改 ini / 删 .so 后仍然准确）。
    # 静态链接 = 在 php -n -m 里仍然出现（-n 不读 php.ini/conf.d）；
    # 共享模块 = lib/php/extensions 下有 .so，且只有带配置时才会加载。
    local exts e names enabled=" " static_list="" shared_list="" unknown_list="" off_list=""
    local n_static=0 n_shared=0 n_unknown=0 n_off=0 builtin_mods extdir ld
    local mm is_static has_so
    local -A ext_mode_map=()
    exts="$(tree_exts "$ver")"
    for e in $exts; do enabled+="$e "; done
    extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
    ld="$tree/lib:$(ls -d "$tree"/deps/*/lib 2>/dev/null | tr '\n' ':')"
    builtin_mods="$(LD_LIBRARY_PATH="$ld" "$tree/bin/php" -n -m 2>/dev/null |
      tr '[:upper:]' '[:lower:]')"
    names="$(all_ext_names)"
    for e in $names; do
      case "$enabled" in
        *" $e "*) ;;
        *) off_list+="$e "; n_off=$((n_off + 1)); continue ;;
      esac
      is_static=0
      for mm in $(ext_modules "$e"); do
        if printf '%s\n' "$builtin_mods" | grep -cix "$mm" >/dev/null; then
          is_static=1
          break
        fi
      done
      has_so=0
      if [ -n "$extdir" ]; then
        [ -f "$extdir/$e.so" ] && has_so=1
        if [ "$has_so" = 0 ]; then
          for mm in $(ext_modules "$e"); do
            [ -f "$extdir/$mm.so" ] && { has_so=1; break; }
          done
        fi
      fi
      if [ "$is_static" = 1 ]; then
        static_list+="$e "; n_static=$((n_static + 1))
        ext_mode_map[$e]=static
      elif [ "$has_so" = 1 ]; then
        shared_list+="$e "; n_shared=$((n_shared + 1))
        ext_mode_map[$e]=shared
      else
        unknown_list+="$e "; n_unknown=$((n_unknown + 1))
        ext_mode_map[$e]=unknown
      fi
    done
    printf '    扩展: 已启用 %s（静态链接 %s，共享模块 %s），未启用 %s\n' \
      "$((n_static + n_shared + n_unknown))" "$n_static" "$n_shared" "$n_off"
    if [ "$n_static" = 0 ]; then
      printf '      静态链接: 无\n'
    else
      printf '      静态链接: %s\n' "${static_list% }"
    fi
    if [ "$n_shared" = 0 ]; then
      printf '      共享模块: 无\n'
    else
      printf '      共享模块: %s\n' "${shared_list% }"
    fi
    [ "$n_unknown" = 0 ] || printf '      状态未知: %s\n' "${unknown_list% }"
    [ "$n_off" = 0 ] || printf '      未启用: %s\n' "${off_list% }"

    if [ "$tree_all" = 1 ]; then
      command -v readelf >/dev/null 2>&1 ||
        die "缺少 readelf（binutils），无法打印链接树"
      if [ -n "$tree_one" ]; then
        ext_recipe_sh "$tree_one" >/dev/null 2>&1 ||
          die "未知扩展: $tree_one"
        case "$enabled" in
          *" $tree_one "*) ;;
          *) die "扩展未启用: $tree_one" ;;
        esac
      fi
      # builtin 的 needs() 依赖 ENABLE_* 开关
      export SELECTED_EXTS="$exts"
      builtin_apply_switches
      load_tree_ext_opts "$tree"
      apply_ext_opts
      printf '    链接树:\n'
      show_php_link_tree "$tree"
      local php_closed
      php_closed="$(php_runtime_closure "$tree")"
      for e in ${tree_one:-$exts}; do
        show_ext_link_tree "$tree" "$e" "${ext_mode_map[$e]:-unknown}" "$php_closed"
      done
    else
      printf '      链接树: phprt show --tree [扩展]\n'
    fi
  fi
  ( cd "$PHPRT_PROJECT" && ./myphp -v | head -1 | sed 's/^/    /' )
}

cmd_current() {
  local v
  v="$(current_version)" || die "还没有设置 current（先跑 phprt install <系列>）"
  printf '%s\n' "$v"
}

cmd_list() {
  local cur d name
  cur="$(current_version 2>/dev/null || true)"
  [ -d "$PHPRT_HOME" ] || { printf '（还没有 .phprt）\n'; return 0; }
  for d in "$PHPRT_HOME"/php-*; do
    [ -d "$d" ] || continue
    name="$(basename "$d")"
    name="${name#php-}"
    [ -x "$d/bin/php" ] || continue
    if [ "$name" = "$cur" ]; then
      printf '* %s\n' "$name"
    else
      printf '  %s\n' "$name"
    fi
  done
}

# phprt select [系列|版本] [--php 系列] [--default]
# 交互选择扩展和子选项，结果打印到 stdout。install 内部也调用同一套 UI。
cmd_select() {
  local arg="" php_arg="" use_default=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --php)     php_arg="${2:-}"; shift 2 ;;
      --php=*)   php_arg="${1#*=}"; shift ;;
      --default) use_default=1; shift ;;
      -h|--help)
        printf '用法: phprt select [系列|版本] [--php 系列] [--default]\n'
        return 0 ;;
      -*) die "未知参数: $1" ;;
      *)  [ -z "$arg" ] || die "多余参数: $1"; arg="$1"; shift ;;
    esac
  done
  [ -n "$arg" ] || arg="$php_arg"

  local series="" concrete="" bundle tree initial init_opts cur
  if [ -n "$arg" ]; then
    case "$arg" in
      *.*.*)
        printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-dev)?$' ||
          die "版本号格式不对: $arg"
        series="$(series_of "$arg")"
        concrete="$arg" ;;
      *)
        printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+$' ||
          die "系列格式不对: $arg（应为 major.minor，如 8.5）"
        series="$arg" ;;
    esac
  else
    cur="$(current_version 2>/dev/null || true)"
    [ -n "$cur" ] || die "请指定系列，如 phprt select 8.5"
    series="$(series_of "$cur")"
    concrete="$cur"
  fi

  bundle="$(bundle_for "$series")" || die "找不到 bundle: php$series"
  export BUNDLE="$bundle"
  load_bundle_conf "$bundle" "$series"
  [ -n "$concrete" ] || concrete="$PHP_VERSION"
  export PHPRT_PHP_VERSION="$concrete"
  tree="$(tree_of "$concrete")"

  cur="$(current_version 2>/dev/null || true)"
  initial="${DEFAULT_EXTS:-}"
  if [ -n "$cur" ] && [ "$(series_of "$cur")" = "$series" ]; then
    initial="$(tree_exts "$cur" 2>/dev/null || true)"
  fi
  init_opts=""
  [ -f "$tree/.build-opts" ] && init_opts="$(tr '\n' ' ' < "$tree/.build-opts")"
  init_opts="${init_opts% }"

  if [ "$use_default" = 1 ] || ! ui_select_available; then
    printf '扩展: %s\n' "$initial"
    [ -n "$init_opts" ] && printf '子选项: %s\n' "$init_opts"
    return 0
  fi
  UI_SELECT_QUIET=1
  if ! ui_ext_select "$series" "$concrete" "$tree" "$initial" "$init_opts"; then
    UI_SELECT_QUIET=0
    return 130
  fi
  UI_SELECT_QUIET=0
  printf '扩展: %s\n' "$UI_SELECTED_EXTS"
  if [ -n "$UI_SELECTED_OPTS" ]; then
    printf '子选项: %s\n' "$UI_SELECTED_OPTS"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# ext list --json 的契约字段。所有值都从 recipe/accessor 取，driver 不写包名。
# ---------------------------------------------------------------------------
ext_json_flag_array() {  # $1=recipe $2=0|1 -> JSON 数组；没有查询钩子时输出 null
  local sh="$1" val="$2" out line first=1 arr="" verb=""
  if recipe_has_func "$sh" flags; then
    verb=--flags
  elif [ "$(recipe_kind_of "$sh")" = builtin ]; then
    verb=--flags
  elif recipe_has_func "$sh" configure_args; then
    verb=--configure-args
  else
    printf 'null'
    return 0
  fi
  # ext 不选就不构建，off 侧没有负向 configure 参数。
  if [ "$(recipe_kind_of "$sh")" != builtin ] && [ "$val" = 0 ]; then
    printf '[]'
    return 0
  fi
  out="$(recipe_query_with_switch "$sh" "$val" "$verb" || true)"
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    [ -n "${DEPS:-}" ] && line="${line//$DEPS/\$\{DEPS\}}"
    [ -n "${TREE:-}" ] && line="${line//$TREE/\$\{TREE\}}"
    [ -n "${BUILD_ROOT:-}" ] && line="${line//$BUILD_ROOT/\$\{BUILD_ROOT\}}"
    [ "$first" = 1 ] || arr+=","
    first=0
    arr+="\"$(json_escape "$line")\""
  done <<< "$out"
  printf '[%s]' "$arr"
}

ext_json_needs() {  # $1=recipe $2=kind [$3=on] [$4=off]
  local sh="$1" kind="$2" on="${3-}" off="${4-}" declared="null" by_opt="{}"
  if [ $# -lt 3 ]; then
    on="$(recipe_query_with_switch "$sh" 1 --needs || true)"
    on="$(printf '%s\n' "$on" | awk 'NF && !seen[$0]++' | tr '\n' ' ')"
    on="${on% }"
    if [ "$kind" = builtin ]; then
      off="$(recipe_query_with_switch "$sh" 0 --needs || true)"
      off="$(printf '%s\n' "$off" | awk 'NF && !seen[$0]++' | tr '\n' ' ')"
      off="${off% }"
    fi
  fi
  if recipe_has_key "$sh" needs; then
    declared="$(json_words "$(recipe_needs_of "$sh")")"
  fi
  printf '{"declared":%s,"on":%s,"off":%s,"by_opt":%s}' \
    "$declared" "$(json_words $on)" "$(json_words $off)" "$by_opt"
}

ext_json_opts() {  # $1=recipe
  local sh="$1" sw meta key state label env first=1 out=""
  sw="$(recipe_switch "$sh")"
  meta="$(recipe_opts_meta "$sh" 2>/dev/null || true)"
  while IFS=$'\t' read -r key state label; do
    [ -n "$key" ] || continue
    env="$(ext_opt_var "$sw" "$key")"
    [ "$first" = 1 ] || out+=","
    first=0
    out+="{\"key\":\"$(json_escape "$key")\",\"state\":\"$(json_escape "$state")\",\"label\":\"$(json_escape "$label")\",\"env\":\"$(json_escape "$env")\"}"
  done <<< "$meta"
  printf '[%s]' "$out"
}

ext_json_detail() {  # $1..=依赖名 -> {name:{kind,version,needs,tools,active,notes}}
  local first=1 out="" name kind ver needs tools notes
  while IFS='|' read -r name kind ver needs tools notes; do
    [ -n "$name" ] || continue
    [ "$first" = 1 ] || out+=","
    first=0
    out+="\"$(json_escape "$name")\":{\"kind\":\"$(json_escape "$kind")\",\"version\":\"$(json_escape "$ver")\",\"needs\":$(json_words $needs),\"tools\":$(json_words $tools),\"active\":true,\"notes\":$(json_words $notes)}"
  done < <(recipe_needs_closure "$@")
  printf '{%s}' "$out"
}

ext_json_nullable() {  # $1=值 -> JSON 字符串或 null
  [ -n "$1" ] && printf '"%s"' "$(json_escape "$1")" || printf 'null'
}

# phprt ext list [系列] [--php 系列] [--default] [--installed]
#                [--builtin|--pecl] [--names|--json] [--name 扩展]
cmd_ext() {
  local sub="${1:-list}"
  case "$sub" in
    list) shift ;;
    -h|--help)
      printf '用法: phprt ext list [系列] [--php 系列] [--default] [--installed] [--builtin|--pecl] [--names|--json] [--name 扩展]\n'
      return 0 ;;
    *) die "未知子命令: ext $sub（当前只有 ext list）" ;;
  esac

  local arg="" php_arg="" only_default=0 only_installed=0 only_kind="" only_name=""
  local names=0 json=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --php)       php_arg="${2:-}"; shift 2 ;;
      --php=*)     php_arg="${1#*=}"; shift ;;
      --all)       shift ;;   # 默认就是全部，保留参数方便脚本
      --default)   only_default=1; shift ;;
      --installed) only_installed=1; shift ;;
      --builtin)   only_kind=builtin; shift ;;
      --pecl)      only_kind=pecl; shift ;;
      --name)      only_name="${2:-}"; shift 2 ;;
      --name=*)    only_name="${1#*=}"; shift ;;
      --names)     names=1; shift ;;
      --json)      json=1; shift ;;
      -h|--help)
        printf '用法: phprt ext list [系列] [--php 系列] [--default] [--installed] [--builtin|--pecl] [--names|--json] [--name 扩展]\n'
        return 0 ;;
      -*) die "未知参数: $1" ;;
      *)
        [ -z "$arg" ] || die "多余参数: $1"
        arg="$1"; shift ;;
    esac
  done

  [ -n "$arg" ] || arg="$php_arg"
  local series="" cur repo display
  cur="$(current_version 2>/dev/null || true)"
  if [ -n "$arg" ]; then
    case "$arg" in
      *.*.*)
        printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-dev)?$' ||
          die "版本号格式不对: $arg"
        series="$(series_of "$arg")"
        display="$arg" ;;
      *)
        printf '%s' "$arg" | grep -Eq '^[0-9]+\.[0-9]+$' ||
          die "系列格式不对: $arg（应为 major.minor，如 8.5）"
        series="$arg"
        display="$arg" ;;
    esac
  elif [ -n "$cur" ]; then
    series="$(series_of "$cur")"
    display="$cur"
  else
    repo="$(repo_root)"
    local -a dirs=("$repo"/bundles/php*)
    [ -d "${dirs[0]:-}" ] || die "找不到 bundle（需要 phprt 仓库或 PHPRT_REPO）"
    [ "${#dirs[@]}" -eq 1 ] ||
      die "有多个 bundle，请指定系列: phprt ext list 8.5"
    series="$(basename "${dirs[0]}")"
    series="${series#php}"
    display="$series"
  fi

  local bundle
  bundle="$(bundle_for "$series")" || die "找不到 bundle: php$series"
  export BUNDLE="$bundle"
  load_bundle_conf "$bundle" "$series"

  local enabled=" " has_tree=0
  if [ -n "$cur" ] && [ "$(series_of "$cur")" = "$series" ]; then
    has_tree=1
    enabled=" $(tree_exts "$cur") "
  fi
  if [ "$only_installed" = 1 ] && [ "$has_tree" != 1 ]; then
    die "当前没有 PHP $series 的已安装树；先 phprt install $series"
  fi

  local e sh kind ver modes dflt dflt_set inst cat line
  local -a rows=()
  while IFS= read -r e; do
    [ -n "$e" ] || continue
    sh="$(ext_recipe_sh "$e")" || continue
    [ -n "$only_name" ] && [ "$e" != "$only_name" ] && continue
    kind="$(recipe_kind_of "$sh")"
    [ "$kind" = builtin ] && kind=builtin || kind=pecl
    [ -n "$only_kind" ] && [ "$kind" != "$only_kind" ] && continue

    ver="$(recipe_version "$sh")"
    [ -n "$ver" ] || ver="-"
    modes="$(recipe_types "$sh" | tr ' ' ',')"
    dflt="$(recipe_default_mode "$sh")"
    dflt_set=""
    case " ${DEFAULT_EXTS:-} " in *" $e "*) dflt_set="*" ;; esac
    inst=""
    case "$enabled" in *" $e "*) inst="installed" ;; esac

    [ "$only_default" = 1 ] && [ -z "$dflt_set" ] && continue
    [ "$only_installed" = 1 ] && [ -z "$inst" ] && continue

    cat="$(ext_category_of "$sh")"
    rows+=("$e|$kind|$ver|$modes|$dflt|$dflt_set|$inst|$cat")
  done < <(all_ext_names)

  if [ -n "$only_name" ] && [ ${#rows[@]} -eq 0 ]; then
    die "未知扩展: $only_name$(ext_name_hint "$only_name")"
  fi

  if [ "$names" = 1 ]; then
    local row
    for row in "${rows[@]}"; do printf '%s\n' "${row%%|*}"; done
    return 0
  fi

  if [ "$json" = 1 ]; then
    local first=1 name sh contract_kind catlabel needs_on needs_off old_sel
    local flags_on flags_off flags_source
    # 查询 flags/needs 不要求已安装树；给 recipe 一个稳定的目标路径。
    export DEPS="${DEPS:-$PHPRT_HOME/php-${PHP_VERSION}/deps}"
    export BUILD_ROOT="${BUILD_ROOT:-$PHPRT_HOME/build-php-${PHP_VERSION}}"
    export TREE="${TREE:-$PHPRT_HOME/php-${PHP_VERSION}}"
    export PREFIX="${PREFIX:-$TREE}"
    printf '[\n'
    for row in "${rows[@]}"; do
      IFS='|' read -r name kind ver modes dflt dflt_set inst cat <<< "$row"
      sh="$(ext_recipe_sh "$name")" || continue
      contract_kind="$(recipe_kind_of "$sh")"
      catlabel="$(ext_category_label "$cat")"

      old_sel="${SELECTED_EXTS:-}"
      export SELECTED_EXTS="$name"
      apply_ext_opts 2>/dev/null || true

      needs_on="$(recipe_query_with_switch "$sh" 1 --needs 2>/dev/null || true)"
      needs_on="$(printf '%s\n' "$needs_on" |
        awk 'NF && !seen[$0]++' | tr '\n' ' ')"
      needs_on="${needs_on% }"
      needs_off=""
      if [ "$contract_kind" = builtin ]; then
        needs_off="$(recipe_query_with_switch "$sh" 0 --needs 2>/dev/null || true)"
        needs_off="$(printf '%s\n' "$needs_off" |
          awk 'NF && !seen[$0]++' | tr '\n' ' ')"
        needs_off="${needs_off% }"
      fi

      if recipe_has_func "$sh" flags; then
        flags_source="flags()"
      elif [ "$contract_kind" = builtin ]; then
        flags_source="flags()"
      elif recipe_has_func "$sh" configure_args; then
        flags_source="configure_args()"
      else
        flags_source="build()"
      fi
      flags_on="$(ext_json_flag_array "$sh" 1)"
      flags_off="$(ext_json_flag_array "$sh" 0)"

      [ "$first" = 1 ] || printf ',\n'
      first=0
      printf '  {"contract_schema":1,"name":"%s","kind":"%s","version":"%s","switch":%s,"modules":%s,"ini":%s,"modes":"%s","default":"%s","default_set":%s,"installed":%s,"category":"%s","category_label":"%s","menu_order":%s,"desc":%s,"tools":%s,"needs":%s,"needs_detail":%s,"opts":%s,"flags_source":"%s","flags_on":%s,"flags_off":%s,"hooks":%s,"upstream":%s,"notes":%s}' \
        "$(json_escape "$name")" "$(json_escape "$kind")" \
        "$(json_escape "$ver")" "$(ext_json_nullable "$(recipe_switch "$sh")")" \
        "$(json_words $(recipe_modules_list "$sh"))" \
        "$(ext_json_nullable "$(recipe_ini "$sh")")" \
        "$(json_escape "$modes")" "$(json_escape "$dflt")" \
        "$([ -n "$dflt_set" ] && printf true || printf false)" \
        "$([ -n "$inst" ] && printf true || printf false)" \
        "$(json_escape "$cat")" "$(json_escape "$catlabel")" \
        "$(ext_menu_order "$sh")" \
        "$(ext_json_nullable "$(recipe_desc "$sh")")" \
        "$(json_words $(recipe_tools_of "$sh"))" \
        "$(ext_json_needs "$sh" "$contract_kind" "$needs_on" "$needs_off")" \
        "$(ext_json_detail $needs_on $(recipe_tools_of "$sh"))" \
        "$(ext_json_opts "$sh")" \
        "$(json_escape "$flags_source")" "$flags_on" "$flags_off" \
        "$(json_lines < <(recipe_hooks "$sh"))" \
        "$(ext_json_nullable "$(recipe_upstream "$sh")")" \
        "$(json_notes "$sh")"

      export SELECTED_EXTS="$old_sel"
    done
    printf '\n]\n'
    return 0
  fi

  local title="可用扩展"
  [ "$only_default" = 1 ] && title="默认扩展"
  [ "$only_installed" = 1 ] && title="已启用扩展"
  printf 'PHP %s %s（%d）\n\n' "$display" "$title" "${#rows[@]}"
  printf '%-12s %-7s %-10s %-14s %-7s %s\n' NAME KIND VERSION MODES DEFAULT STATUS
  local row name
  for row in "${rows[@]}"; do
    IFS='|' read -r name kind ver modes dflt dflt_set inst cat <<< "$row"
    printf '%-12s %-7s %-10s %-14s %-7s %s\n' \
      "$name" "$kind" "$ver" "$modes" "${dflt_set:--}" "${inst:--}"
  done
}

# 单个模块的检查结果：name<TAB>kind<TAB>current<TAB>latest<TAB>safe
# 查不到上游时输出 -，返回 1，让调用方统计“结果不完整”。
outdated_module_row() {
  local name="$1" kind="$2" sh="$3" cur="$4" vers latest safe
  if [ "$(recipe_upstream "$sh")" = none ] &&
     ! recipe_has_func "$sh" versions && ! recipe_has_func "$sh" latest_version; then
    # 显式声明不检查上游，不算“结果不完整”
    printf '%s\t%s\t%s\t-\t-\n' "$name" "$kind" "$cur"
    return 0
  fi
  vers="$(module_versions "$sh" || true)"
  if [ -z "$vers" ]; then
    printf '%s\t%s\t%s\t-\t-\n' "$name" "$kind" "$cur"
    return 1
  fi
  latest="$(printf '%s\n' "$vers" | version_max)"
  safe="$(printf '%s\n' "$vers" | version_safe_max "$cur")"
  printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$kind" "$cur" "${latest:--}" "${safe:--}"
}

# 在线检查安全更新。
# 默认只看当前版本树实际启用的扩展及其依赖（php -m + needs 拓扑）；
# --all 检查 bundle 里所有接入的模块。
cmd_outdated() {
  local json=0 all=0 series="" ver
  while [ $# -gt 0 ]; do
    case "$1" in
      --json) json=1; shift ;;
      --all)  all=1; shift ;;
      -*)     die "未知参数: $1" ;;
      *)      series="$1"; shift ;;
    esac
  done

  if [ -n "$series" ]; then
    ver="$(installed_series_max "$series")"
  else
    ver="$(current_version 2>/dev/null || true)"
  fi

  local bundle series_key
  series_key="${series:-$(series_of "$ver")}"
  bundle="$(bundle_for "$series_key")" ||
    die "找不到 bundle: php$series_key"
  export BUNDLE="$bundle"
  load_bundle_conf "$bundle" "$series_key"

  if [ "${OFFLINE:-0}" = 1 ]; then
    printf '离线模式，未查询上游（outdated 需要联网；去掉 OFFLINE=1 再试）\n' >&2
    return 3
  fi

  if [ "$all" != 1 ] && [ -z "$ver" ]; then
    die "还没有安装 PHP，无法确定当前启用的扩展；用 phprt outdated --all 查看全部，或先 install"
  fi

  local rows unknown=0
  rows="$(mktemp "${TMPDIR:-/tmp}/phprt-outdated.XXXXXX")"

  # ---- PHP 本体 ----
  local php_cur="$PHP_VERSION" php_up="${PHP_UPSTREAM:-}" php_any="-" php_safe="-" vers=""
  if [ -n "$php_up" ]; then
    vers="$(upstream_versions_by_field "$php_up" 2>/dev/null || true)"
  elif recipe_has_func "$bundle/php.sh" versions; then
    vers="$(bash "$bundle/php.sh" versions 2>/dev/null || true)"
  fi
  if [ -n "$vers" ]; then
    php_any="$(printf '%s\n' "$vers" | version_max)"
    php_safe="$(printf '%s\n' "$vers" | version_safe_max "$php_cur")"
    [ -n "$php_any" ] || php_any="-"
    [ -n "$php_safe" ] || php_safe="-"
  else
    unknown=1
  fi
  printf 'php\tcore\t%s\t%s\t%s\n' "$php_cur" "$php_any" "$php_safe" >> "$rows"

  # ---- 依赖 / 扩展 / 工具 ----
  local kind sh name cur x
  if [ "$all" = 1 ]; then
    for kind in deps ext tools; do
      for sh in "$bundle/$kind"/*.sh; do
        [ -f "$sh" ] || continue
        name="$(recipe_name_of "$sh")"
        cur="$(recipe_version "$sh")"
        [ -n "$cur" ] || continue
        outdated_module_row "$name" "$(recipe_kind_of "$sh")" "$sh" "$cur" >> "$rows" || unknown=1
      done
    done
  else
    # 当前树实际启用的扩展 -> needs/tools 闭包
    export SELECTED_EXTS="$(tree_exts "$ver")"
    builtin_apply_switches
    load_tree_ext_opts "$(tree_of "$ver")"
    apply_ext_opts
    local -a queue=()
    local seen=" "
    for x in $SELECTED_EXTS; do queue+=("$x"); done
    while [ ${#queue[@]} -gt 0 ]; do
      name="${queue[0]}"
      queue=("${queue[@]:1}")
      case "$seen" in *" $name "*) continue ;; esac
      seen+="$name "
      sh="$(module_sh "$name")"
      kind="$(recipe_kind_of "$sh")"
      cur="$(recipe_version "$sh")"
      if [ -n "$cur" ]; then
        outdated_module_row "$name" "$kind" "$sh" "$cur" >> "$rows" || unknown=1
      fi
      for x in $(module_needs "$name"); do queue+=("$x"); done
    done
  fi

  if [ "$json" = 1 ]; then
    local first=1 n k c a s
    printf '{'
    while IFS=$'\t' read -r n k c a s; do
      [ "$first" = 1 ] || printf ','
      first=0
      printf '"%s":{"kind":"%s","current":"%s","latest":"%s","safe":"%s"}' \
        "$n" "$k" "$c" "$a" "$s"
    done < "$rows"
    printf '}\n'
  else
    printf '%-12s %-8s %-12s %-12s %-12s\n' 模块 类型 已装 安全更新 最新
    local n k c a s
    while IFS=$'\t' read -r n k c a s; do
      printf '%-12s %-8s %-12s %-12s %-12s\n' "$n" "$k" "$c" "$s" "$a"
    done < "$rows"
    if [ "$all" = 1 ]; then
      printf '\n范围：bundle 里所有已接入的模块（--all）\n' >&2
    else
      printf '\n范围：只提示当前 php.ini 配置了的扩展及依赖（--all 查看全部）\n' >&2
    fi
    [ "$unknown" = 0 ] ||
      printf '有模块查不到上游版本（网络或来源问题），结果不完整。\n' >&2
  fi

  rm -f "$rows"
  [ "$unknown" = 0 ] || return 3
}

cmd_add() {
  [ "${1:-}" = "ext" ] || die "用法: phprt add ext <名字> --shared|--static"
  local name="${2:-}"
  [ -n "$name" ] || die "用法: phprt add ext <名字> --shared|--static"
  shift 2 || true

  local mode="" a chroot_mode="" env_set=0
  local downloads_only=0 print_urls=0 opt_spec=""
  while [ $# -gt 0 ]; do
    a="$1"
    case "$a" in
      --shared)
        [ -z "$mode" ] || [ "$mode" = shared ] || die "--shared 和 --static 不能同时用"
        mode=shared; shift ;;
      --static)
        [ -z "$mode" ] || [ "$mode" = static ] || die "--shared 和 --static 不能同时用"
        mode=static; shift ;;
      --opt) opt_spec="${opt_spec:+$opt_spec }${2:-}"; shift 2 ;;
      --opt=*) opt_spec="${opt_spec:+$opt_spec }${1#*=}"; shift ;;
      --downloads|--download|--predownload) downloads_only=1; shift ;;
      --downloads-print-urls|--print-urls|--urls) print_urls=1; shift ;;
      --download-jobs) export PHPRT_DOWNLOAD_JOBS="${2:-}"; shift 2 ;;
      --download-jobs=*) export PHPRT_DOWNLOAD_JOBS="${1#*=}"; shift ;;
      --download-host-jobs) export PHPRT_DOWNLOAD_HOST_JOBS="${2:-}"; shift 2 ;;
      --download-host-jobs=*) export PHPRT_DOWNLOAD_HOST_JOBS="${1#*=}"; shift ;;
      --timeout) export PHPRT_DOWNLOAD_TIMEOUT="${2:-}"; shift 2 ;;
      --timeout=*) export PHPRT_DOWNLOAD_TIMEOUT="${1#*=}"; shift ;;
      --ui) export PHPRT_UI="${2:-}"; shift 2 ;;
      --ui=*) export PHPRT_UI="${1#*=}"; shift ;;
      --proxy)  export https_proxy="${2:-}" http_proxy="${2:-}" HTTPS_PROXY="${2:-}" HTTP_PROXY="${2:-}"; shift 2 ;;
      --proxy=*) export https_proxy="${1#*=}" http_proxy="${1#*=}" HTTPS_PROXY="${1#*=}" HTTP_PROXY="${1#*=}"; shift ;;
      --chroot) chroot_mode=chroot; env_set=1; shift ;;
      --host)   chroot_mode=host; env_set=1; shift ;;
      --yes|-y) shift ;;
      -h|--help) usage; return 0 ;;
      *) die "未知参数: $a（add ext 接受 --shared / --static / --opt / --downloads / --downloads-print-urls / --chroot / --host）" ;;
    esac
  done
  [ "$env_set" = 1 ] && buildroot_set_env "$chroot_mode"

  local ver tree bundle sh types dflt cur extdir ld list m present=0 tree_link=""
  ver="$(current_version)" || die "还没有 current 版本"
  export PHPRT_PHP_VERSION="$ver"
  phprt_set_build_root "$ver"
  tree="$(tree_of "$ver")"
  bundle="$(bundle_for "$(series_of "$ver")")" || die "缺少 bundle: php$(series_of "$ver")"
  load_bundle_conf "$bundle" "$(series_of "$ver")"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" DEPS="$tree/deps"
  load_tree_ext_opts "$tree"

  tree_link="$(link_mode_get "$tree" 2>/dev/null || true)"
  sh="$(ext_recipe_sh "$name")" || die "未知扩展: $name$(ext_name_hint "$name")"
  types="$(recipe_types "$sh")"
  dflt="$(recipe_default_mode "$sh")"

  # 契约读出来的方式提示；没带参数就停在这里，让用户自己选
  if [ -z "$mode" ]; then
    printf '%s 支持：%s\n' "$name" "$(printf '%s' "$types" | tr ' ' ',')" >&2
    printf '  契约默认：%s\n' "$dflt" >&2
    case " $types " in
      *" shared "*) printf '  --shared  编译成 %s.so，不重编 PHP，可随时停用\n' "$name" >&2 ;;
    esac
    case " $types " in
      *" static "*) printf '  --static  编入 PHP 二进制，需要重新编译 PHP\n' >&2 ;;
    esac
    die "请重新执行并加 --shared 或 --static"
  fi
  mode_supported "$sh" "$mode" ||
    die "$name 不支持 $mode（type=$(printf '%s' "$types" | tr ' ' ',')）"
  if [ "$tree_link" = static ] && [ "$mode" = shared ]; then
    die "当前树是 --static-all（依赖只有静态库），不能加 shared 扩展；先 phprt install 切回普通安装"
  fi

  extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
  ld="$tree/lib:$(ls -d "$tree"/deps/*/lib 2>/dev/null | tr '\n' ':')"
  for m in $(ext_modules "$name"); do
    if { [ -n "$extdir" ] && [ -f "$extdir/$m.so" ]; } ||
       LD_LIBRARY_PATH="$ld" "$tree/bin/php" -m 2>/dev/null | grep -cix "$m" >/dev/null; then
      present=1
    fi
  done
  cur="$(ext_current_mode "$tree" "$name" 2>/dev/null || true)"

  # 现有扩展的实际模式 + 本次目标
  list="$(tree_exts "$ver")"
  case " $list " in *" $name "*) ;; *) list="$list $name" ;; esac
  local mode_env="" e em
  for e in $list; do
    if [ "$e" = "$name" ]; then
      em="$mode"
    else
      em="$(mode_of "$e" 2>/dev/null || true)"
      [ -n "$em" ] || em="$(recipe_default_mode "$(ext_recipe_sh "$e")")"
    fi
    mode_env="$mode_env${mode_env:+ }$e=$em"
  done
  export PHPRT_EXT_MODES="$mode_env" SELECTED_EXTS="$list"
  if [ -n "$opt_spec" ]; then
    validate_ext_opts "$list" "$opt_spec"
    # 只改这次点名的扩展；其它扩展沿用树里已有的 .build-opts
    export PHPRT_EXT_OPTS="$(ext_opts_merge "${PHPRT_EXT_OPTS:-}" "$opt_spec")"
  fi

  if [ "$print_urls" = 1 ]; then
    source_plan "$list"
    source_plan_print_urls
    return 0
  fi
  if [ "$downloads_only" = 1 ]; then
    prefetch_sources "$list" 0 || return 1
    return 0
  fi

  # 没带 --opt 且已经在同一种模式下：只补 ini，不重编。
  # 带 --opt 时即使已安装也要重编——否则没法改子选项。
  if [ "$cur" = "$mode" ] && [ "$present" = 1 ] && [ -z "$opt_spec" ]; then
    enable_ext "$name"
    ok "$name 已经是 $mode 方式"
    return 0
  fi

  if [ "$mode" = shared ] && [ "$cur" = static ]; then
    warn "$name 当前是 static，改成 shared 需要重编 PHP（先去掉静态版，再编 .so）"
  elif [ "$mode" = static ] && [ "$cur" = shared ]; then
    warn "$name 当前是 shared，改成 static 需要重编 PHP"
  elif [ "$mode" = static ]; then
    warn "$name 需要重新 configure 并重编 PHP（依赖仍复用）"
  else
    log "编译扩展 $name（shared）"
  fi

  prefetch_sources "$list" 0 || die "源码下载失败；未进入构建"

  ensure_base_files
  need_cmds flock
  exec 9>"$BUILD_ROOT/.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"

  if ! PHPRT_EXT_MODES="$mode_env" \
       PHPRT_SEED="${PHPRT_SEED:-}" \
       PHPRT_FORCE_PHP=0 \
       PHPRT_CLEAN_SRC=0 \
       PHPRT_NO_DOWNLOAD=1 \
       run_build_env bash "$PHPRT_RUNNER/build.sh" \
         --bundle "$bundle" \
         --tree "$tree" \
         --exts "$list"; then
    die "扩展 $name 构建失败"
  fi

  # enable 失败时不要把树标成 ok，否则下次 add 会以为已经装好
  printf 'activating\n' > "$tree/BUILD-STATUS"
  TREE="$tree" mode_set "$name" "$mode"
  enable_ext "$name"
  printf 'ok\n' > "$tree/BUILD-STATUS"
}

cmd_remove() {
  [ "${1:-}" = "ext" ] || die "用法: phprt remove ext <名字>"
  local name="${2:-}"
  [ -n "$name" ] || die "用法: phprt remove ext <名字>"
  local ver tree bundle
  ver="$(current_version)" || die "还没有 current 版本"
  export PHPRT_PHP_VERSION="$ver"
  tree="$(tree_of "$ver")"
  bundle="$(bundle_for "$(series_of "$ver")")" || die "缺少 bundle: php$(series_of "$ver")"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" DEPS="$tree/deps"
  # 只停用；状态以 php -m 为准，不需要另外记账
  disable_ext "$name"
}

cmd_verify() {
  require_project() { [ -d "$PHPRT_HOME" ] || die "还没有 .phprt"; }
  require_project
  local ver tree bundle
  ver="$(current_version)" || die "还没有 current 版本"
  export PHPRT_PHP_VERSION="$ver"
  tree="$(tree_of "$ver")"
  bundle="$(bundle_for "$(series_of "$ver")")" || die "缺少 bundle: php$(series_of "$ver")"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" DEPS="$tree/deps"
  [ -x "$tree/bin/php" ] || die "缺少 $tree/bin/php"
  [ -x "$PHPRT_PROJECT/myphp" ] || die "缺少 $PHPRT_PROJECT/myphp"

  log "1) PHP"
  ( cd "$PHPRT_PROJECT" && ./myphp -v | head -1 | sed 's/^/    /' )

  log "2) 已启用的扩展（以 php -m 的实际加载为准）"
  local e m mods ini loaded err bad_ini="" bad_mods=""
  loaded="$(tree_php "$ver" -m 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  for e in $(tree_exts "$ver"); do
    for m in $(ext_modules "$e"); do
      if printf '%s\n' "$loaded" | grep -cix "$m" >/dev/null; then
        printf '    ✓ %s\n' "$m"
      else
        bad_mods+="$e: 声明的模块 $m 没有出现在 php -m"$'\n'
      fi
    done
  done
  # conf.d 声明了但没加载的（.so 缺失、路径错、依赖缺）直接报出来
  for ini in "$tree"/etc/conf.d/*.ini; do
    [ -f "$ini" ] || continue
    for m in $(sed -n 's/^[[:space:]]*extension[[:space:]]*=[[:space:]]*"\?\([^"/]*\)\.so"\?.*/\1/p' "$ini"); do
      printf '%s\n' "$loaded" | grep -cix "$m" >/dev/null ||
        bad_ini+="$ini: $m 没有加载"$'\n'
    done
  done
  err="$(tree_php "$ver" -m 2>&1 >/dev/null || true)"
  if [ -n "$bad_mods" ] || [ -n "$bad_ini" ] || [ -n "$err" ]; then
    [ -n "$bad_mods" ] && printf '    ✗ recipe 声明与 php -m 不一致:\n%s' "$bad_mods"
    [ -n "$bad_ini" ] && printf '    ✗ 扩展声明了但没有加载:\n%s' "$bad_ini"
    [ -n "$err" ] && printf '    ✗ PHP 加载扩展时报错:\n%s\n' "$err"
    return 1
  fi

  log "3) 动态库（安装根外只允许 glibc 等基础库）"
  local bad="" f
  for f in "$tree"/bin/* "$tree"/lib/php/extensions/*/*.so; do
    [ -f "$f" ] || continue
    [ -L "$f" ] && continue
    readelf -h "$f" >/dev/null 2>&1 || continue
    local one=""
    one="$(ldd "$f" 2>/dev/null | awk '$3 ~ /^\// {print $3}' \
      | grep -Ev '/(libc|libm|libpthread|libdl|librt|libresolv|libcrypt|libgcc_s|libstdc\+\+)\.so' \
      | grep -v "^$PHPRT_HOME/" || true)"
    [ -n "$one" ] && bad+="$one"$'\n'
  done
  if [ -n "$bad" ]; then
    printf '    ✗ 借用了安装根外的库:\n%s' "$bad"
    return 1
  fi
  ok "php 与扩展没有从安装根外借库"

  if [ "$(link_mode_get "$tree" 2>/dev/null || true)" = static ]; then
    log "3b) 静态链接（bin/php 只允许 glibc）"
    local needed n bad_static=""
    needed="$(readelf -d "$tree/bin/php" 2>/dev/null |
      awk -F'[][]' '/\(NEEDED\)/{print $2}')"
    for n in $needed; do
      case "$n" in
        libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|\
        libresolv.so.*|libcrypt.so.*|libutil.so.*|libnsl.so.*|\
        libanl.so.*|libmvec.so.*|ld-linux*) ;;
        *) bad_static+="$n"$'\n' ;;
      esac
    done
    for f in "$tree"/lib/php/extensions/*/*.so; do
      [ -e "$f" ] && bad_static+="$f（静态链接下不应有共享扩展）"$'\n'
    done
    if [ -n "$bad_static" ]; then
      printf '    ✗ 静态链接不完整:\n%s' "$bad_static"
      return 1
    fi
    ok "bin/php 只依赖 glibc，且没有共享扩展"
  fi

  if [ "$(lto_mode_get "$tree" 2>/dev/null || true)" = 1 ]; then
    log "3c) LTO（PHP 和依赖）"
    local bi="$tree/BUILD-INFO"
    if [ ! -f "$bi" ] ||
       ! grep -qx 'lto_php=1' "$bi" ||
       ! grep -qx 'lto_deps=1' "$bi"; then
      printf '    ✗ BUILD-INFO 没有记录 lto_php=1 / lto_deps=1\n'
      return 1
    fi
    if readelf -S "$tree/bin/php" 2>/dev/null | grep -c '\.gnu\.lto' >/dev/null; then
      printf '    ✗ bin/php 仍残留 .gnu.lto 段\n'
      return 1
    fi
    ok "PHP 和依赖都按 LTO 构建"
  fi

  log "4) RPATH（只允许 \$ORIGIN 相对路径）"
  local rp="" abs=""
  for f in "$tree"/bin/* "$tree"/lib/*.so* "$tree"/lib/php/extensions/*/*.so; do
    [ -f "$f" ] || continue
    [ -L "$f" ] && continue
    readelf -h "$f" >/dev/null 2>&1 || continue
    rp="$(readelf -d "$f" 2>/dev/null | awk -F'[][]' '/\(RPATH\)|\(RUNPATH\)/{print $2}' | tr '\n' ':')"
    case "$rp" in
      "") ;;
      *'$ORIGIN'*) ;;
      *) abs+="$f: $rp"$'\n' ;;
    esac
  done
  if [ -n "$abs" ]; then
    printf '    ✗ 存在绝对路径 RPATH:\n%s' "$abs"
    return 1
  fi
  ok "RPATH 全部是相对路径"

  log "5) 配置路径（树内标准位置）"
  local loaded scanenv
  loaded="$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo php_ini_loaded_file();' )"
  if [ "$(readlink -f "${loaded:-/nonexistent}")" != "$(readlink -f "$tree/etc/php.ini")" ]; then
    printf '    ✗ php.ini 不是 %s/etc/php.ini（实际: %s）\n' "$tree" "${loaded:-无}"
    return 1
  fi
  scanenv="$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)getenv("PHP_INI_SCAN_DIR");' )"
  if [ "$(readlink -f "${scanenv:-/nonexistent}")" != "$(readlink -f "$tree/etc/conf.d")" ]; then
    printf '    ✗ 扫描目录不是 %s/etc/conf.d（实际: %s）\n' "$tree" "${scanenv:-空}"
    return 1
  fi
  ok "php.ini 与 conf.d 都在版本目录里"

  log "6) CA（只有一份，随项目移动）"
  local cert="$tree/etc/ssl/cert.pem" has_ssl=0 has_curl=0 php_cafile="" php_curlca=""
  if [ ! -s "$cert" ]; then
    printf '    ✗ 缺少 %s\n' "$cert"
    return 1
  fi
  ( cd "$PHPRT_PROJECT" && ./myphp -r 'exit(extension_loaded("openssl")?0:1);' ) && has_ssl=1
  ( cd "$PHPRT_PROJECT" && ./myphp -r 'exit(extension_loaded("curl")?0:1);' ) && has_curl=1
  if [ "$has_ssl" = 1 ]; then
    php_cafile="$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)ini_get("openssl.cafile");' )"
    if [ -z "$php_cafile" ] || [ ! -s "$php_cafile" ] ||
       [ "$(readlink -f "$php_cafile")" != "$(readlink -f "$cert")" ]; then
      printf '    ✗ openssl.cafile 不是 %s（实际: %s）\n' "$cert" "${php_cafile:-空}"
      return 1
    fi
    ok "openssl.cafile -> 树内 cert.pem"
  fi
  if [ "$has_curl" = 1 ]; then
    php_curlca="$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)ini_get("curl.cainfo");' )"
    if [ "$(readlink -f "${php_curlca:-/nonexistent}")" != "$(readlink -f "${php_cafile:-$cert}")" ]; then
      printf '    ✗ curl.cainfo 与树内 CA 不一致（实际: %s）\n' "${php_curlca:-空}"
      return 1
    fi
    ok "curl.cainfo -> 同一份 CA"
  fi
  if [ "$has_ssl" = 1 ] || [ "$has_curl" = 1 ]; then
    if ( cd "$PHPRT_PROJECT" && ./myphp -r '$c=stream_context_create(["http"=>["timeout"=>10]]); $s=@file_get_contents("https://www.php.net/",false,$c); exit($s===false?1:0);' ); then
      ok "HTTPS 请求可用"
    else
      warn "HTTPS 连通性未验证（可能离线）"
    fi
  fi
  if [ "$has_curl" = 1 ]; then
    if ( cd "$PHPRT_PROJECT" && ./myphp -r '$c=curl_init("https://www.php.net/"); curl_setopt_array($c,[CURLOPT_RETURNTRANSFER=>1,CURLOPT_TIMEOUT=>10]); $r=curl_exec($c); exit($r===false?1:0);' ); then
      ok "PHP curl 也走同一个 CA"
    else
      warn "curl 连通性未验证（可能离线）"
    fi
  fi

  log "7) wheel 完整性（sha256 / SONAME / singleton 组 / 无残留原始名）"
  if wheel_integrity_check "$tree"; then
    ok "wheel 完整性通过"
  else
    return 1
  fi
}

main() {
  local cmd="${1:-}"
  [ $# -gt 0 ] && shift
  # 旧版 project.conf 已废弃：扩展状态以 php -m 为准。
  # v1 项目（.phprt/bin 存在）还在用这个文件，不能动；只清理 v2 项目里的残留。
  if [ -f "$PHPRT_HOME/project.conf" ] && [ ! -d "$PHPRT_HOME/bin" ]; then
    rm -f "$PHPRT_HOME/project.conf"
    warn "已删除旧版 .phprt/project.conf（扩展状态改为从 php -m 读取）"
  fi
  case "$cmd" in
    install)  cmd_install "$@" ;;
    download) cmd_download "$@" ;;
    outdated) cmd_outdated "$@" ;;
    update)   cmd_update "$@" ;;
    use)      cmd_use "$@" ;;
    show)     cmd_show "$@" ;;
    current)  cmd_current ;;
    list|ls)  cmd_list ;;
    select)   cmd_select "$@" ;;
    ext)      cmd_ext "$@" ;;
    add)      cmd_add "$@" ;;
    remove|rm) cmd_remove "$@" ;;
    verify)   cmd_verify ;;
    ""|-h|--help) usage ;;
    *)        die "未知命令: $cmd（--help 查看用法）" ;;
  esac
}
