#!/bin/bash

##
#  Sonic
#
#  Fast, lightweight and schema-less search backend
#  Copyright: 2023, Valerian Saliou <valerian@valeriansaliou.name>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

# Detect system architecture
detect_architecture() {
    arch=$(uname -m)
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    case $arch in
        aarch64)
            if [[ "$os" == "mingw"* || "$os" == "cygwin"* ]]; then
                TARGET_ARCH="aarch64-pc-windows-msvc"
            else
                TARGET_ARCH="aarch64-linux-android"
            fi
            ;;
        x86_64)
            if [[ "$os" == "mingw"* || "$os" == "cygwin"* ]]; then
                TARGET_ARCH="x86_64-pc-windows-msvc"
            else
                TARGET_ARCH="x86_64-unknown-linux-gnu"
            fi
            ;;
        armv7l)
            TARGET_ARCH="armv7-unknown-linux-gnueabihf"
            ;;
        i686)
            if [[ "$os" == "mingw"* || "$os" == "cygwin"* ]]; then
                TARGET_ARCH="i686-pc-windows-msvc"
            else
                TARGET_ARCH="i686-unknown-linux-gnu"
            fi
            ;;
        *)
            echo "Unsupported architecture or OS: $arch on $os"
            exit 1
            ;;
    esac
    echo "Detected architecture: $arch on $os -> $TARGET_ARCH"
}

# Read arguments
while [ "$1" != "" ]; do
    argument_key=$(echo $1 | awk -F= '{print $1}')
    argument_value=$(echo $1 | awk -F= '{print $2}')
    case $argument_key in
        -v | --version)
            # Notice: strip any leading 'v' to the version number
            SONIC_VERSION="${argument_value/v/}"
            ;;
        *)
            echo "Unknown argument received: '$argument_key'"
            exit 1
            ;;
    esac

    shift
done

# Ensure release version is provided
if [ -z "$SONIC_VERSION" ]; then
  echo "No Sonic release version was provided, please provide it using '--version'"
  exit 1
fi

# Define release pipeline
function release_for_architecture {
    final_tar="v$SONIC_VERSION-$1-$2.tar.gz"

    rm -rf ./sonic/ && \
        cargo build --target "$3" --release && \
        mkdir ./sonic && \
        if [[ "$3" == *"windows-msvc"* ]]; then
            cp -p "target/$3/release/sonic.exe" ./sonic/ 
        else
            cp -p "target/$3/release/sonic" ./sonic/
        fi
        cp -r ./config.cfg sonic/ && \
        tar --owner=0 --group=0 -czvf "$final_tar" ./sonic && \
        rm -r ./sonic/
    release_result=$?

    if [ $release_result -eq 0 ]; then
        echo "Result: Packed architecture: $1 ($2) to file: $final_tar"
    fi

    return $release_result
}

# Detect architecture
detect_architecture

# Run release tasks
ABSPATH=$(cd "$(dirname "$0")"; pwd)
BASE_DIR="$ABSPATH/../"

rc=0

pushd "$BASE_DIR" > /dev/null
echo "Executing release steps for Sonic v$SONIC_VERSION..."
release_for_architecture "$TARGET_ARCH" "gnu" "$TARGET_ARCH"
rc=$?

if [ $rc -eq 0 ]; then
    echo "Success: Done executing release steps for Sonic v$SONIC_VERSION"
else
    echo "Error: Failed executing release steps for Sonic v$SONIC_VERSION"
fi
popd > /dev/null

exit $rc