#!/usr/bin/env bash
# PHP 本体：只负责根据最终 configure 参数编译 PHP。
# 依赖由 build.sh 的 deps 阶段先构建，这里不再隐式建依赖。
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config
need_cmds gcc make tar

build_args() {
  local e

  # 按 SELECTED_EXTS 设置 ENABLE_*，然后收集每个 builtin recipe 的 configure 参数。
  # builtin 清单来自 bundles/<版本>/builtin/*.sh，不再维护 BUILTIN_ORDER。
  builtin_apply_switches

  ARGS=(
    --prefix="$TREE"
    --with-config-file-path="$TREE/etc"
    --with-config-file-scan-dir="$TREE/etc/conf.d"
    --enable-cli --disable-cgi
    --without-pear
    # OPcache 自 PHP 8.5 起固定编入 PHP，--enable-opcache 已被上游移除
    --enable-filter --enable-phar --enable-fileinfo
    --enable-bcmath --enable-calendar --enable-exif
    # pcntl / posix / sockets / shmop / sysv* 和 readline 现在是 builtin recipe，
    # 由各自的 flags() 决定 --enable-* / --disable-*（posix 在 PHP 里默认 enabled，
    # 不选时 recipe 会显式 --disable-posix）。
    # PHP 默认会尝试编译这些扩展。phprtv2 不提供对应 recipe，显式关掉，
    # 避免 configure 去探测系统库（libxml2 / zlib）。
    # 如果以后加了对应 builtin recipe，它的 --flags 在后面，会覆盖这里的负向参数。
    --without-libxml --disable-dom --disable-simplexml
    --disable-xml --disable-xmlreader --disable-xmlwriter
    --without-zlib
  )
  for e in $(builtin_names); do
    while IFS= read -r a; do ARGS+=("$a"); done < <(bash "$BUNDLE/builtin/$e.sh" --flags)
  done

  # static 方式的 PECL 扩展：源码已经放进 ext/<名字>，参数由 recipe 的
  # stage_static() 写进这个文件。
  if [ -n "${STATIC_EXT_FLAGS:-}" ] && [ -s "$STATIC_EXT_FLAGS" ]; then
    while IFS= read -r a; do
      [ -n "$a" ] && ARGS+=("$a")
    done < "$STATIC_EXT_FLAGS"
  fi
}

build_args

# 只打印最终 configure 参数，供 build.sh 计算 PHP 签名
if [ "${1:-}" = "--flags" ]; then
  printf '%s\n' "${ARGS[@]}"
  exit 0
fi

ensure_source "$PHP_ARCHIVE" "$PHP_URL" "$PHP_SHA256"
SRC="$SRCDIR/$PHP_SRC_DIR"
if [ ! -d "$SRC" ]; then
  rm -rf "$SRC"
  extract "$DOWNLOADS/$PHP_ARCHIVE" "$SRCDIR"
else
  log "复用源码 $SRC"
fi
[ -d "$SRC" ] || die "解压后找不到 $SRC"
cd "$SRC"

# static 扩展增删后，旧的 Makefile / 生成头还引用着 ext/<名字>。
# 先 distclean，再 buildconf + configure，避免 “没有规则可制作目标 ext/redis/php_redis.h”。
if [ "${PHPRT_BUILDCONF:-0}" = 1 ] && [ -f Makefile ]; then
  log "清理 PHP 目标文件（static 扩展集合变化）"
  make distclean >/dev/null 2>&1 || true
fi

if [ "${PHPRT_BUILDCONF:-0}" = 1 ] ||
   { [ "${NEEDS_BUILDCONF:-0}" = 1 ] && [ ! -x "$SRC/configure" ]; }; then
  AC_DIR="$(dep_dir_of autoconf)"
  PERL_DIR="$(dep_dir_of perlshim)"
  [ -x "$AC_DIR/bin/autoconf" ] || ensure_dep autoconf
  [ -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ] || ensure_dep perlshim
  export PATH="$AC_DIR/bin:$PATH"
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"
  log "buildconf"
  ./buildconf --force > "$LOGS/php-buildconf.log" 2>&1 ||
    { tail -30 "$LOGS/php-buildconf.log"; die "buildconf 失败（需要 autoconf）"; }
fi

# PHP 默认 autoconf 会给 -g -O2；这里统一从 -O2 起步，两个对比版本都不带 -g。
export CFLAGS="${PHP_CFLAGS:--O2}${CFLAGS:+ $CFLAGS}"
export CXXFLAGS="${PHP_CXXFLAGS:--O2}${CXXFLAGS:+ $CXXFLAGS}"

LTO_LDFLAGS_EXTRA=""
if lto_on php; then
  export CFLAGS="$CFLAGS $LTO_CFLAGS"
  export CXXFLAGS="$CXXFLAGS $LTO_CFLAGS"
  LTO_LDFLAGS_EXTRA="$LTO_LDFLAGS"
