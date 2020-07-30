#!/usr/bin/env bash

run="cargo run --example perf_experiment --features=sled,rocks,faster --release -- "

NUM_OPS=1000000 \
SESSION_LENGTH=10 \
KEY_SIZE=16 \
VALUE_SIZE=64 \
$run 1 FASTER
