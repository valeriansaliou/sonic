#!/usr/bin/env bash

##
#  Sonic
#
#  Fast, lightweight and schema-less search backend
#  Copyright: 2026, Rémi Bardon <remi@remibardon.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

# Configure the script to exit when a command fails.
set -e
# Configure the script so ERR traps are inherited.
set -E

: ${SCRIPTS_ROOT:="$(dirname $0)"}
export SCRIPTS_ROOT
for f in colors log die; do
  source "${SCRIPTS_ROOT:?}"/util/"${f:?}".sh
done


# ===== CONSTANTS =====

: ${SELF:="$(basename $0)"}

: ${REPOSITORY_ROOT:="${SCRIPTS_ROOT:?}"/..}
CHANGELOG_FILE="${REPOSITORY_ROOT:?}"/CHANGELOG.md
README_FILE="${REPOSITORY_ROOT:?}"/README.md
CARGO_TOML_FILE="${REPOSITORY_ROOT:?}"/Cargo.toml
CARGO_LOCK_FILE="${REPOSITORY_ROOT:?}"/Cargo.lock
DEBIAN_RULES_FILE="${REPOSITORY_ROOT:?}"/debian/rules

# NOTE: We could use `cargo metadata` here, but it would require `jq` to parse
#   so this is a good enough no-dependency equivalent.
VERSION="$(cargo pkgid | sed 's/.*@//')"


# ===== HELPER FUNCTIONS =====

description() {
  cat <<EOF
${I_BOLD}Creates a new release for the Prose Pod API.${I_RESET}

This script bumps the version number, then adds and pushes a tag to 'origin'.
EOF
}

usage() {
  cat <<EOF
Usage:
  ${SELF:?} major|minor|patch [OPTION...]

Options:
  Safety checks:
    --no-pull   Do not pull remote before making changes.
    --force     The script won't stop you if your index contains uncommitted
                changes.
  Miscellaneous options:
    --help      Explains what the command does and how to use it.
EOF
}

help() {
  printf "$(description)\n"
  echo ''
  printf "$(usage)\n"
  exit 0
}

to_tag() {
  local version="${1:?"Must pass a version number"}"
  echo "v${version/v}"
}

# A simplified `sed` command that works on both macOS and Linux.
replace() {
  local find="${1:?}"
  local replace="${2:?}"
  local file="${3:?}"

  # NOTE: `//$'\n'/\\n` allows escaping newlines.
  local pattern="s@${find}@${replace//$'\n'/\\n}@gm"

  # Make `perl` exit with code 1 if no substitution was applied.
  pattern="$(printf '$changed ||= %s;\nEND { exit !$changed }' "${pattern}")"

  perl -i -pe "$pattern" "$file" || { log_error "Pattern '$find' did not match anything in '$file')."; return 1; }
}
UPDATED_FILES=()
replace_version() {
  UPDATED_FILES+=("${2:?}")
  replace "${1:?}" "\${1}${NEW_VERSION:?}\${2}" "${2:?}"
}


# ===== ARGUMENT PARSING =====

VERSION_COMPONENTS=($(echo "${VERSION:?}" | tr '.' ' '))

case "$1" in
  major)
    VERSION_COMPONENTS[0]=$(( VERSION_COMPONENTS[0] + 1 ))
    VERSION_COMPONENTS[1]=0
    VERSION_COMPONENTS[2]=0
    ;;
  minor)
    VERSION_COMPONENTS[1]=$(( VERSION_COMPONENTS[1] + 1 ))
    VERSION_COMPONENTS[2]=0
    ;;
  patch)
    VERSION_COMPONENTS[2]=$(( VERSION_COMPONENTS[2] + 1 ))
    ;;
  --help) help ;;
  '') log_error "Expected at least one argument."; log_info "$(usage)"; die ;;
  *) log_error "Unknown positional argument: '$1'."; log_info "$(usage)"; die ;;
esac
# Skip first argument now that it's processed.
shift 1

for arg in "$@"; do
  case $arg in
    --no-pull) NO_PULL=1 ;;
    --force) FORCE=1 ;;
    --help) help ;;
    *) log_error "Unknown argument: '$arg'."; log_info "$(usage)"; die ;;
  esac
