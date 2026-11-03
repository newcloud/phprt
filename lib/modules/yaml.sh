#!/usr/bin/env bash
# =============================================================================
#  yaml 扩展（PECL 源码 + phpize，依赖 libyaml）
#  可单独执行: lib/modules/yaml.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
LY="$(dep_dir_of libyaml)"
SIG="yaml=$YAML_EXT_VERSION;libyaml=$LIBYAML_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok yaml "$SIG" && [ -f "$PREFIX/etc/conf.d/26-yaml.ini" ]; then
  ok "yaml 扩展已装，跳过"; exit 0
fi

TAR="$(basename "$YAML_EXT_URL")"
fetch "$YAML_EXT_URL" "$DOWNLOADS/$TAR" "${YAML_EXT_SHA256:-}"

SRC="$SRCDIR/yaml-$YAML_EXT_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.yaml-$YAML_EXT_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/yaml-$YAML_EXT_VERSION" ] || die "解压后未找到 yaml-$YAML_EXT_VERSION"
  mv "$TMPX/yaml-$YAML_EXT_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 PECL 扩展需要 autoconf，先安装"
  bash "$MODULES/autoconf.sh"
fi
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  bash "$MODULES/perlshim.sh"
fi

LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
export LD_LIBRARY_PATH="$LY/lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

cd "$SRC"
log "phpize"
phpize > "$LOGS/yaml-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/yaml-phpize.log"; die "phpize 失败"; }

log "configure（--with-yaml=$LY）"
./configure --with-php-config="$PREFIX/bin/php-config" --with-yaml="$LY" \
  > "$LOGS/yaml-configure.log" 2>&1 \
  || { tail -30 "$LOGS/yaml-configure.log"; die "yaml configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/yaml-make.log" 2>&1 \
  || { tail -40 "$LOGS/yaml-make.log"; die "yaml 编译失败"; }
make install >> "$LOGS/yaml-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/yaml.so" ] || die "没找到 $EXT_DIR/yaml.so"

cat > "$PREFIX/etc/conf.d/26-yaml.ini" <<EOF
; 由 phprt 生成（yaml $YAML_EXT_VERSION / libyaml $LIBYAML_VERSION）
extension=yaml.so
EOF

stamp_write yaml "$SIG"
ok "yaml $YAML_EXT_VERSION -> $EXT_DIR/yaml.so"
