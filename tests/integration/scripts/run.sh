#!/bin/bash

##
#  Sonic
#  Fast, lightweight and schema-less search backend
#
#  Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
#  License: Mozilla Public License v2.0 (MPL v2.0)
##

ABSPATH=$(cd "$(dirname "$0")"; pwd)
TESTSPATH="$ABSPATH/../"

STATUS=0

# Build Sonic
cargo build

# Run tests
pushd "$TESTSPATH" > /dev/null
  # Install test dependencies from a clean state
  pushd "./runner/" > /dev/null
    npm ci
  popd

  # Run each test scenario
  for scenario in $(find ./scenarios/ -name "*.js")
  do
      [[ -d ./instance/data/ ]] && rm -r ./instance/data/

      # Run sonic from a clean state
      pushd "./instance/" > /dev/null
        cargo run -- --config config.cfg &
        SONIC_PID=$!
        sleep 2
      popd

      # Run scenario
      node $scenario

      [[ $? -eq 0 ]] || STATUS=1

      # Stop Sonic
      kill $SONIC_PID
      wait $SONIC_PID
  done

  [[ -d ./instance/data/ ]] && rm -r ./instance/data/
popd

exit $STATUS
