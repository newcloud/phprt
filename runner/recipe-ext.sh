#!/usr/bin/env bash
# phprt：扩展的通用构建流程（driver 侧，不含任何包名）。
#
# 用法（在 ext recipe 的 build() 里）：
#   ext_pecl_build <名字> <版本> <URL> <SHA256> [configure 参数...]
#
# 用法（在 builtin recipe 的 shared 构建里，由 build.sh 调用）：
#   ext_build_ready <名字> <源码目录> <签名> <显示名> [configure 参数...]
#
# 用法（在 ext recipe 的 stage_static() 里）：
#   ext_stage_static <名字> <版本> <URL> <SHA256> [configure 参数...]
#   把源码放进 PHP 源码树的 ext/<名字>，并记录 static 构建的
#   configure 参数和 static_ldflags() 的输出。
#
# 可选钩子（定义在 recipe 里）：
#   ext_prepare()   解压后、phpize 前执行，cwd 是源码目录（打补丁用）
#   EXT_MODULE      产物模块名，默认等于 <名字>
#   EXT_SIG_EXTRA   额外签名内容（例如依赖版本），可省略
#   static_ldflags() static 构建时补进 PHP 链接行的库
#
# 只负责编译和安装 .so；ini 由 recipe 的 enable() 写。

# 编译扩展需要的工具链环境（autoconf/perlshim/依赖库路径）。
ext_build_env() {
  local ac_dir perl_dir lib_dirs names
  ac_dir="$(dep_dir_of autoconf)"
  perl_dir="$(dep_dir_of perlshim)"
  [ -x "$ac_dir/bin/autoconf" ] || ensure_dep autoconf
  [ -f "$perl_dir/share/perl5core/IPC/Cmd.pm" ] || ensure_dep perlshim
  lib_dirs="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
  export PATH="$ac_dir/bin:$PREFIX/bin:$PATH"
  export LD_LIBRARY_PATH="$PREFIX/lib${lib_dirs:+:$lib_dirs}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export PHP_AUTOCONF="$ac_dir/bin/autoconf" PHP_AUTOHEADER="$ac_dir/bin/autoheader"
  export PERL5LIB="$perl_dir/share/perl5core${PERL5LIB:+:$PERL5LIB}"

  # 和 PHP 本体一样用树内依赖的头/库/pkg-config；否则 intl 这类会找到
  # 系统 ICU，verify 会报“借用了安装根外的库”。
  names="$(enabled_dep_names)"
  export CPPFLAGS="$(dep_include_flags $names)${CPPFLAGS:+ $CPPFLAGS}"
  export LDFLAGS="$(dep_build_flags $names)${LDFLAGS:+ $LDFLAGS}"
  export PKG_CONFIG_PATH="$(dep_pc_path $names)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
}

# 源码已经就位：phpize -> configure -> make -> install。
# 名字、源码目录、签名、显示名由调用方给；$@ 是 configure 参数。
ext_build_ready() {
  local name="$1" src="$2" sig="$3" label="$4"
  shift 4
  local php ext_dir mod
  php="$PREFIX/bin/php"
  [ -x "$php" ] || die "还没有 PHP：先编译 PHP 本体"
  [ -d "$src" ] || die "扩展源码不存在: $src"
  need_cmds gcc make tar
  mod="${EXT_MODULE:-$name}"

  ext_build_env "$name"

  cd "$src"
  if declare -F ext_prepare >/dev/null 2>&1; then
    log "预处理源码（$name）"
    ext_prepare || die "$name 预处理失败"
  fi

  log "phpize"
  phpize > "$LOGS/$name-phpize.log" 2>&1 \
    || { tail -30 "$LOGS/$name-phpize.log"; die "phpize 失败（$name）"; }

  log "configure（$name）"
  ./configure --with-php-config="$PREFIX/bin/php-config" "$@" \
    > "$LOGS/$name-configure.log" 2>&1 \
    || { tail -30 "$LOGS/$name-configure.log"; die "$name configure 失败"; }

  log "make -j$JOBS"
  make -j"$JOBS" > "$LOGS/$name-make.log" 2>&1 \
    || { tail -40 "$LOGS/$name-make.log"; die "$name 编译失败"; }
  make install >> "$LOGS/$name-make.log" 2>&1

  ext_dir="$("$PREFIX/bin/php-config" --extension-dir)"
  [ -f "$ext_dir/$mod.so" ] || die "没找到 $ext_dir/$mod.so"

  [ -n "$sig" ] && stamp_write "$name" "$sig"
  ok "$label -> $ext_dir/$mod.so"
}

ext_pecl_build() {
  local name="$1" ver="$2" url="$3" sha="$4"
  shift 4
  local sig tar src tmpx
  sig="$name=$ver;$(php_sig)"
  [ -n "${EXT_SIG_EXTRA:-}" ] && sig+=";$EXT_SIG_EXTRA"

  if [ "$FORCE" != 1 ] && stamp_ok "$name" "$sig"; then
    ok "$name 已编译，跳过"; return 0
  fi

  tar="$(basename "$url")"
  fetch "$url" "$DOWNLOADS/$tar" "${sha:-}"

  src="$SRCDIR/$name-$ver-php-$PHP_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$src" ]; then
    log "复用源码 $src"
    ( cd "$src" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$src"
    tmpx="$SRCDIR/.$name-$ver.$$"
    rm -rf "$tmpx"; mkdir -p "$tmpx"
    log "解压 $tar"
    extract "$DOWNLOADS/$tar" "$tmpx"
    [ -d "$tmpx/$name-$ver" ] || die "解压后未找到 $name-$ver"
    mv "$tmpx/$name-$ver" "$src"
    rm -rf "$tmpx"
  fi

  ext_build_ready "$name" "$src" "$sig" "$name $ver" "$@"
}

# static 构建：把 PECL 源码放进 PHP 源码树的 ext/<名字>。
# 参数里的 configure 参数会追加到 $STATIC_EXT_FLAGS；
# recipe 若定义了 static_ldflags()，输出追加到 $STATIC_EXT_LDFLAGS。
ext_stage_static() {
  local name="$1" ver="$2" url="$3" sha="$4"
  shift 4
  local php_src="${PHPRT_PHP_SRC:?缺少 PHPRT_PHP_SRC}" tar src tmp top
  [ -d "$php_src/ext" ] || die "PHP 源码树不完整: $php_src/ext"
  [ -n "${STATIC_EXT_FLAGS:-}" ] || die "缺少 STATIC_EXT_FLAGS"
  [ -n "${STATIC_EXT_LDFLAGS:-}" ] || die "缺少 STATIC_EXT_LDFLAGS"

  tar="$(basename "$url")"
  fetch "$url" "$DOWNLOADS/$tar" "${sha:-}"

  src="$php_src/ext/$name"
  rm -rf "$src"
  tmp="$SRCDIR/.static-$name-$$"
  rm -rf "$tmp"; mkdir -p "$tmp"
  log "解压 $tar（static 源码）"
  extract "$DOWNLOADS/$tar" "$tmp"
  top="$tmp/$name-$ver"
  if [ ! -d "$top" ]; then
    top="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d | head -1)"
  fi
  [ -n "$top" ] && [ -d "$top" ] || die "解压后未找到 $name-$ver"
  mv "$top" "$src"
  rm -rf "$tmp"

  printf '%s\n' "$@" >> "$STATIC_EXT_FLAGS"
  if declare -F static_ldflags >/dev/null 2>&1; then
    static_ldflags >> "$STATIC_EXT_LDFLAGS"
  fi
  ok "static 源码就位: $src"
}
