#!/usr/bin/env bash
cd "${0%/*}" || exit 1

run="../../target/release/examples/perf_experiment"
cargo build --example perf_experiment --features=sled,rocks,faster --release

export NUM_OPS=5000000
# used only in a few experiments with limited number of keys
export NUM_KEYS=2500000

rm ../../target/experiment_results.csv

for experiment in 1 2 3 4 5; do
  echo "experiment,key_size,value_size,InMemory,Rocks,Sled,Faster (1),Faster (3),Faster (10),Faster (100)" >>../../target/experiment_results.csv
  for size_mul in 1 4 16 64; do
    export KEY_SIZE=$((8 * size_mul))
    export VALUE_SIZE=$((32 * size_mul))
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>../../target/experiment_results.csv
    for backend in InMemory Rocks Sled Faster; do
      # session lengths matter only for the Faster backend
      for sess_len in $(if [[ $backend == "Faster" ]]; then echo 1 3 10 100; else echo 1; fi); do
        export SESSION_LENGTH=$sess_len

        echo -n "," >>../../target/experiment_results.csv
        echo Warmup...
        $run $experiment $backend >/dev/null 2>&1
        $run $experiment $backend >/dev/null 2>&1

        echo "session reset = $sess_len, key size = $KEY_SIZE, value size = $VALUE_SIZE"
        res=$($run $experiment $backend)
        if [ $? -eq 0 ]; then
          echo "$res" | cut -d, -f 8 | tr -d '\n' >>../../target/experiment_results.csv
        else
          # did not finish, time not printed
          echo -n "-1" >>../../target/experiment_results.csv
        fi
      done
    done
    echo "" >>../../target/experiment_results.csv
  done
done

#export KEY_SIZE=512
#export VALUE_SIZE=2048
#export NUM_OPS=1000000
#export NUM_KEYS=100
#export SESSION_LENGTH=10
#cargo flamegraph --example perf_experiment --open --features=rocks,sled,faster -- 5 Faster
