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
# Configure the script to exit on undefined variables.
set -u
# Configure the script so errors in pipes are bubbled up.
set -o pipefail
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
README_FILE="${REPOSITORY_ROOT:?}"/README.md
CARGO_LOCK_FILE="${REPOSITORY_ROOT:?}"/Cargo.lock
DEBIAN_RULES_FILE="${REPOSITORY_ROOT:?}"/packaging/debian/rules
SERVER_DIR="${REPOSITORY_ROOT:?}"/server
CORE_DIR="${REPOSITORY_ROOT:?}"/core
CLIENT_DIR="${REPOSITORY_ROOT:?}"/client

# NOTE: We could use `cargo metadata` here, but it would require `jq` to parse
#   so this is a good enough no-dependency equivalent.
SERVER_VERSION="$(cargo pkgid -p sonic-server | sed 's/.*@//')"
CORE_VERSION="$(cargo pkgid -p sonic-core | sed 's/.*@//')"
CLIENT_VERSION="$(cargo pkgid -p sonic_client | sed 's/.*@//')"


# ===== HELPER FUNCTIONS =====

description() {
  cat <<EOF
${I_BOLD}Creates a new release of Sonic.${I_RESET}

This script bumps the version number, then adds and pushes a tag to 'origin'.
EOF
}

usage() {
  cat <<EOF
Usage:
  ${SELF:?} server|core|client major|minor|patch|no-bump [OPTION...]

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

UPDATED_FILES=()
# A simplified `sed` command that works on both macOS and Linux.
# Also stores updated files in a list for later use.
replace() {
  local find="${1:?}"
  local replace="${2:?}"
  local file="${3:?}"

  UPDATED_FILES+=("${file:?}")

  # NOTE: `//$'\n'/\\n` allows escaping newlines.
  local pattern="s@${find}@${replace//$'\n'/\\n}@gm"

  # Make `perl` exit with code 1 if no substitution was applied.
  pattern="$(printf '$changed ||= %s;\nEND { exit !$changed }' "${pattern}")"

  perl -i -pe "$pattern" "$file" || { log_error "Pattern '$find' did not match anything in '$file')."; return 1; }
}
replace_version() {
  replace "${1:?}" "\${1}${3:-"${NEW_VERSION:?}"}\${2}" "${2:?}"
}


# ===== ARGUMENT PARSING =====

# Process non-positional arguments.
ARGS_=()
for arg in "$@"; do
  case $arg in
    --no-pull) NO_PULL=1 ;;
    --force) FORCE=1 ;;
    --help) help ;;
    *) ARGS_+=("$arg") ;;
  esac
done
# Update command args so we can then list test names.
set -- "${ARGS_[@]}"
unset ARGS_

# Process positional arguments.
if [ $# -lt 2 ]; then
  log_error "Missing argument(s)."; log_info "$(usage)"; die
elif [ $# -gt 2 ]; then
  log_error "Too many arguments."; log_info "$(usage)"; die
fi

case "$1" in
  server|bin)
    RELEASING=sonic-server
    VERSION="${SERVER_VERSION:?}"
    CHANGELOG_FILE="${SERVER_DIR:?}"/CHANGELOG.md
    CARGO_TOML_FILE="${SERVER_DIR:?}"/Cargo.toml

    to_tag() {
      local version="${1:?"Must pass a version number"}"
      echo "v${version#v}"
    }
    ;;
  core|lib)
    RELEASING=sonic-core
    VERSION="${CORE_VERSION:?}"
    CHANGELOG_FILE="${CORE_DIR:?}"/CHANGELOG.md
    CARGO_TOML_FILE="${CORE_DIR:?}"/Cargo.toml
    SERVER_CARGO_TOML_FILE="${SERVER_DIR:?}"/Cargo.toml

    to_tag() {
      local version="${1:?"Must pass a version number"}"
      echo "core-v${version#v}"
    }
    ;;
  client|client-rust)
    RELEASING=sonic_client
    VERSION="${CLIENT_VERSION:?}"
    CHANGELOG_FILE="${CLIENT_DIR:?}"/CHANGELOG.md
    CARGO_TOML_FILE="${CLIENT_DIR:?}"/Cargo.toml
    SERVER_CARGO_TOML_FILE="${SERVER_DIR:?}"/Cargo.toml

    to_tag() {
      local version="${1:?"Must pass a version number"}"
      echo "client-v${version#v}"
    }
    ;;
  *) log_error "Unknown argument: '$arg'."; log_info "$(usage)"; die ;;
esac

# TODO: Automatically detect semver change level when releasing the core.

VERSION_COMPONENTS=($(echo "${VERSION:?}" | tr '.' ' '))
case "$2" in
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
  no-bump) ;;
  --help) help ;;
  '') log_error "Expected at least two arguments."; log_info "$(usage)"; die ;;
  *) log_error "Unknown semver change level: '$2'."; log_info "$(usage)"; die ;;
esac


# ===== MAIN LOGIC =====

if [ "${DRY_RUN:-0}" -ne 0 ]; then
  die 'Dry run mode not supported.'
fi

