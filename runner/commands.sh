#!/usr/bin/env bash
# phprt：命令实现

# shellcheck disable=SC1091
. "${BASH_SOURCE[0]%/*}/update-check.sh"

usage() {
  cat <<'EOF'
phprt

用法:
  phprt set <版本> [--ext a,b] [--recompile] [--clean-src] [--plan] [--yes]
  phprt current
  phprt list
  phprt add ext <名字>
  phprt remove ext <名字>
  phprt update <依赖> <版本>
  phprt outdated
  phprt verify

phprt update 只升级一个依赖库的同系列小版本（如 openssl 3.5.7 -> 3.5.8）：
只重编该依赖并替换 lib/phprt 里的运行时库，消费方不重编、DT_NEEDED 零改写。

phprt outdated 只读检查已安装软件有没有同系列新版本，结果写入
.phprt/cache/update-check/，不下载、不编译、不改 bundle.conf。

示例:
  phprt set 8.5.12
  phprt set 8.6.0-dev --ext sqlite
  phprt set 8.5.12 --recompile --ext sqlite --yes
  phprt update openssl 3.5.8
  phprt update libpng 1.6.51
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

  # 构建锁：和 update / add 互斥，防止并发改同一棵树
  need_cmds flock
  ensure_base_files
  exec 9>"$PHPRT_HOME/.phprt.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"

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

  # 构建锁：和 set / update 互斥
  need_cmds flock
  exec 9>"$PHPRT_HOME/.phprt.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"
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

# 依赖名 -> bundle.conf 里的版本变量
dep_version_var() {
  case "$1" in
    openssl)   printf 'OPENSSL_VERSION' ;;
    zlib)      printf 'ZLIB_VERSION' ;;
    libxml2)   printf 'LIBXML2_VERSION' ;;
    sqlite)    printf 'SQLITE_VERSION' ;;
    curl)      printf 'LIBCURL_VERSION' ;;
    oniguruma) printf 'ONIGURUMA_VERSION' ;;
    libzip)    printf 'LIBZIP_VERSION' ;;
    icu)       printf 'ICU_VERSION' ;;
    libpng)    printf 'LIBPNG_VERSION' ;;
    jpeg)      printf 'JPEG_VERSION' ;;
    freetype)  printf 'FREETYPE_VERSION' ;;
    webp)      printf 'WEBP_VERSION' ;;
    aom)       printf 'AOM_VERSION' ;;
    avif)      printf 'AVIF_VERSION' ;;
    libffi)    printf 'LIBFFI_VERSION' ;;
    libyaml)   printf 'LIBYAML_VERSION' ;;
    *)         return 1 ;;
  esac
}

# 升级日志：.phprt/logs/upgrade.log，阶段 START/BUILD/WHEEL/VERIFY/COMMIT/ROLLBACK
upgrade_log() {
  mkdir -p "$PHPRT_HOME/logs"
  printf '[%s] %-8s %s %s -> %s %s\n' \
    "$(date '+%F %T')" "$1" "${UP_DEP:-}" "${UP_OLD:-}" "${UP_NEW:-}" "$2" \
    >> "$PHPRT_HOME/logs/upgrade.log"
}

