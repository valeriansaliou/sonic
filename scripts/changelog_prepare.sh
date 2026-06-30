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

: ${SCRIPTS_ROOT:="$(dirname $0)"}
export SCRIPTS_ROOT
for f in colors log die; do
  source "${SCRIPTS_ROOT:?}"/util/"${f:?}".sh
done


# ===== CONSTANTS =====

: ${SELF:="$(basename $0)"}

: ${REPOSITORY_ROOT:="${SCRIPTS_ROOT:?}"/..}
README_FILE="${REPOSITORY_ROOT:?}"/README.md
SERVER_DIR="${REPOSITORY_ROOT:?}"/server
CORE_DIR="${REPOSITORY_ROOT:?}"/core

# NOTE: We could use `cargo metadata` here, but it would require `jq` to parse
#   so this is a good enough no-dependency equivalent.
SERVER_VERSION="$(cargo pkgid -p sonic-server | sed 's/.*@//')"
CORE_VERSION="$(cargo pkgid -p sonic-core | sed 's/.*@//')"


# ===== HELPER FUNCTIONS =====

description() {
  cat <<EOF
${I_BOLD}Prepares changelog entries for the next release.${I_RESET}

This script is not a replacement for writing changelog entries manually, it’s
only there to speed it up. It won’t work if you have already started writing
unreleased changelog entries.
EOF
}

usage() {
  cat <<EOF
Usage:
  ${SELF:?} server|core [OPTION...]

Options:
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

git_log() {
  git --no-pager log --reverse --no-merges \
    --format="${1:?"Format expected"}" --date=short --color \
    "$(to_tag "${VERSION:?}")"..HEAD
}


# ===== ARGUMENT PARSING =====

# Process non-positional arguments.
ARGS_=()
for arg in "$@"; do
  case $arg in
    --help) help ;;
    *) ARGS_+=("$arg") ;;
  esac
done
# Update command args so we can then list test names.
set -- "${ARGS_[@]}"
unset ARGS_

# Process positional arguments.
if [ $# -lt 1 ]; then
  log_error "Missing argument(s)."; log_info "$(usage)"; die
elif [ $# -gt 1 ]; then
  log_error "Too many arguments."; log_info "$(usage)"; die
fi

case "$1" in
  server|bin)
    RELEASING=server
    VERSION="${SERVER_VERSION:?}"
    CHANGELOG_FILE="${SERVER_DIR:?}"/CHANGELOG.md
    CARGO_TOML_FILE="${SERVER_DIR:?}"/Cargo.toml

    to_tag() {
      local version="${1:?"Must pass a version number"}"
      echo "v${version#v}"
    }
    ;;
  core|lib)
    RELEASING=core
    VERSION="${CORE_VERSION:?}"
    CHANGELOG_FILE="${CORE_DIR:?}"/CHANGELOG.md
    CARGO_TOML_FILE="${CORE_DIR:?}"/Cargo.toml

    to_tag() {
      local version="${1:?"Must pass a version number"}"
      echo "core-v${version#v}"
    }
    ;;
  *) log_error "Unknown argument: '$1'."; log_info "$(usage)"; die ;;
esac


# ===== MAIN LOGIC =====

if [ -z "${FORCE-}" ] && ! grep -zq '...HEAD\n\n## \['"${VERSION:?}"'\]' "${CHANGELOG_FILE:?}"; then
  log_error "Cannot prepare changelog entries when some already exist."
  log_info "For your information, commits since last release ($(to_tag "${VERSION:?}")) are:"
  git_log '* %s (in `%C(auto)%h`)'
  die
fi

# A regex to separate commit messages which are meaningful in the changelog
# from those which users shouldn’t have to worry about (i.e. internal stuff).
# NOTE: `^` also helps not strating with `-`, which `grep` would read as an
#   argument. Make sure to escape the leading `-` if you ever remove the `^`.
MEANINGLESS_COMMIT_REGEX='^* (ci|tools|docs|chore|test):'

cat <<EOF > temp
New commits:
$(git_log '* %s (in `%h`)' | grep -vE "${MEANINGLESS_COMMIT_REGEX:?}")

Probably not meaningful in the changelog:
$(git_log '* %s (in `%h`)' | grep -E "${MEANINGLESS_COMMIT_REGEX:?}")

### Removals

* TODO

### Changes

* TODO

### New Features

* TODO

### Bug Fixes

* TODO

EOF

# Source: <https://unix.stackexchange.com/a/193498/632020>.
ed -s "${CHANGELOG_FILE:?}" <<EOF
/## \[${VERSION:?}\]/-r temp
w
q
EOF

rm temp

log_success 'Successfully prepared next changelog entries.'

log_warn '========================================================================'
log_warn 'As stated in Keep a Changelog (https://keepachangelog.com/en/1.1.0/),'
log_warn 'changelogs are meant for humans. This script simplified your job of'
log_warn "writing it by inserting all commits since last release ($(to_tag "${VERSION:?}"))"
log_warn 'but you still have to split it into Removed/Changed/Added/Fixed and make'
log_warn 'it human-readable. Some commits should probably be removed, and others'
log_warn 'might need to be squashed into a single changelog entry.'
log_warn '========================================================================'
