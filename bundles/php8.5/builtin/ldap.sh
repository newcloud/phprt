#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=ldap switch=ENABLE_LDAP modules=ldap ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_LDAP:-0}" = 1 ]; then
    printf '%s\n' "--with-ldap=$(dep_dir_of openldap)"
  else
    printf '%s\n' --without-ldap
  fi
}

needs() {
  [ "${ENABLE_LDAP:-0}" = 1 ] && printf '%s\n' openldap
  return 0
}

recipe_main "$@"
