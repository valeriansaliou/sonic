#!/bin/bash

npm ci
cargo build
STATUS=0

for i in $(find ./scenarios/ -name "*.js")
do
    [[ -d ./data/ ]] && rm -r ./data/
    cargo run -- --config config.cfg &
    SONIC_PID=$!
    sleep 2
    node $i
    [[ $? -eq 0 ]] || STATUS=1
    kill $SONIC_PID
    wait $SONIC_PID
done

[[ -d ./data/ ]] && rm -r ./data/
exit $STATUS
