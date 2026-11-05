#!/usr/bin/env bash
# phprt：命令实现

usage() {
  cat <<'EOF'
phprt

用法:
  phprt set <版本> [--ext a,b] [--recompile] [--clean-src] [--plan] [--yes]
  phprt current
  phprt list
  phprt add ext <名字>
  phprt remove ext <名字>
  phprt verify

示例:
  phprt set 8.5.12
  phprt set 8.6.0-dev --ext sqlite
  phprt set 8.5.12 --recompile --ext sqlite --yes
EOF
}

repo_root() {
  printf '%s\n' "${PHPRT_REPO:-}"
}

# 取版本 bundle。仓库里有时以仓库为准并同步到项目（仓库只当模板），
# 没有仓库时用项目内的本地副本。
bundle_for() {
  local version="$1" repo dst
  dst="$PHPRT_HOME/bundles/php-$version"
  repo="$(repo_root)"
  if [ -n "$repo" ] && [ -d "$repo/bundles/php-$version" ]; then
    ensure_base_files
    copy_runner "$repo"
    rm -rf "$dst"
    mkdir -p "$PHPRT_HOME/bundles"
    cp -a "$repo/bundles/php-$version" "$dst"
    printf '%s\n' "$dst"
    return 0
  fi
  [ -d "$dst" ] || return 1
  printf '%s\n' "$dst"
}

normalize_exts() {
  printf '%s' "$1" | tr ',' ' ' | tr -s ' '
}

