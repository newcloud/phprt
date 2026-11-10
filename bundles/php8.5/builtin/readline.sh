#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=readline switch=ENABLE_READLINE modules=readline ini=- type=static default=static category=Runtime
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# php -a / 交互式 CLI 的行编辑。readline 需要 GNU readline + ncurses（termcap）。
# 只能静态：PHP 的 ext/readline 用 PHP_ARG_WITH，phpize 会把 libedit 强制打开，
# 单独编译 .so 会去要 libedit（ext/readline/config.m4），所以契约写 type=static。
flags() {
  if [ "${ENABLE_READLINE:-0}" = 1 ]; then
    printf '%s\n' "--with-readline=$(dep_dir_of readline)"
  else
    printf '%s\n' --without-readline
  fi
}

needs() {
  [ "${ENABLE_READLINE:-0}" = 1 ] && printf '%s\n' readline
  return 0
}

recipe_main "$@"
