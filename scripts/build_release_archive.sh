#!/usr/bin/env bash

##
#  Sonic
#
#  Fast, lightweight and schema-less search backend
#  Copyright: 2023, Valerian Saliou <valerian@valeriansaliou.name>
#  Copyright: 2026, Rémi Bardon <remi@remibardon.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

# Configure the script to exit when a command fails.
set -e
# Configure the script to exit on undefined variables.
set -u
# Configure the script so errors in pipes are bubbled up.
set -o pipefail

: ${SCRIPTS_ROOT:="$(dirname $0)"}
export SCRIPTS_ROOT
for f in colors log die; do
  source "${SCRIPTS_ROOT:?}"/util/"${f:?}".sh
done


# ===== CONSTANTS =====

: ${SELF:="$(basename $0)"}

: ${REPOSITORY_ROOT:="${SCRIPTS_ROOT:?}"/..}

# NOTE: We could use `cargo metadata` here, but it would require `jq` to parse
#   so this is a good enough no-dependency equivalent.
SERVER_VERSION="$(cargo pkgid -p sonic-server | sed 's/.*@//')"

# NOTE: This could be parameterized one day.
BUILD_PROFILE=release


# ===== HELPER FUNCTIONS =====

description() {
  cat <<EOF
${I_BOLD}Builds Sonic (server) and creates a release archive.${I_RESET}
EOF
}

usage() {
  cat <<EOF
Usage:
  ${SELF:?} <ARCH> <PLATFORM> <TARGET>
    Where ARCH is the target CPU architecture (used for naming)
          PLATFORM is a tag for the target platform (used for naming)
          TARGET is the target triple (used for compilation)

Options:
  Miscellaneous options:
    --help      Explains what the command does and how to use it.

Examples:
  Build for Linux using glibc:
    ${SELF:?} x86_64 gnu x86_64-unknown-linux-gnu
  Build for Linux using musl:
    ${SELF:?} x86_64 musl x86_64-unknown-linux-musl
  Build for ARM-based Macs:
    ${SELF:?} aarch64 darwin aarch64-apple-darwin
EOF
}

help() {
  printf "$(description)\n"
  echo ''
  printf "$(usage)\n"
  exit 0
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
if [ $# -lt 3 ]; then
  log_error "Missing argument(s)."; log_info "$(usage)"; die
elif [ $# -gt 3 ]; then
  log_error "Too many arguments."; log_info "$(usage)"; die
fi
TARGET_ARCH="$1" TARGET_PLATFORM="$2" TARGET_TRIPLE="$3"


# ===== MAIN LOGIC =====

main() {
  log_info "Building and archiving Sonic v${SERVER_VERSION:?}…"

  # NOTE: No need to `pushd` here as the script has its own context
  #   (unless ran with `source`, which one shouldn’t do).
  cd "${REPOSITORY_ROOT:?}"

  cargo build --target "${TARGET_TRIPLE:?}" --locked --profile "${BUILD_PROFILE:?}"

  rm -rf ./sonic/
  mkdir -p ./sonic
  cp -p "target/${TARGET_TRIPLE:?}/${BUILD_PROFILE:?}/sonic" ./sonic/
  cp -r ./config.cfg sonic/

  local final_tar="v${SERVER_VERSION:?}-${TARGET_ARCH:?}-${TARGET_PLATFORM:?}.tar.gz"
  tar --owner=0 --group=0 -czvf "${final_tar:?}" ./sonic
  rm -r ./sonic/

  log_success "Packed Sonic v${SERVER_VERSION:?} for ${TARGET_ARCH:?} (${TARGET_PLATFORM:?}) to file '${final_tar:?}'."

  if [ -n "${GITHUB_OUTPUT-}" ]; then
    echo "archive_path=${final_tar:?}" >> "${GITHUB_OUTPUT:?}"
  fi
}
main
