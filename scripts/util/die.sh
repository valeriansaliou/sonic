##
#  Some reusable logging functions.
#
#  Copyright: 2026, Rémi Bardon <remi@remibardon.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

source "$(dirname "${BASH_SOURCE[0]}")"/log.sh

die() {
  if [ $# -gt 0 ]; then
    log_error "$@"
  fi
  exit 1
}
