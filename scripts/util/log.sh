##
#  Some reusable logging functions.
#
#  Copyright: 2026, Rémi Bardon <remi@remibardon.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

source "$(dirname "${BASH_SOURCE[0]}")"/colors.sh

log_success_() {
  printf "${I_BOLD}%b${I_RESET} %s\n" "${C_GREEN}Success:${C_RESET}" "$*"
}
log_success() {
  echo "$@" | while IFS= read -r line; do log_success_ "$line"; done
}

log_debug_() {
  if [ -n "${GITHUB_ACTIONS-}" ]; then
    # NOTE: See <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-a-debug-message>.
    printf "::debug::%s\n" "$*" >&2
  else
    printf "%b %s\n" "${C_CYAN}Debug:${C_RESET}" "$*"
  fi
}
log_debug() {
  echo "$@" | while IFS= read -r line; do log_debug_ "$line"; done
}

log_info_() {
  printf "${I_BOLD}%b${I_RESET} %s\n" "${C_BLUE}Info:${C_RESET}" "$*"
}
log_info() {
  echo "$@" | while IFS= read -r line; do log_info_ "$line"; done
}

log_warn_() {
  if [ -n "${GITHUB_ACTIONS-}" ]; then
    # NOTE: See <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-a-warning-message>.
    printf "::warning::%s\n" "$*" >&2
  else
    printf "${I_BOLD}%b${I_RESET} %s\n" "${C_YELLOW}Warning:${C_RESET}" "$*" >&2
  fi
}
log_warn() {
  echo "$@" | while IFS= read -r line; do log_warn_ "$line"; done
}

log_error_() {
  if [ -n "${GITHUB_ACTIONS-}" ]; then
    # NOTE: See <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-an-error-message>.
    printf "::error::%s\n" "$*" >&2
  else
    printf "${I_BOLD}%b${I_RESET} %s\n" "${C_RED}Error:${C_RESET}" "$*" >&2
  fi
}
log_error() {
  echo "$@" | while IFS= read -r line; do log_error_ "$line"; done
}

# Runs the command and logs its output with `log_info`.
log_as_info_() {
  ( set -o pipefail; "$@" 2>&1 | while IFS= read -r line; do log_info_ "$line"; done )
}
