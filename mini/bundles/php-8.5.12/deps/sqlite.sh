#!/usr/bin/env bash
# =============================================================================
#  SQLite（PHP sqlite3 + pdo_sqlite）
#  安装到: deps/sqlite-<版本>/
#  可单独执行: lib/modules/deps/sqlite.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc ar unzip

D="$(dep_dir_of sqlite)"
SIG="sqlite=$SQLITE_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok sqlite "$SIG" && [ -f "$D/lib/libsqlite3.so" ]; then
  ok "SQLite 已安装，跳过"; exit 0
fi

ensure_clean_target sqlite "$D"

ZIP="$(basename "$SQLITE_URL")"
fetch "$SQLITE_URL" "$DOWNLOADS/$ZIP" "${SQLITE_SHA256:-}"

WORK="$SRCDIR/sqlite-$SQLITE_VERSION"
if [ ! -f "$WORK/sqlite3.c" ]; then
  rm -rf "$WORK"; mkdir -p "$WORK"
  log "解压 $ZIP"
  extract "$DOWNLOADS/$ZIP" "$WORK"
  AMALG="$(find "$WORK" -maxdepth 1 -type d -name 'sqlite-amalgamation-*' | head -1)"
  [ -n "$AMALG" ] || die "SQLite 解压后未找到 sqlite-amalgamation 目录"
  mv "$AMALG"/* "$WORK"/ && rmdir "$AMALG"
fi

cd "$WORK"
log "编译 libsqlite3（共享库 + 静态库）"
CFLAGS="-O2 -fPIC -DSQLITE_ENABLE_FTS5 -DSQLITE_ENABLE_RTREE -DSQLITE_ENABLE_JSON1"
gcc $CFLAGS -c sqlite3.c -o sqlite3.o
gcc -shared -Wl,-soname,libsqlite3.so.0 -o libsqlite3.so.0 sqlite3.o -lm -lpthread -ldl
ln -sf libsqlite3.so.0 libsqlite3.so
ar rcs libsqlite3.a sqlite3.o

mkdir -p "$D/lib" "$D/include" "$D/lib/pkgconfig"
install -m 755 libsqlite3.so.0 "$D/lib/"
ln -sf libsqlite3.so.0 "$D/lib/libsqlite3.so"
install -m 644 libsqlite3.a "$D/lib/"
install -m 644 sqlite3.h sqlite3ext.h "$D/include/"

SQLITE_VER_ACTUAL="$(grep -oE '#define SQLITE_VERSION[[:space:]]+"[0-9.]+"' sqlite3.h | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
cat > "$D/lib/pkgconfig/sqlite3.pc" <<EOF
prefix=$D
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: SQLite
Description: SQLite library
Version: $SQLITE_VER_ACTUAL
Libs: -L\${libdir} -lsqlite3
Libs.private: -lm -lpthread -ldl
Cflags: -I\${includedir}
EOF

stamp_write sqlite "$SIG"
ok "SQLite $SQLITE_VER_ACTUAL -> $D"