fi

names="$(enabled_dep_names)"

# 静态链接：建一个只含 .a 软链的搜索目录，并放在所有 deps 的 -L 前面。
# GNU ld 在第一个有同名库的目录里选定 libfoo.a，于是 -lfoo 不会退化成 .so。
# 不用 -Wl,-Bstatic：libtool 会把它和 --start-group 重排到对象文件之前，
# 实际链接时这些库仍然是动态的。
STATIC_LIB_DIR=""
if link_is_static; then
  STATIC_LIB_DIR="$BUILD_ROOT/static-libs"
  rm -rf "$STATIC_LIB_DIR"; mkdir -p "$STATIC_LIB_DIR"
  for _d in $names; do
    _ddir="$(dep_dir_of "$_d")"
    _found=0
    for _f in "$_ddir"/lib/*.a "$_ddir"/lib64/*.a; do
      [ -f "$_f" ] || continue
      _found=1
      ln -sfn "$_f" "$STATIC_LIB_DIR/$(basename "$_f")"
    done
    if [ "$_found" != 1 ]; then
      case " ${PHPRT_PHP_DEPS:-} " in
        *" $_d "*) die "依赖 $_d 没有静态库（--static-all 需要 .a，见 docs/STATIC-ALL.md）" ;;
      esac
    fi
  done
fi

export CPPFLAGS="$(dep_include_flags $names)${CPPFLAGS:+ $CPPFLAGS}"
export LDFLAGS="$(dep_build_flags $names)${LTO_LDFLAGS_EXTRA:+ $LTO_LDFLAGS_EXTRA}${LDFLAGS:+ $LDFLAGS}"
[ -n "$STATIC_LIB_DIR" ] && LDFLAGS="-L$STATIC_LIB_DIR $LDFLAGS"

# 静态链接：static_ldflags() 不能放在 LDFLAGS（对象文件之前），要等
# configure 生成 Makefile 后放进 EXTRA_LIBS 的链接组末尾。
STATIC_EXT_LIBS=""
if [ -n "${STATIC_EXT_LDFLAGS:-}" ] && [ -s "$STATIC_EXT_LDFLAGS" ]; then
  if link_is_static; then
    STATIC_EXT_LIBS="$(tr '\n' ' ' < "$STATIC_EXT_LDFLAGS")"
  else
    LDFLAGS="$LDFLAGS $(tr '\n' ' ' < "$STATIC_EXT_LDFLAGS")"
  fi
fi

if link_is_static; then
  # 这两个库如果动态链接，会出现在产物 NEEDED 里，单文件复制到别的机器
  # 可能缺 libgcc_s.so / libstdc++.so。没有 C++ 代码时静态档案不会被拉进来。
  LDFLAGS="$LDFLAGS -static-libgcc -static-libstdc++"
fi
export PKG_CONFIG_PATH="$(dep_pc_path $names)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

# 复用源码树时 make 只看时间戳：CFLAGS 变了必须清掉旧 .o，
# 否则 LTO / 非 LTO 对象会混在一起。
# FLAGS_SIG 必须含 CPPFLAGS/LDFLAGS：同系列依赖升级后 include/lib 路径变了，
    # 即使 CFLAGS 没变也要 distclean，否则旧的 .dep 文件还指着旧版本目录，
    # 下次 make 会报「没有规则可制作目标 …/openssl-3.5.6/include/…」。
    FLAGS_SIG="CFLAGS=${CFLAGS:-}|CXXFLAGS=${CXXFLAGS:-}|CPPFLAGS=${CPPFLAGS:-}|LDFLAGS=${LDFLAGS:-}|lto_ld=${LTO_LDFLAGS_EXTRA}"
FLAGS_STAMP="$SRC/.phprt-build-flags"
if [ -f "$SRC/Makefile" ] && [ "$(cat "$FLAGS_STAMP" 2>/dev/null || true)" != "$FLAGS_SIG" ]; then
  log "编译参数变化，清理 PHP 目标文件"
  make distclean >/dev/null 2>&1 || true
fi
printf '%s\n' "$FLAGS_SIG" > "$FLAGS_STAMP"

log "PHP configure"
./configure "${ARGS[@]}" > "$LOGS/php-configure.log" 2>&1 ||
  { tail -60 "$LOGS/php-configure.log"; die "PHP configure 失败"; }

if link_is_static; then
  log "静态链接 PHP（依赖使用 .a）"
  # configure 会把每个 --with-* 的库目录写进 EXTRA_LDFLAGS(_PROGRAM)，并加
  # 绝对 rpath。静态链接必须让只含 .a 的静态目录排在它们前面，同时删掉 rpath。
  for _v in EXTRA_LDFLAGS EXTRA_LDFLAGS_PROGRAM; do
    sed -i -E "s#^($_v = )#\1-L$STATIC_LIB_DIR #" Makefile
  done
  grep -q -- "-L$STATIC_LIB_DIR" Makefile ||
    die "把静态库目录加进 Makefile 失败"
  sed -i -E '/^(EXTRA_LDFLAGS|EXTRA_LDFLAGS_PROGRAM|NATIVE_RPATHS|PHP_RPATHS|LDFLAGS) =/{
    s|-Wl,-rpath,[^ ]+||g
    s|-R [^ ]+||g
  }' Makefile
  # 静态库是单向扫描：libcurl.a 在 libssl.a 之前就拉不出后面的符号。
  # 把同一组库再放一遍（libtool --preserve-dup-deps 会保留重复项），
  # 解决依赖倒序；static_ldflags 的输出放在每一遍末尾。
  _extra_libs="$(sed -n 's/^EXTRA_LIBS = //p' Makefile | head -1)"
  [ -n "$_extra_libs" ] || die "Makefile 里找不到 EXTRA_LIBS"
  _libs="$_extra_libs${STATIC_EXT_LIBS:+ $STATIC_EXT_LIBS}"
  sed -i "s|^EXTRA_LIBS = .*|EXTRA_LIBS = $_libs $_libs|" Makefile
fi

if lto_on php; then
  # 这两个文件使用 GCC 全局寄存器变量（%r14/%r15）；LTO 合并翻译单元后
  # GCC 报 "global register variable follows a function definition"。
  # 只让它们用 -fno-lto 编译，其余对象仍参与 LTO，混合链接没有问题。
  #   Zend/zend_execute.c                    — VM 执行器
  #   ext/opcache/jit/zend_jit_vm_helpers.c  — JIT helper
  sed -i '/-c .*\/zend_execute\.c/ s|\$(CFLAGS_CLEAN)|$(CFLAGS_CLEAN) -fno-lto|' Makefile
  sed -i '/-c .*\/zend_jit_vm_helpers\.c/ s|\$(CFLAGS_CLEAN)|$(CFLAGS_CLEAN) -fno-lto|' Makefile
  rm -f Zend/zend_execute.lo Zend/zend_execute.dep Zend/.libs/zend_execute.o \
        ext/opcache/jit/zend_jit_vm_helpers.lo \
        ext/opcache/jit/zend_jit_vm_helpers.dep \
        ext/opcache/jit/.libs/zend_jit_vm_helpers.o
  grep -q -- '-fno-lto .*zend_execute\.c' Makefile ||
    die "给 zend_execute.c 加 -fno-lto 失败"
  grep -q -- '-fno-lto .*zend_jit_vm_helpers\.c' Makefile ||
    die "给 zend_jit_vm_helpers.c 加 -fno-lto 失败"
fi

{
  printf 'configure: ./configure'
  printf ' %q' "${ARGS[@]}"
  printf '\n'
} > "$TREE/.configure-line"

log "PHP make -j$JOBS"
make -j"$JOBS" > "$LOGS/php-make.log" 2>&1 ||
  { tail -60 "$LOGS/php-make.log"; die "PHP make 失败"; }
log "PHP make install"
make install > "$LOGS/php-install.log" 2>&1 ||
  { tail -40 "$LOGS/php-install.log"; die "PHP make install 失败"; }

[ -x "$TREE/bin/php" ] || die "安装后没有 $TREE/bin/php"
[ -f "$TREE/bin/phar.phar" ] || die "安装后没有 phar.phar（phar 命令不可用）"

if link_is_static; then
  _bad_needed=""
  for _b in "$TREE"/bin/*; do
    [ -f "$_b" ] && [ ! -L "$_b" ] || continue
    readelf -h "$_b" >/dev/null 2>&1 || continue
    _one="$(readelf -d "$_b" 2>/dev/null | awk -F'[][]' '/\(NEEDED\)/{print $2}' |
      grep -Ev '^(libc|libm|libpthread|libdl|librt|libresolv|libcrypt|libutil|libnsl|libanl|libmvec|ld-linux)\.so' || true)"
    [ -n "$_one" ] && _bad_needed+="$_b: $_one"$'\n'
  done
  [ -z "$_bad_needed" ] ||
    die "静态链接未完成，仍有外部依赖:"$'\n'"$_bad_needed"
fi

if lto_on php; then
  # 链接完成后 LTO 中间段应全部被消费；残留说明链接没有走 LTO。
  if readelf -S "$TREE/bin/php" 2>/dev/null | grep -c '\.gnu\.lto' >/dev/null; then
    die "bin/php 仍残留 .gnu.lto 段，LTO 没有完成链接合并"
  fi
fi
ok "PHP $PHP_VERSION 本体完成"