# phprt update <依赖> <版本>
# 同系列小版本升级（如 openssl 3.5.7 -> 3.5.8）：只重编该依赖、替换
# lib/phprt 里的运行时库；消费方不重编、DT_NEEDED 零改写。任何一步失败
# 都回滚到升级前的配置、依赖目录和运行时库。
cmd_update() {
  local dep="$1" newver="$2"
  [ $# -eq 2 ] || die "用法: phprt update <依赖> <版本>（如 phprt update openssl 3.5.8）"
  [ -n "$(dep_version_var "$dep")" ] || die "未知依赖: $dep"

  local ver tree flat repo_conf vvar urlvar oldver newver url oldurl newurl
  local bak tmp bundle old_id new_id rtver f line
  local pconf dnew dold
  ver="$(current_version)" || die "还没有设置 current（先跑 phprt set <版本>）"
  tree="$(tree_of "$ver")"
  flat="$tree/lib/phprt"
  # 回滚要用的路径一次性算好（rollback 里不再拼 $ver，避免作用域意外）
  pconf="$PHPRT_HOME/bundles/php-$ver/bundle.conf"
  dnew="$tree/deps/$dep-$newver"
  [ -x "$tree/bin/php" ] || die "树不存在或未构建: $tree"
  [ "$(cat "$tree/BUILD-STATUS" 2>/dev/null || true)" = ok ] || die "上次构建未完成，先修好树"
  grep -q "^deps/$dep\$" "$tree/BUILD-ORDER" 2>/dev/null ||
    die "当前树没有依赖 $dep（BUILD-ORDER 里没有）"

  vvar="$(dep_version_var "$dep")"
  repo_conf="$PHPRT_REPO/bundles/php-$ver/bundle.conf"
  [ -f "$repo_conf" ] || die "找不到 $repo_conf"
  oldver="$(sed -n "s/^$vvar=//p" "$repo_conf" | head -1)"
  [ -n "$oldver" ] || die "$repo_conf 里没有 $vvar"
  dold="$tree/deps/$dep-$oldver"

  # 只允许同系列（major.minor 相同）且更高的小版本
  [ "$newver" != "$oldver" ] || die "新版本与当前相同: $oldver"
  [ "$(printf '%s\n' "$oldver" | cut -d. -f1-2)" = "$(printf '%s\n' "$newver" | cut -d. -f1-2)" ] ||
    die "只允许同系列小版本升级（当前 $oldver，目标 $newver）"
  [ "$(printf '%s\n%s\n' "$oldver" "$newver" | sort -V | head -1)" = "$oldver" ] ||
    die "拒绝降级: $oldver -> $newver"

  # 构建锁：和 phprt set / add 共用同一把，防止并发改同一棵树
  need_cmds flock
  exec 9>"$PHPRT_HOME/.phprt.lock"
  flock -n 9 || die "另一个 phprt 构建操作正在占用这棵树"

  bak="$PHPRT_HOME/upgrade-backup/update-$(date +%Y%m%d%H%M%S)"
  tmp="$PHPRT_HOME/logs/.update.$$"
  mkdir -p "$bak" "$tmp"
  UP_DEP="$dep" UP_OLD="$oldver" UP_NEW="$newver"

  # 失败回滚：配置、旧 deps 目录、lib/phprt、wheel.map 全部还原
  # 注意：set -e 下不能用 `[ -f x ] && cmd` 一行式，条件为假会静默退出
  rollback_update() {
    local reason="$1"
    log "升级失败，回滚"
    if [ -f "$bak/bundle.conf" ]; then
      cp -f "$bak/bundle.conf" "$repo_conf"
    fi
    if [ -f "$bak/project-bundle.conf" ]; then
      cp -f "$bak/project-bundle.conf" "$pconf"
    fi
    if [ -d "$bak/deps-old" ]; then
      rm -rf "$dnew"
      mv "$bak/deps-old" "$dold"
    fi
    if [ -d "$bak/flat" ]; then
      rm -f "$flat"/*.so*
      cp -af "$bak/flat"/. "$flat"/
    fi
    if [ -f "$bak/wheel.map" ]; then
      cp -f "$bak/wheel.map" "$flat/wheel.map"
    fi
    if [ -f "$bak/wheel.map.prev" ]; then
      cp -f "$bak/wheel.map.prev" "$flat/wheel.map.prev"
    fi
    if [ -d "$bak/la-pc" ]; then
      while IFS= read -r rel; do
        cp -f "$bak/la-pc/$rel" "$tree/$rel"
      done < "$bak/la-pc.list"
    fi
    # wheelize 的临时文件；消费方 state 不用动（签名按 ABI 身份，升级前后不变）
    rm -f "$flat/.replace" "$flat/.wheel.map.tmp"
    upgrade_log ROLLBACK "$reason"
    rm -rf "$tmp"
    die "update 失败，已回滚: $reason"
  }

  upgrade_log START "dep=$dep"
  elf_link_snapshot "$tree" "$tmp/before.snap" || rollback_update "升级前快照失败"

  # 备份：仓库 bundle.conf、项目副本、旧 deps 目录、整个 lib/phprt、wheel.map
  cp -f "$repo_conf" "$bak/bundle.conf"
  cp -f "$PHPRT_HOME/bundles/php-$ver/bundle.conf" "$bak/project-bundle.conf"
  mv "$tree/deps/$dep-$oldver" "$bak/deps-old" || rollback_update "备份旧依赖目录失败"
  mkdir -p "$bak/flat"
  cp -af "$flat"/*.so* "$bak/flat/" || rollback_update "备份运行时库失败"
  cp -f "$flat/wheel.map" "$bak/wheel.map"
  if [ -f "$flat/wheel.map.prev" ]; then
    cp -f "$flat/wheel.map.prev" "$bak/wheel.map.prev"
  fi

  # 旧库的 SONAME 集合：升级后必须完全一致，否则当跨系列拒绝。
  # 用 -maxdepth 2 从 deps 目录起找（lib/ 与 lib64/ 都覆盖，少了也不报错）
  old_id="$(
    find "$bak/deps-old" -maxdepth 2 -type f -name '*.so.*' 2>/dev/null |
      while IFS= read -r f; do patchelf --print-soname "$f" 2>/dev/null; done |
      LC_ALL=C sort -u | paste -sd, -
  )" || rollback_update "读取旧库 SONAME 失败"

  # 改版本与 URL（仓库 bundle.conf 是唯一版本源；项目副本由 bundle_for 同步）。
  # URL 行可能带行尾注释，只替换引号内的部分
  urlvar="$(dep_version_var "$dep" | sed 's/_VERSION$/_URL/')"
  oldurl="$(sed -n "s/^$urlvar=\"\([^\"]*\)\".*$/\1/p" "$repo_conf" | head -1)"
  [ -n "$oldurl" ] || rollback_update "读不到 $urlvar"
  newurl="${oldurl//$oldver/$newver}"
  sed -i "s|^$vvar=.*|$vvar=$newver|" "$repo_conf" || rollback_update "改版本变量失败"
  sed -i "s|^\($urlvar=\"\)[^\"]*\"|\1$newurl\"|" "$repo_conf" || rollback_update "改 URL 失败"
  bundle="$(bundle_for "$ver")" || rollback_update "同步项目 bundle 副本失败"
  # 当前 shell 也要拿到新版本变量（artifact_ok / dep_dir_of 用）
  # shellcheck disable=SC1090
  . "$repo_conf"

  # 只重编该依赖（FORCE=1 绕过配方内部 stamp）。
  # 注意：BUNDLE/TREE 必须显式赋值为小写变量的内容，否则 export 的是空串；
  # 其余变量与 build.sh 保持一致，配方（JOBS/PREFIX/...）才不会报未绑定
  upgrade_log BUILD "start $oldver -> $newver"
  export BUNDLE="$bundle" TREE="$tree" PREFIX="$tree" \
         DEPS="$tree/deps" SRCDIR="$PHPRT_HOME/build" LOGS="$PHPRT_HOME/logs" \
         SELECTED_EXTS="" JOBS="${JOBS:-$(nproc 2>/dev/null || echo 4)}" \
         FORCE=1 REBUILD=1 \
         PHPRT_RUNNER="$PHPRT_HOME/bin/runner"
  if ! bash "$bundle/deps/$dep.sh" > "$tmp/build.log" 2>&1; then
    tail -30 "$tmp/build.log" >&2
    rollback_update "依赖 $dep $newver 构建失败"
  fi
  artifact_ok "$dep" || rollback_update "依赖 $dep $newver 没有产物"

  new_id="$(
    find "$tree/deps/$dep-$newver" -maxdepth 2 -type f -name '*.so.*' 2>/dev/null |
      while IFS= read -r f; do patchelf --print-soname "$f" 2>/dev/null; done |
      LC_ALL=C sort -u | paste -sd, -
  )" || rollback_update "读取新库 SONAME 失败"
  [ -n "$new_id" ] || rollback_update "新依赖没有共享库"
  [ "$new_id" = "$old_id" ] ||
    rollback_update "SONAME 集合变化（$old_id -> $new_id），疑似跨系列，拒绝"
  upgrade_log BUILD "ok soname 集合不变"

  # 其它依赖的 .la/.pc 里可能写着旧依赖目录（libtool/pkg-config 构建期
  # 元数据）：libtool 链接时会读 .la 的 dependency_libs，不改掉以后重链接
  # 会指向已移走的旧目录。完整绝对路径替换（不会误伤同名普通文本），
  # 先备份进事务目录，回滚时一并还原。
  local oldpath m rel fixed=0
  oldpath="$tree/deps/$dep-$oldver"
  : > "$bak/la-pc.list"
  while IFS= read -r m; do
    rel="${m#"$tree"/}"
    mkdir -p "$bak/la-pc/$(dirname "$rel")"
    cp -f "$m" "$bak/la-pc/$rel"
    printf '%s\n' "$rel" >> "$bak/la-pc.list"
    sed -i "s|$oldpath|$dnew|g" "$m"
    fixed=1
  done < <(grep -rl "$oldpath" "$tree/deps" --include='*.la' --include='*.pc' 2>/dev/null)
  [ "$fixed" = 1 ] && upgrade_log BUILD "fixed 其它依赖 .la/.pc 里的旧路径引用"

  # wheel 化：旧 deps 目录已移走，不会扫到两份同名 SONAME；rt 名不变 =>
  # .prev 映射为空 => 消费方 DT_NEEDED 零改写
  upgrade_log WHEEL "start"
  wheelize_tree "$tree" || rollback_update "wheelize 失败"
  elf_link_snapshot "$tree" "$tmp/after.snap" || rollback_update "升级后快照失败"
  if ! diff -q "$tmp/before.snap" "$tmp/after.snap" >/dev/null; then
    diff "$tmp/before.snap" "$tmp/after.snap" | head -20 >&2
    rollback_update "消费方动态链接关系发生变化"
  fi
  upgrade_log WHEEL "ok 消费方 NEEDED/SONAME/RPATH 零变化"

  upgrade_log VERIFY "start"
  wheel_integrity_check "$tree" || rollback_update "wheel 完整性校验失败"
  ( cd "$PHPRT_PROJECT" && ./myphp -v >/dev/null ) || rollback_update "myphp 启动失败"
  if [ "$dep" = openssl ]; then
    # 注意：php 的 OPENSSL_VERSION_TEXT 是编译期字符串，不重编就不会变，
    # 不能当验证依据。直接查运行时真正加载的 flat/libcrypto 里的版本串。
    need_cmds strings
    rtver="$(strings "$flat/libcrypto-rt3.so.3" 2>/dev/null |
             grep -m1 -o 'OpenSSL [0-9][0-9.]*' || true)"
    case "$rtver" in
      *"$newver"*) ;;
      *) rollback_update "运行时 libcrypto 不是 $newver（$rtver）" ;;
    esac
  fi
  upgrade_log VERIFY "ok"

  # 提交：清理备份（失败时 bak 留在 upgrade-backup/ 供人工排查）
  rm -rf "$bak/deps-old" "$bak/flat" "$bak/la-pc" \
         "$bak/wheel.map" "$bak/wheel.map.prev" "$bak/la-pc.list" \
         "$bak/bundle.conf" "$bak/project-bundle.conf"
  rmdir "$bak" 2>/dev/null || true
  upgrade_log COMMIT "ok"
  rm -rf "$tmp"
  ok "phprt update $dep $oldver -> $newver 完成（消费方零改写、零重编）"
}

# phprt outdated：只读检查已安装的 PHP 和依赖库有没有同系列新版本。
# 清单以实际安装为准（BUILD-ORDER + deps/ 实际目录），版本取自项目
# bundle.conf；查询失败只影响本行，不中断其它软件，也不改任何文件。
# 每次查询成功都写进 .phprt/cache/update-check/results.json，之后 update
# 直接读缓存取版本，不再重复请求上游。
# TTY 下动态刷新（先画整表，再原地更新行和底部进度）；非 TTY 输出静态表。
cmd_outdated() {
  need_cmds jq
  local ver tree conf name vvar cur deps_line i rc out st up
  local -a pkg cur_l disp hasq
  local rows_file total nq n_up n_cur n_fail UC_RC UC_LATEST UC_STATUS
  ver="$(current_version)" || die "还没有设置 current（先跑 phprt set <版本>）"
  tree="$(tree_of "$ver")"
  [ -x "$tree/bin/php" ] || die "树不存在或未构建: $tree"
  conf="$PHPRT_HOME/bundles/php-$ver/bundle.conf"
  [ -f "$conf" ] || die "找不到 $conf"
  rows_file="$(mktemp "${TMPDIR:-/tmp}/phprt-outdated.XXXXXX")"

  # 收集清单：PHP + 实际装了 deps 目录的库；bundle.conf 只当版本来源
  pkg=(); cur_l=(); disp=(); hasq=()
  pkg+=("php"); cur_l+=("$ver"); disp+=("PHP"); hasq+=("1")
  while IFS= read -r deps_line; do
    name="${deps_line#deps/}"
    [ -n "$name" ] || continue
    ls -d "$tree/deps/$name"-* >/dev/null 2>&1 || continue
    if vvar="$(dep_version_var "$name")"; then
      cur="$(sed -n "s/^$vvar=//p" "$conf" | head -1)"
      [ -n "$cur" ] || cur='-'
    else
      cur='-'
    fi
    if uc_supported "$name"; then hasq+=("1"); else hasq+=("0"); fi
    pkg+=("$name"); cur_l+=("$cur"); disp+=("$(uc_name "$name")")
  done < <(grep '^deps/' "$tree/BUILD-ORDER" 2>/dev/null || true)

  total="${#pkg[@]}"; nq=0; n_up=0; n_cur=0; n_fail=0
  for i in "${!pkg[@]}"; do
    [ "${hasq[$i]}" = 1 ] && nq=$((nq + 1))
  done

  # 非 TTY：直接输出最终静态表格（日志/管道里不出现 ANSI 控制字符）
  if ! uc_tty; then
    printf '%-12s %-12s %-12s %s\n' '软件包' '当前版本' '最新版本' '状态'
    printf '%s\n' '---------------------------------------------'
    for i in "${!pkg[@]}"; do
      if [ "${hasq[$i]}" != 1 ]; then
        uc_row "${disp[$i]}" "${cur_l[$i]}" '-' '暂不支持'
        continue
      fi
      uc_consider "${pkg[$i]}" "${cur_l[$i]}" "$rows_file"
      if [ "$UC_RC" = 0 ]; then
        uc_row "${disp[$i]}" "${cur_l[$i]}" "$UC_LATEST" "$UC_STATUS"
        if [ "$UC_STATUS" = '发现更新' ]; then n_up=$((n_up + 1)); else n_cur=$((n_cur + 1)); fi
      else
        uc_row "${disp[$i]}" "${cur_l[$i]}" '-' '查询失败'
        n_fail=$((n_fail + 1))
      fi
    done
    uc_cache_save "$rows_file"
    rm -f "$rows_file"
    return 0
  fi

  # TTY：先画整表，再逐个原地刷新行与底部进度
  uc_draw_table pkg cur_l disp hasq "$nq"
  trap 'uc_cache_save "$rows_file"; rm -f "$rows_file"; printf "\r\033[2K\n"; exit 130' INT
  local done_n=0
  for i in "${!pkg[@]}"; do
    [ "${hasq[$i]}" = 1 ] || continue
    uc_write_querying "${disp[$i]}"
    uc_consider "${pkg[$i]}" "${cur_l[$i]}" "$rows_file"
    up=$(( total + 2 - i ))   # 从「正在查询」行上移到第 i 个数据行
    if [ "$UC_RC" = 0 ]; then
      uc_update_row "$up" "${disp[$i]}" "${cur_l[$i]}" "$UC_LATEST" "$UC_STATUS"
      if [ "$UC_STATUS" = '发现更新' ]; then n_up=$((n_up + 1)); else n_cur=$((n_cur + 1)); fi
    else
      uc_update_row "$up" "${disp[$i]}" "${cur_l[$i]}" '-' '查询失败'
      n_fail=$((n_fail + 1))
    fi
    done_n=$((done_n + 1))
    uc_write_progress "$done_n" "$nq"
    uc_write_querying '-'
  done
  trap - INT
  uc_cache_save "$rows_file"
  rm -f "$rows_file"
  # 统计与表格一致：暂不支持 = 总数 - 有查询器数
  printf '\n\n检查完成：发现更新 %s 个，已是最新 %s 个，查询失败 %s 个，暂不支持 %s 个\n' \
    "$n_up" "$n_cur" "$n_fail" "$(( total - nq ))"
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

  # gd 启用时核对格式支持与 bundle.conf 的 ENABLE_GD_* 一致
  if ( cd "$PHPRT_PROJECT" && ./myphp -m 2>/dev/null | grep -qx gd ); then
    log "2b) gd 格式支持（对照 bundle.conf 的 ENABLE_GD_*）"
    local gdconf="$PHPRT_HOME/bundles/php-$ver/bundle.conf"
    local exp_jpg=1 exp_webp=1 exp_avif=1
    if [ -f "$gdconf" ]; then
      # shellcheck disable=SC1090
      . "$gdconf"
      exp_jpg="${ENABLE_GD_JPEG:-1}"
      exp_webp="${ENABLE_GD_WEBP:-1}"
      exp_avif="${ENABLE_GD_AVIF:-1}"
    fi
    local act f got exp gd_mismatch=0
    act="$( ( cd "$PHPRT_PROJECT" && ./myphp -r '
      $i = gd_info();
      foreach (["PNG","JPEG","WebP","AVIF"] as $f) {
        $k = "$f Support";
        echo "$f=" . (!empty($i[$k]) ? "1" : "0") . "\n";
      }') )"
    while IFS='=' read -r f got; do
      case "$f" in
        PNG)  exp=1 ;;
        JPEG) exp="$exp_jpg" ;;
        WebP) exp="$exp_webp" ;;
        AVIF) exp="$exp_avif" ;;
        *)    continue ;;
      esac
      if [ "$got" = "$exp" ]; then
        printf '    ✓ %s=%s\n' "$f" "$got"
      else
        printf '    ✗ %s: 期望 %s，实际 %s\n' "$f" "$exp" "$got"
        gd_mismatch=1
      fi
    done <<< "$act"
    [ "$gd_mismatch" = 0 ] || return 1
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

  log "7) wheel 完整性（sha256 / SONAME / OpenSSL 唯一 / 无残留原始名）"
  if wheel_integrity_check "$tree"; then
    ok "wheel 完整性通过"
  else
    return 1
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
    update)   cmd_update "$@" ;;
    outdated) cmd_outdated ;;
    verify)   cmd_verify ;;
    ""|-h|--help) usage ;;
    *)        die "未知命令: $cmd（--help 查看用法）" ;;
  esac
}
