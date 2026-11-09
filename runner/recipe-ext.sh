#!/usr/bin/env bash
# phprt：PECL 扩展的通用构建流程（driver 侧，不含任何包名）。
#
# 用法（在 ext recipe 的 build() 里）：
#   ext_pecl_build <名字> <版本> <URL> <SHA256> [configure 参数...]
#
# 可选钩子（定义在 recipe 里）：
#   ext_prepare()   解压后、phpize 前执行，cwd 是源码目录（打补丁用）
#   EXT_MODULE      产物模块名，默认等于 <名字>
#   EXT_SIG_EXTRA   额外签名内容（例如依赖版本），可省略
#
# 只负责编译和安装 .so；ini 由 recipe 的 enable() 写。

ext_pecl_build() {
  local name="$1" ver="$2" url="$3" sha="$4"
  shift 4
  local php ac_dir perl_dir sig tar src tmpx ext_dir lib_dirs mod
  php="$PREFIX/bin/php"
  [ -x "$php" ] || die "还没有 PHP：先编译 PHP 本体"

  ac_dir="$(dep_dir_of autoconf)"
  perl_dir="$(dep_dir_of perlshim)"
  mod="${EXT_MODULE:-$name}"
  sig="$name=$ver;$(php_sig)"
  [ -n "${EXT_SIG_EXTRA:-}" ] && sig+=";$EXT_SIG_EXTRA"

  if [ "$FORCE" != 1 ] && stamp_ok "$name" "$sig"; then
    ok "$name 已编译，跳过"; return 0
  fi

  need_cmds gcc make tar

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

  [ -x "$ac_dir/bin/autoconf" ] || ensure_dep autoconf
  [ -f "$perl_dir/share/perl5core/IPC/Cmd.pm" ] || ensure_dep perlshim

  lib_dirs="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
  export PATH="$ac_dir/bin:$PREFIX/bin:$PATH"
  export LD_LIBRARY_PATH="$PREFIX/lib${lib_dirs:+:$lib_dirs}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export PHP_AUTOCONF="$ac_dir/bin/autoconf" PHP_AUTOHEADER="$ac_dir/bin/autoheader"
  export PERL5LIB="$perl_dir/share/perl5core${PERL5LIB:+:$PERL5LIB}"

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

  stamp_write "$name" "$sig"
  ok "$name $ver -> $ext_dir/$mod.so"
}
