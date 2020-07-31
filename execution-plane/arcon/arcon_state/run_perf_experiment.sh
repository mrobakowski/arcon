#!/usr/bin/env bash
cd "${0%/*}" || exit 1

run="../../target/release/examples/perf_experiment"
cargo build --example perf_experiment --features=sled,rocks,faster --release

export NUM_OPS=1000000
# used only in a few experiments with limited number of keys
export NUM_KEYS=100

echo "bench_num,backend,session_length,num_ops,num_keys,key_size,value_size,time" >../../target/experiment_results.csv

for backend in InMemory Rocks Sled Faster; do
  for experiment in 1 2 3 4 5; do
    # session lengths matter only for the Faster backend
    for sess_len in $(if [[ $backend == "Faster" ]]; then echo 1 3 10 100; else echo 1; fi); do
      export SESSION_LENGTH=$sess_len
      for size_mul in 1 4 16 64; do
        export KEY_SIZE=$((8 * size_mul))
        export VALUE_SIZE=$((32 * size_mul))

        echo Warmup...
        $run $experiment $backend >/dev/null 2>&1
        $run $experiment $backend >/dev/null 2>&1

        echo "session reset = $sess_len, key size = $KEY_SIZE, value size = $VALUE_SIZE"
        if ! $run $experiment $backend >>../../target/experiment_results.csv; then
          # did not finish, time not printed
          echo "-1" >>../../target/experiment_results.csv
        fi
      done
    done
  done
done
