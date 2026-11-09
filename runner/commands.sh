#!/usr/bin/env bash
# phprt：命令实现

usage() {
  cat <<'EOF'
phprt —— 为项目安装一套隔离的 PHP 运行环境

用法:
  phprt install <系列|版本> [选项]
      系列写法（8.5）用 bundle.conf 的 pin；精确写法（8.5.12）装指定版本。
      --ext "a b"   选择扩展，默认用 bundle.conf 的 DEFAULT_EXTS
      --plan        只打印构建计划（依赖、PHP、扩展），不构建
      --recompile   强制重编 PHP（依赖和扩展签名保留）
      --clean-src   清空已解压源码，重新解压后再编
      --yes         对提示一律回答“是”

  phprt add ext <名字>
      在当前版本树里加一个扩展。PECL 扩展只编它自己；
      builtin 扩展需要重新 configure PHP，会重编 PHP（依赖仍复用）。

  phprt remove ext <名字>
      停用扩展：删掉对应的 conf.d ini。静态编入 PHP 的 builtin
      需要重新 configure 才能去掉。

  phprt use <系列|版本>
      切换 current 软链。系列写法切到该系列已装的最高版本。

  phprt show
      显示当前 PHP 版本、安装路径、php.ini、已启用扩展和 -v 输出。

  phprt current
      只打印当前版本号。

  phprt list
      列出已安装的版本，当前版本前面标 *。

  phprt outdated [--all] [--json] [系列]
      检查上游版本。默认只查当前 php.ini 启用的扩展及其依赖；
      --all 查全部已接入模块；--json 输出机器可读结果。
      退出码：0 查询成功，3 网络或上游不可用。

  phprt verify
      自检：PHP、扩展、动态库、RPATH、配置路径、CA、wheel 完整性。

环境变量:
  PHPRT_REPO      phprt 仓库位置（默认是脚本所在仓库）
  PHPRT_WORK      构建区基目录（默认 <项目>/.phprt）
  OFFLINE=1       不联网（outdated 会直接报告未查询）
  https_proxy / HTTPS_PROXY / http_proxy / HTTP_PROXY
                  查询和下载使用的代理；直连失败会自动尝试

扩展清单由 bundles/php<系列>/{builtin,ext}/*.sh 决定；新增一个 recipe
文件就会出现在可选清单里，不需要改 driver。

示例:
  phprt install 8.5
  phprt install 8.5.12
  phprt install 8.5 --ext "openssl curl zip swoole"
  phprt install 8.5 --plan
  phprt add ext redis
  phprt remove ext redis
  phprt outdated
  phprt outdated --all --json
  phprt use 8.5
  phprt show
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

# 安装 / 重建 PHP。
# 系列写法（8.5）自动解析到该系列最新小版本：非离线先查 php.net，
# 本地已有不旧于目标的树就停止；离线或查不到用 bundle.conf 的 pin。
# 精确写法（8.5.12）直接装指定版本。
cmd_install() {
  local arg="" exts="" recompile=0 assume_yes=0 plan=0 clean_src=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --ext)       exts="${2:-}"; shift 2 ;;
      --ext=*)     exts="${1#*=}"; shift ;;
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
  . "$bundle/bundle.conf"
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
       [ "$recompile" != 1 ] && [ "$plan" != 1 ] && [ -z "$exts" ]; then
      if [ "$imax" = "$concrete" ]; then
        ok "PHP $concrete 已安装，无需重复安装"
      else
        ok "本地已装 PHP $imax（目标 $concrete 更旧），不降级"
      fi
      [ "$imax" = "$(current_version 2>/dev/null || true)" ] ||
        printf '    切换: phprt use %s\n' "$series"
      return 0
    fi
  fi

  local tree complete=0 status=""
  tree="$(tree_of "$concrete")"

  if [ -d "$tree" ]; then
    status="$(cat "$tree/BUILD-STATUS" 2>/dev/null || true)"
    # 新树必须看到 ok；老版本没有状态文件时按已有 bin/php 视为完成
    if [ -x "$tree/bin/php" ] && {
         [ "$status" = ok ] ||
         { [ ! -f "$tree/BUILD-STATUS" ] && [ ! -f "$tree/.build-exts" ]; }
       }; then
      complete=1
    fi
  fi

  # 精确版已装齐：停止（系列版走到这里说明目标高于已装最高版，不会重入）
  if [ "$complete" = 1 ] && [ "$exact" = 1 ] &&
     [ "$recompile" != 1 ] && [ "$plan" != 1 ] && [ -z "$exts" ]; then
    ok "PHP $concrete 已安装"
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
    if [ -z "$exts" ] && [ -t 0 ] && [ "$assume_yes" != 1 ]; then
      printf '\n可选扩展: %s\n默认: %s\n' \
        "$(BUNDLE="$bundle" all_ext_names | paste -sd' ' -)" "${DEFAULT_EXTS:-}"
      local ans=""
      read -r -p "编译哪些扩展 [${DEFAULT_EXTS:-}]: " ans || ans=""
      exts="${ans:-${DEFAULT_EXTS:-}}"
    fi
    [ -n "$exts" ] || exts="${DEFAULT_EXTS:-}"
  fi
  [ "$exts" = "none" ] && exts=""
  exts="$(normalize_exts "$exts")"

  # bundle 声明的最小必需扩展（REQUIRED_EXTS，可选）
  local req
  for req in ${REQUIRED_EXTS:-}; do
    case " $exts " in
      *" $req "*) ;;
      *) die "PHP $concrete 必须包含扩展 $req（bundle.conf 的 REQUIRED_EXTS）" ;;
    esac
  done

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
       bash "$PHPRT_RUNNER/build.sh" \
         --bundle "$build_bundle" \
         --tree "$tree" \
         --exts "$exts"; then
    die "构建失败；已完成的依赖、PHP 和扩展会保留，重试会从失败处继续"
  fi

  if [ ! -x "$tree/bin/php" ]; then
    die "构建结束但找不到 $tree/bin/php"
  fi

  activate_tree "$concrete" "$exts"
  ok "PHP $concrete 就绪 -> $tree"
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

cmd_show() {
  local ver tree bundle
  ver="$(current_version)" || die "还没有安装 PHP（先跑 phprt install <系列>）"
  tree="$(tree_of "$ver")"
  printf 'PHP %s\n' "$ver"
  printf '    路径: %s\n' "$tree"
  printf '    php.ini: %s\n' "$tree/etc/php.ini"
  bundle="$(bundle_for "$(series_of "$ver")")" || true
  if [ -n "$bundle" ]; then
    export BUNDLE="$bundle"
    printf '    已启用: %s\n' "$(tree_exts "$ver")"
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

# 单个模块的检查结果：name<TAB>kind<TAB>current<TAB>latest<TAB>safe
# 查不到上游时输出 -，返回 1，让调用方统计“结果不完整”。
outdated_module_row() {
  local name="$1" kind="$2" sh="$3" cur="$4" vers latest safe
  if [ "$(recipe_field "$sh" upstream)" = none ] &&
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

  local bundle
  bundle="$(bundle_for "${series:-$(series_of "$ver")}")" ||
    die "找不到 bundle: php${series:-$(series_of "$ver")}"
  export BUNDLE="$bundle"
  # shellcheck disable=SC1090
  . "$bundle/bundle.conf"

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
  local kind sh name var cur x
  if [ "$all" = 1 ]; then
    for kind in deps ext tools; do
      for sh in "$bundle/$kind"/*.sh; do
        [ -f "$sh" ] || continue
        var="$(recipe_field "$sh" version)"
        [ -n "$var" ] || continue
        name="$(recipe_field "$sh" name)"
        cur="${!var:-}"
        [ -n "$cur" ] || continue
        outdated_module_row "$name" "$(recipe_kind_of "$sh")" "$sh" "$cur" >> "$rows" || unknown=1
      done
    done
  else
    # 当前树实际启用的扩展 -> needs/tools 闭包
    export SELECTED_EXTS="$(tree_exts "$ver")"
    builtin_apply_switches
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
      var="$(recipe_field "$sh" version)"
      if [ -n "$var" ]; then
        cur="${!var:-}"
        [ -n "$cur" ] && { outdated_module_row "$name" "$kind" "$sh" "$cur" >> "$rows" || unknown=1; }
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
  [ "${1:-}" = "ext" ] || die "用法: phprt add ext <名字>"
  local name="${2:-}"
  [ -n "$name" ] || die "用法: phprt add ext <名字>"

  # 构建锁：和 install 互斥；锁在 current 版本的 build-php-<版本> 里
  need_cmds flock
  local ver tree bundle kind list mods m extdir ld missing=0
  ver="$(current_version)" || die "还没有 current 版本"
  export PHPRT_PHP_VERSION="$ver"
  phprt_set_build_root "$ver"
  ensure_base_files
  exec 9>"$BUILD_ROOT/.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"
  tree="$(tree_of "$ver")"
  bundle="$(bundle_for "$(series_of "$ver")")" || die "缺少 bundle: php$(series_of "$ver")"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" DEPS="$tree/deps"

  if [ -f "$bundle/builtin/$name.sh" ]; then
    kind=builtin
  elif [ -f "$bundle/ext/$name.sh" ]; then
    kind=ext
  else
    die "未知扩展: $name"
  fi

  extdir="$(ext_dir_of "$tree" 2>/dev/null || true)"
  ld="$tree/lib:$(ls -d "$tree"/deps/*/lib 2>/dev/null | tr '\n' ':')"
  mods="$(ext_modules "$name")"
  for m in $mods; do
    if [ -f "$extdir/$m.so" ]; then
      continue
    elif LD_LIBRARY_PATH="$ld" "$tree/bin/php" -m 2>/dev/null | grep -qix "$m"; then
      continue
    else
      missing=1
    fi
  done

  # 现有扩展从 PHP 实际加载状态推出来，保证 builtin 编译开关不被误关
  list="$(tree_exts "$ver")"
  if [ "$missing" = 1 ]; then
    if [ "$kind" = builtin ]; then
      warn "扩展 $name 需要重新 configure PHP，会重编 PHP（依赖仍复用）"
    else
      log "编译扩展 $name"
    fi
    if ! SELECTED_EXTS="$list $name" \
         PHPRT_SEED="${PHPRT_SEED:-}" \
         PHPRT_FORCE_PHP=0 \
         PHPRT_CLEAN_SRC=0 \
         bash "$PHPRT_RUNNER/build.sh" \
           --bundle "$bundle" \
           --tree "$tree" \
           --exts "$list $name"; then
      die "扩展 $name 构建失败"
    fi
  fi

  enable_ext "$name"
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
  local e m mods ini loaded err bad_ini=""
  loaded="$(tree_php "$ver" -m 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  for e in $(tree_exts "$ver"); do
    for m in $(ext_modules "$e"); do
      printf '    ✓ %s\n' "$m"
    done
  done
  # conf.d 声明了但没加载的（.so 缺失、路径错、依赖缺）直接报出来
  for ini in "$tree"/etc/conf.d/*.ini; do
    [ -f "$ini" ] || continue
    for m in $(sed -n 's/^[[:space:]]*extension[[:space:]]*=[[:space:]]*"\?\([^"/]*\)\.so"\?.*/\1/p' "$ini"); do
      printf '%s\n' "$loaded" | grep -qx "$m" ||
        bad_ini+="$ini: $m 没有加载"$'\n'
    done
  done
  err="$(tree_php "$ver" -m 2>&1 >/dev/null || true)"
  if [ -n "$bad_ini" ] || [ -n "$err" ]; then
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
    outdated) cmd_outdated "$@" ;;
    use)      cmd_use "$@" ;;
    show)     cmd_show ;;
    current)  cmd_current ;;
    list|ls)  cmd_list ;;
    add)      cmd_add "$@" ;;
    remove|rm) cmd_remove "$@" ;;
    verify)   cmd_verify ;;
    ""|-h|--help) usage ;;
    *)        die "未知命令: $cmd（--help 查看用法）" ;;
  esac
}