done

# ===== MAIN LOGIC =====

if [ "${DRY_RUN:-0}" -ne 0 ]; then
  die 'Dry run mode not supported.'
fi

# Ensure there are no uncommitted changes.
if [ -z "${FORCE-}" ]; then
  git diff-index --quiet HEAD || die "Your index contains uncommitted changes. Please commit or stash them before creating a release."
fi

# Ensure the changelog has been updated.
if [ -z "${FORCE-}" ] && git diff --quiet "$(to_tag "${VERSION:?}")" -- "${CHANGELOG_FILE:?}"; then
  die "Don’t forget to run 'task changelog:prepare'."
fi

GIT_BRANCH="$(git branch --show-current)"

if [ -z "${NO_PULL-}" ]; then
  # Ensure linear history.
  log_info "Pulling 'origin'…"
  git pull origin "${GIT_BRANCH:?}"
fi

# Convert the new version to a string.
NEW_VERSION=$(echo "${VERSION_COMPONENTS[*]}" | tr ' ' '.')

# Log some useful info.
log_info "Version change: $(fg_yellow "$(to_tag "${VERSION:?}")") -> $(fg_green "$(to_tag "${NEW_VERSION:?}")")"
log_info "New commits:"
log_as_info_ git --no-pager log --reverse --no-merges \
  --format="- %C(auto)%h %s %C(green)(%ad)%C(reset)" --date=short --color \
  "$(to_tag "${VERSION:?}")"..HEAD

# Register a trap to revert on error.
revert_on_error() {
  local exit_code=$?

  # Invert `VERSION` and `NEW_VERSION` then rerun `update_all_versions` to
  # revert changes.
  # NOTE: Using `git restore` could revert changes unexpectedly when `FORCE=1`.
  local old_version="${VERSION:?}"
  VERSION="${NEW_VERSION:?}"
  NEW_VERSION="${old_version:?}"
  update_all_versions &>/dev/null || :

  # But restore the `CHANGELOG.md` file anyway as its changes cannot be easily
  # reverted.
  git restore "${CHANGELOG_FILE:?}"
}
trap revert_on_error ERR

# Update version numbers in files.
update_all_versions() {
  log_info "Changing version number in '$(basename "${CARGO_TOML_FILE:?}")'…"
  replace_version '^(version = \").+(\")' "${CARGO_TOML_FILE:?}"

  log_info "Updating '$(basename "${CARGO_LOCK_FILE:?}")'…"
  cargo check

  log_info "Updating '$(basename "${CHANGELOG_FILE:?}")'…"
  replace "compare/$(to_tag "${VERSION:?}")...HEAD" "$(cat <<EOF
compare/$(to_tag "${NEW_VERSION:?}")...HEAD

## [${NEW_VERSION:?}] ($(date -I))

[${NEW_VERSION:?}]: https://github.com/valeriansaliou/sonic/compare/$(to_tag "${VERSION:?}")...$(to_tag "${NEW_VERSION:?}")
EOF
)" "${CHANGELOG_FILE:?}"

  log_info "Changing version number in '$(basename "${README_FILE:?}")'…"
  replace_version '^(.*valeriansaliou/sonic:v).+$' "${README_FILE:?}"

  log_info "Changing version number in '$(basename "${DEBIAN_RULES_FILE:?}")'…"
  replace_version '^(VERSION = ).+$' "${DEBIAN_RULES_FILE:?}"
}
update_all_versions

# Commit changes.
commit() {
  log_info "Committing changes…"
  git add "${UPDATED_FILES[@]}"
  git commit -m "$(to_tag "${NEW_VERSION:?}")"
}
commit

# Create & push a new git tag.
push_new_tag() {
  log_info "Creating tag…"
  git tag "$(to_tag "${NEW_VERSION:?}")" -m "$(to_tag "${NEW_VERSION:?}")"

  log_info "Pushing tag…"
  git push ${FORCE_PUSH:+-f} --atomic origin "${GIT_BRANCH:?}" "$(to_tag "${NEW_VERSION:?}")"

  success "Successfully created and pushed tag '$(to_tag "${NEW_VERSION:?}")'"
}
push_new_tag
