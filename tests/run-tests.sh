#!/bin/sh
npm ci

cargo run -- --config config.cfg &
SONIC_PID=$!
sleep 2

node .

STATUS=$?
kill $SONIC_PID
exit $STATUS