cmd_set() {
  local version="" exts="" recompile=0 assume_yes=0 plan=0 clean_src=0
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
      *)           version="$1"; shift ;;
    esac
  done
  [ -n "$version" ] || die "用法: phprt set <版本>"

  local bundle tree complete=0 status=""
  tree="$(tree_of "$version")"

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

  # 已完成且没有显式改扩展：直接切换，不重编
  # （仍要同步一次 runner：仓库里改了模板/逻辑，老项目也得跟上）
  if [ "$complete" = 1 ] && [ "$recompile" != 1 ] && [ -z "$exts" ] && [ "$plan" != 1 ]; then
    switch_current "$version"
    ensure_base_files
    if [ -n "$(repo_root)" ]; then
      copy_runner "$(repo_root)"
      # runner 刚被替换成最新版：重新载入，老项目也能用上新加的修复函数
      # shellcheck disable=SC1091
      . "$PHPRT_HOME/bin/runner/common.sh"
    fi
    ensure_tree_config "$tree"
    ensure_tree_ssl "$tree"
    write_project_conf "$version" "$(project_exts)"
    write_myphp
    ok "切换到 PHP $version（使用已有树，未重编）"
    return 0
  fi

  [ -d "$tree" ] && [ "$complete" != 1 ] &&
    warn "上次构建未完成，继续；已完成的依赖和 PHP 会跳过"

  bundle="$(bundle_for "$version")" ||
    die "找不到 bundle: php-$version（需要 phprt 仓库或 PHPRT_REPO）"

  # shellcheck disable=SC1090
  . "$bundle/bundle.conf"

  if [ -z "$exts" ]; then
    if [ -f "$tree/BUILD-STATUS" ] && [ "$(cat "$tree/BUILD-STATUS")" != ok ] &&
       [ -f "$tree/.build-exts" ]; then
      # 上次没装完：沿用那次的选择
      exts="$(cat "$tree/.build-exts")"
    else
      exts="$(project_exts)"
    fi
    if [ -z "$exts" ] && [ -f "$tree/BUILD-PLAN" ]; then
      exts="$(sed -n 's/^EXTENSIONS="\(.*\)"$/\1/p' "$tree/BUILD-PLAN" | head -1)"
    fi
    if [ -z "$exts" ] && [ -t 0 ] && [ "$assume_yes" != 1 ]; then
      printf '\n可选扩展: %s\n默认: %s\n' "${SUPPORTED_EXTS:-}" "${DEFAULT_EXTS:-}"
      local ans=""
      read -r -p "编译哪些扩展 [${DEFAULT_EXTS:-}]: " ans || ans=""
      exts="${ans:-${DEFAULT_EXTS:-}}"
    fi
    [ -n "$exts" ] || exts="${DEFAULT_EXTS:-}"
  fi
  [ "$exts" = "none" ] && exts=""
  exts="$(normalize_exts "$exts")"

  # bundle 声明的最小必需扩展（例如 7.4 的 libxml2 是内核强制依赖）
  local req
  for req in ${REQUIRED_EXTS:-}; do
    case " $exts " in
      *" $req "*) ;;
      *) die "PHP $version 必须包含扩展 $req（bundle.conf 的 REQUIRED_EXTS）" ;;
    esac
  done

  # 明确的 --recompile 只强制重编 PHP，依赖和扩展签名保留
  local force_php=0
  [ "$recompile" = 1 ] && force_php=1

  log "构建 PHP $version"
  printf '    扩展: %s\n' "${exts:-（无）}"
  ensure_base_files

  local build_bundle="$bundle"

  if [ "$plan" = 1 ]; then
    PHPRT_RUNNER="$PHPRT_HOME/bin/runner" \
    SELECTED_EXTS="$exts" \
    PHPRT_SEED="${PHPRT_SEED:-}" \
    PHPRT_PLAN=1 \
      bash "$PHPRT_HOME/bin/runner/build.sh" \
        --bundle "$build_bundle" \
        --tree "$tree" \
        --exts "$exts" --plan
    return 0
  fi

  if ! PHPRT_RUNNER="$PHPRT_HOME/bin/runner" \
       SELECTED_EXTS="$exts" \
       PHPRT_SEED="${PHPRT_SEED:-}" \
       PHPRT_FORCE_PHP="$force_php" \
       PHPRT_CLEAN_SRC="$clean_src" \
       bash "$PHPRT_HOME/bin/runner/build.sh" \
         --bundle "$build_bundle" \
         --tree "$tree" \
         --exts "$exts"; then
    die "构建失败；已完成的依赖、PHP 和扩展会保留，重试会从失败处继续"
  fi

  if [ ! -x "$tree/bin/php" ]; then
    die "构建结束但找不到 $tree/bin/php"
  fi

  switch_current "$version"
  ensure_base_files
  # 去掉本次未选中的扩展 ini（静态内置无法卸载，但至少不再启用）
  local old
  for old in ${SUPPORTED_EXTS:-}; do
    case " $exts " in
      *" $old "*) ;;
      *) rm -f "$tree/etc/conf.d"/*-"$old".ini ;;
    esac
  done
  local e
  for e in $exts; do
    enable_ext "$e" || warn "扩展 $e 没有启用"
  done
  write_project_conf "$version" "$exts"
  write_myphp
  ok "PHP $version 就绪 -> $tree"
}

cmd_current() {
  local v
  v="$(current_version)" || die "还没有设置 current（先跑 phprt set <版本>）"
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

update_project_exts() {
  local php="$1" list="$2" out="" e
  for e in $list; do
    case " $out " in *" $e "*) continue ;; esac
    out="$out${out:+ }$e"
  done
  write_project_conf "$php" "$out"
}

cmd_add() {
  [ "${1:-}" = "ext" ] || die "用法: phprt add ext <名字>"
  local name="${2:-}"
  [ -n "$name" ] || die "用法: phprt add ext <名字>"
  local ver tree bundle kind list mods m extdir ld missing=0
  ver="$(current_version)" || die "还没有 current 版本"
  tree="$(tree_of "$ver")"
  bundle="$(bundle_for "$ver")" || die "缺少 bundle: php-$ver"

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

  list="$(project_exts)"
  if [ "$missing" = 1 ]; then
    if [ "$kind" = builtin ]; then
      warn "扩展 $name 需要重新 configure PHP，会重编 PHP（依赖仍复用）"
    else
      log "编译扩展 $name"
    fi
    if ! PHPRT_RUNNER="$PHPRT_HOME/bin/runner" \
         SELECTED_EXTS="$list $name" \
         PHPRT_SEED="${PHPRT_SEED:-}" \
         PHPRT_FORCE_PHP=0 \
         PHPRT_CLEAN_SRC=0 \
         bash "$PHPRT_HOME/bin/runner/build.sh" \
           --bundle "$bundle" \
           --tree "$tree" \
           --exts "$list $name"; then
      die "扩展 $name 构建失败"
    fi
  fi

  enable_ext "$name"
  update_project_exts "$ver" "$list $name"
}

cmd_remove() {
  [ "${1:-}" = "ext" ] || die "用法: phprt remove ext <名字>"
  local name="${2:-}"
  [ -n "$name" ] || die "用法: phprt remove ext <名字>"
  local ver list out="" e
  ver="$(current_version)" || die "还没有 current 版本"
  disable_ext "$name"
  list="$(project_exts)"
  for e in $list; do
    [ "$e" = "$name" ] && continue
    out="$out${out:+ }$e"
  done
  write_project_conf "$ver" "$out"
}

cmd_verify() {
  require_project() { [ -d "$PHPRT_HOME" ] || die "还没有 .phprt"; }
  require_project
  local ver tree
  ver="$(current_version)" || die "还没有 current 版本"
  tree="$(tree_of "$ver")"
  [ -x "$tree/bin/php" ] || die "缺少 $tree/bin/php"
  [ -x "$PHPRT_PROJECT/myphp" ] || die "缺少 $PHPRT_PROJECT/myphp"

  log "1) PHP"
  ( cd "$PHPRT_PROJECT" && ./myphp -v | head -1 | sed 's/^/    /' )

  log "2) 已启用的扩展"
  local e m mods
  for e in $(project_exts); do
    mods="$(ext_modules "$e")"
    for m in $mods; do
      if ( cd "$PHPRT_PROJECT" && ./myphp -r "exit(extension_loaded('$m')?0:1);" ); then
        printf '    ✓ %s\n' "$m"
      else
        printf '    ✗ %s 未加载\n' "$m"
        return 1
      fi
    done
  done

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
  local loaded scanenv cafile
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
  cafile="$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)ini_get("openssl.cafile");' )"
  if [ -z "$cafile" ] || [ ! -s "$cafile" ] ||
     [ "$(readlink -f "$cafile")" != "$(readlink -f "$tree/etc/ssl/cert.pem")" ]; then
    printf '    ✗ openssl.cafile 不是 %s/etc/ssl/cert.pem（实际: %s）\n' "$tree" "${cafile:-空}"
    return 1
  fi
  if [ "$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)ini_get("curl.cainfo");' )" != "$cafile" ]; then
    printf '    ✗ curl.cainfo 与 openssl.cafile 不一致\n'
    return 1
  fi
  if [ "$( cd "$PHPRT_PROJECT" && ./myphp -r 'echo (string)ini_get("curl.cainfo");' )" != "$cafile" ]; then
    printf '    ✗ curl.cainfo 与 openssl.cafile 不一致\n'
    return 1
  fi
  if ( cd "$PHPRT_PROJECT" && ./myphp -r '$c=stream_context_create(["http"=>["timeout"=>10]]); $s=@file_get_contents("https://www.php.net/",false,$c); exit($s===false?1:0);' ); then
    ok "HTTPS 请求可用"
  else
    warn "HTTPS 连通性未验证（可能离线）"
  fi
  if ( cd "$PHPRT_PROJECT" && ./myphp -r '$c=curl_init("https://www.php.net/"); curl_setopt_array($c,[CURLOPT_RETURNTRANSFER=>1,CURLOPT_TIMEOUT=>10]); $r=curl_exec($c); exit($r===false?1:0);' ); then
    ok "PHP curl 也走同一个 CA"
  else
    warn "curl 连通性未验证（可能离线）"
  fi
}

main() {
  local cmd="${1:-}"
  [ $# -gt 0 ] && shift
  case "$cmd" in
    set)      cmd_set "$@" ;;
    current)  cmd_current ;;
    list|ls)  cmd_list ;;
    add)      cmd_add "$@" ;;
    remove|rm) cmd_remove "$@" ;;
    verify)   cmd_verify ;;
    ""|-h|--help) usage ;;
    *)        die "未知命令: $cmd（--help 查看用法）" ;;
  esac
}
