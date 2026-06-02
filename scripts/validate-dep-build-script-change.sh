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


# ===== MAIN LOGIC =====

: ${CARGO_HOME:=$HOME/.cargo}

shopt -s nullglob

registries=("${CARGO_HOME:?}"/registry/src/*)
case ${#registries[@]} in
  0)
    log_error "No Cargo registry found in '${CARGO_HOME:?}/registry/src'" >&2
    exit 2
    ;;
  1) CARGO_REGISTRY_DIR="${#registries[0]}" ;;
  *)
    log_debug 'Found multiple Cargo registries, using newest…' >&2
    CARGO_REGISTRY_DIR="$(printf '%s\n' "${registries[@]}" | xargs ls -dt | head -n1)" ;;
esac

sort_allowed_build_scripts() {
  ex -s deny.toml <<'EOF'
/^allow-build-scripts = \[/+1;/^\]/-1 sort
wq
EOF
}

allowed_crates="$(sed -n '/^allow-build-scripts = \[/,/^\]/p' deny.toml | rg -o '"([^"]+)"' -r '$1')"

lock_file_diff="$(if git diff --quiet -- Cargo.lock; then
  git log -p -1 -- Cargo.lock
else
  git diff -- Cargo.lock
fi)"

updated=($(cargo deny check bans 2>&1 | rg build-script-not-allowed | sed -nE "s/.* crate '([^ ]+) = ([^']+)' .*/\1@\2/p"))
for entry in "${updated[@]}"; do
  IFS='@' read -r name version <<< "$entry"
  echo
  # log_info "$name: ??? -> '$version'"

  # 1. Show diff of commit containing version changes.
  # 2. Filter on the crate we care about, keeping next two lines
  #    as they contain `-version` and `+version`.
  # 3. If the crate exists in multiple versions, keep only the one that
  #    was changed to the one denied by `cargo deny`.
  # 4. Extract the previous version from the `-version` line.
  # 5. Discard exit code to avoid aborting on missing match.
  old_version="$(echo "$lock_file_diff" \
    | rg -A 2 "\"$name\"" <<< "$lock_file_diff" \
    | rg -B 1 "\+version = \"$version\"" \
    | rg -o '\-version = "([^"]+)"' -r '$1' || :)"

  if [ -z "${old_version-}" ]; then # New dependency with build script.
    log_info "$name: MISSING -> $(fg_green "$version")"

    insert=no

    cat "${CARGO_REGISTRY_DIR:?}"/${name:?}-${version:?}/build.rs

    printf "Allow build script? [y/N] "
    read -r allow

    case "$allow" in
      [yY])
        log_warn "Allowing '${name:?}@${version:?}'."

        ex -s deny.toml <<EOF
let @new="    \"${name:?}@${version:?}\","
/^allow-build-scripts = \[/put new
wq
EOF

        sort_allowed_build_scripts
        ;;
      *) echo "no" ;;
    esac
  else # Dependency with build script previously allowed.
    log_info "$name: $(fg_green "$old_version") -> $(fg_yellow "$version")"

    update=no
    if diff -u --color=always \
      "${CARGO_REGISTRY_DIR:?}"/${name:?}-${old_version:?}/build.rs \
      "${CARGO_REGISTRY_DIR:?}"/${name:?}-${version:?}/build.rs
    then # Build script not changed.
      log_info "Build script not changed, bumping allowed version."
      update=yes
    else # Build script changed.
      printf "Allow change? [y/N] "
      read -r allow

      case "$allow" in
        [yY]) update=yes ;;
        *) log_info 'Skipping.' ;;
      esac
    fi

    if [ $update == "yes" ]; then
      log_warn "Allowing $(fg_purple "${name:?}@${version:?}")."

      perl -i -pe "s/\"${name}\\@${old_version//./\.}\"/\"${name}\\@${version}\"/g" deny.toml

      # Sort just in case there were multiple versions
      # and their order is now wrong (it’s cheap).
      sort_allowed_build_scripts
    fi
  fi
done

echo
log_success 'Successfully checked dependencies build script changes.'
log_info 'Check diff summary using `git diff -- deny.toml`.'
