#!/usr/bin/env bash
# PHP 本体：只负责根据最终 configure 参数编译 PHP。
# 依赖由 build.sh 的 deps 阶段先构建，这里不再隐式建依赖。
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config
need_cmds gcc make tar

BUILTIN_ORDER="openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi"

build_args() {
  local e f sw v
  for e in $BUILTIN_ORDER; do
    f="$BUNDLE/builtin/$e.sh"
    [ -f "$f" ] || continue
    sw="$(sed -n 's/^# *phprt:.*[[:space:]]switch=\([^[:space:]]*\).*/\1/p' "$f" | head -1)"
    [ -n "$sw" ] || continue
    if printf ' %s ' "$SELECTED_EXTS" | grep -q " $e "; then v=1; else v=0; fi
    printf -v "$sw" '%s' "$v"
    export "${sw?}"
  done

  ARGS=(
    --prefix="$TREE"
    --with-config-file-path="$TREE/etc"
    --with-config-file-scan-dir="$TREE/etc/conf.d"
    --enable-cli --disable-cgi
    --without-pear
    # OPcache 自 PHP 8.5 起固定编入 PHP，--enable-opcache 已被上游移除
    --enable-filter --enable-phar --enable-fileinfo
    --enable-bcmath --enable-calendar --enable-exif
    --enable-pcntl --enable-posix --enable-sockets
    --enable-shmop --enable-sysvmsg --enable-sysvsem --enable-sysvshm
  )
  for e in $BUILTIN_ORDER; do
    f="$BUNDLE/builtin/$e.sh"
    [ -f "$f" ] || continue
    while IFS= read -r a; do ARGS+=("$a"); done < <(bash "$f" --flags)
  done
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
  extract "$PHPRT_HOME/downloads/$PHP_ARCHIVE" "$SRCDIR"
else
  log "复用源码 $SRC"
fi
[ -d "$SRC" ] || die "解压后找不到 $SRC"
cd "$SRC"

if [ "${NEEDS_BUILDCONF:-0}" = 1 ] && [ ! -x "$SRC/configure" ]; then
  AC_DIR="$(dep_dir_of autoconf)"
  RE2C_DIR="$(dep_dir_of re2c)"
  PERL_DIR="$(dep_dir_of perl5core)"
  [ -x "$AC_DIR/bin/autoconf" ] || bash "$(module_sh autoconf)"
  [ -x "$RE2C_DIR/bin/re2c" ] || bash "$(module_sh re2c)"
  [ -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ] || bash "$(module_sh perlshim)"
  export PATH="$AC_DIR/bin:$RE2C_DIR/bin:$PATH"
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"
  log "buildconf"
  ./buildconf --force > "$LOGS/php-buildconf.log" 2>&1 ||
    { tail -30 "$LOGS/php-buildconf.log"; die "buildconf 失败（需要 autoconf/re2c）"; }
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
export CPPFLAGS="$(dep_include_flags $names)${CPPFLAGS:+ $CPPFLAGS}"
export LDFLAGS="$(dep_build_flags $names)${LTO_LDFLAGS_EXTRA:+ $LTO_LDFLAGS_EXTRA}${LDFLAGS:+ $LDFLAGS}"
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
ok "PHP $PHP_VERSION 本体完成"