# Ensure command ran on main branch.
MAIN_BRANCH="$(git symbolic-ref refs/remotes/origin/HEAD | sed 's@^refs/remotes/origin/@@')"
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ -z "${FORCE-}" ] && [ "${CURRENT_BRANCH:?}" != "${MAIN_BRANCH:?}" ]; then
  die "'${SELF% --}' must be ran on '${MAIN_BRANCH:?}'."
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

log_info "Ensuring linear history with 'origin'…"
git push --dry-run origin "${GIT_BRANCH:?}"

# If bumping the server and the core has been updated since last version,
# make sure the dependency version has been updated.
if [ "${RELEASING:?}" == 'sonic-server' ] && [ -z "${FORCE-}" ]; then
  # NOTE: We cannot use
  #   `cargo pkgid --manifest-path <(git show "${VERSION:?}":Cargo.toml)`
  #   as it requires the `-Zscript` flag which in turn requires the nightly
  #   Rust toolchain.
  PREVIOUS_CORE_VERSION="$(perl -ne 'print "$1\n" if /^version = "=([^"]+)"$/' <(git show "$(to_tag "${VERSION:?}")":core/Cargo.toml))"

  # SAFETY: This matches the first occurence of `^version = ` in the file,
  #   which means we don’t have to worry about cases like declaring dependency
  #   versions in separate TOML sections.
  LAST_CORE_RELEASE_COMMIT="$(git blame -L '/^version = /',+1 core/Cargo.toml | cut -d' ' -f1)"

  if ! git diff --quiet "${LAST_CORE_RELEASE_COMMIT:?}" -- "${CORE_DIR:?}"/src "${CORE_DIR:?}"/Cargo.toml; then
    # If the core was updated but not released, bail out.
    die 'Sonic core has changes. Release it first with `task release:core`.'
  elif ! grep -q "sonic-core = { version = \"=${CORE_VERSION:?}\"" server/Cargo.toml; then
    # If the core’s version used by the server is not up-to-date, bail out.
    die "sonic-server isn’t using the last version of sonic-core (${CORE_VERSION:?}). Fix this first."
  fi
fi

log_info "Validating supply chain…"
if ! cargo deny check --show-stats; then
  die 'Supply chain rejected. Fix mentioned issues first.'
fi
log_success 'Supply chain validated.'

# Convert the new version to a string.
NEW_VERSION=$(echo "${VERSION_COMPONENTS[*]}" | tr ' ' '.')

if [ "${NEW_VERSION:?}" != "${VERSION:?}" ]; then
  # Log some useful info.
  log_info "Version change: $(fg_yellow "$(to_tag "${VERSION:?}")") -> $(fg_green "$(to_tag "${NEW_VERSION:?}")")"
  log_info "New commits:"
  log_as_info_ git --no-pager log --reverse --no-merges \
    --format="- %C(auto)%h %s %C(green)(%ad)%C(reset)" --date=short --color \
    "$(to_tag "${VERSION:?}")"..HEAD
fi

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
  log_info "Changing version number in '${CARGO_TOML_FILE#"${REPOSITORY_ROOT:?}/"}'…"
  replace_version '^(version = \")[^\"]+(\")' "${CARGO_TOML_FILE:?}"

  if [ "${RELEASING:?}" == 'sonic-core' ]; then
    log_info "Changing core version number in '${SERVER_CARGO_TOML_FILE#"${REPOSITORY_ROOT:?}/"}'…"
    replace_version '^(sonic-core = \{ version = \"=)[^\"]+(\")' "${SERVER_CARGO_TOML_FILE:?}"
  elif [ "${RELEASING:?}" == 'sonic_client' ]; then
    log_info "Changing client version number in '${SERVER_CARGO_TOML_FILE#"${REPOSITORY_ROOT:?}/"}'…"
    replace_version '^(sonic_client = \{ version = \"=)[^\"]+(\")' "${SERVER_CARGO_TOML_FILE:?}"
  elif [ "${RELEASING:?}" == 'sonic-server' ]; then
    log_info "Changing version number in '$(basename "${README_FILE:?}")'…"
    replace_version '^(.*valeriansaliou/sonic:v).+$' "${README_FILE:?}"

    log_info "Changing version number in '$(basename "${DEBIAN_RULES_FILE:?}")'…"
    replace_version '^(VERSION = ).+$' "${DEBIAN_RULES_FILE:?}"
  fi

  log_info "Updating '$(basename "${CARGO_LOCK_FILE:?}")'…"
  cargo metadata --offline --manifest-path "${CARGO_TOML_FILE:?}" --format-version 1 >/dev/null

  log_info "Dry-running \`cargo publish\`…"
  cargo publish --dry-run -p "${RELEASING:?}" --allow-dirty
  UPDATED_FILES+=("${CARGO_LOCK_FILE:?}")

  log_info "Updating '$(basename "${CHANGELOG_FILE:?}")'…"
  replace "compare/([a-z0-9._-]+[a-z0-9_-])...HEAD" "$(cat <<EOF
compare/$(to_tag "${NEW_VERSION:?}")...HEAD

## [${NEW_VERSION:?}] ($(date -I))

[${NEW_VERSION:?}]: https://github.com/valeriansaliou/sonic/compare/\${1}...$(to_tag "${NEW_VERSION:?}")
EOF
)" "${CHANGELOG_FILE:?}"
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

  log_success "Successfully created and pushed tag '$(to_tag "${NEW_VERSION:?}")'"
}
push_new_tag
