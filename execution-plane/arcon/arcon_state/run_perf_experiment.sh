#!/usr/bin/env bash

NUM_OPS=1000000 SESSION_LENGTH=10 cargo run --example perf_experiment --features=sled,rocks,faster --release -- 1 FASTER
