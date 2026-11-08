#!/usr/bin/env bash
# 内置扩展：ftp（FTP 客户端；无外部依赖库）
#  与 openssl 同编时 FTP-SSL 支持隐式启用（ftp.installation.php）
#  phprt: builtin=ftp switch=ENABLE_FTP
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_FTP:-0}" = 1 ]; then
      printf '%s\n' --enable-ftp
    else
      printf '%s\n' --disable-ftp
    fi ;;
  --needs) ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
