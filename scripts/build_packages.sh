#!/bin/bash

##
#  Sonic
#
#  Fast, lightweight and schema-less search backend
#  Copyright: 2023, Valerian Saliou <valerian@valeriansaliou.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

# Define build pipeline
function build_for_target {
    OS="$2" DIST="$3" ARCH="$1" ./packpack/packpack
    release_result=$?

    if [ $release_result -eq 0 ]; then
        mkdir -p "./packages/$2_$3/"
        mv ./build/*$4 "./packages/$2_$3/"

        echo "Result: Packaged architecture: $1 for OS: $2:$3 (*$4)"
    fi

    return $release_result
}

# Run release tasks
ABSPATH=$(cd "$(dirname "$0")"; pwd)
BASE_DIR="$ABSPATH/../"

rc=0

pushd "$BASE_DIR" > /dev/null
    echo "Executing packages build steps for Sonic..."

    # Initialize `packpack`
    rm -rf ./packpack && \
        git clone https://github.com/packpack/packpack.git packpack
    rc=$?

    # Proceed build for each target?
    if [ $rc -eq 0 ]; then
        build_for_target "x86_64" "debian" "bookworm" ".deb"
        rc=$?
    fi

    # Cleanup environment
    rm -rf ./build ./packpack

    if [ $rc -eq 0 ]; then
        echo "Success: Done executing packages build steps for Sonic"
    else
        echo "Error: Failed executing packages build steps for Sonic"
    fi
popd > /dev/null

exit $rc
