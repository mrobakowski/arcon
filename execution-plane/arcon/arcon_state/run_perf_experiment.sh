#!/usr/bin/env bash
cd "${0%/*}" || exit 1

run="../../target/release/examples/perf_experiment"
cargo build --example perf_experiment --features=sled,rocks,faster --release

export NUM_OPS=5000000
# used only in a few experiments with limited number of keys
export NUM_KEYS=2500000

rm ../../target/experiment_results.csv
rm ../../target/experiment_results_ops.csv

for experiment in 1 2 3 4 5; do
  echo "experiment,key_size,value_size,InMemory,Rocks,Sled,Faster (1),Faster (3),Faster (10),Faster (100)" >>../../target/experiment_results.csv
  echo "experiment,key_size,value_size,InMemory,Rocks,Sled,Faster (1),Faster (3),Faster (10),Faster (100)" >>../../target/experiment_results_ops.csv
  for size_mul in 1 4 16 64; do
    export KEY_SIZE=$((8 * size_mul))
    export VALUE_SIZE=$((32 * size_mul))
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>../../target/experiment_results.csv
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>../../target/experiment_results_ops.csv
    for backend in InMemory Rocks Sled Faster; do
      # session lengths matter only for the Faster backend
      for sess_len in $(if [[ $backend == "Faster" ]]; then echo 1 3 10 100; else echo 1; fi); do
        export SESSION_LENGTH=$sess_len

        echo -n "," >>../../target/experiment_results.csv
        echo -n "," >>../../target/experiment_results_ops.csv
        echo Warmup...
        pidstat -druh 2 -e $run $experiment $backend >/dev/null 2>&1
        $run $experiment $backend >/dev/null 2>&1

        echo "session reset = $sess_len, key size = $KEY_SIZE, value size = $VALUE_SIZE"
        res=$($run $experiment $backend)
        if [ $? -eq 0 ]; then
          echo "$res" | cut -d, -f 8 | tr -d '\n' >>../../target/experiment_results.csv
          echo "$res" | cut -d, -f 9 | tr -d '\n' >>../../target/experiment_results_ops.csv
        else
          # did not finish, time not printed
          echo -n "-1" >>../../target/experiment_results.csv
          echo -n "-1" >>../../target/experiment_results_ops.csv
        fi
      done
    done
    echo "" >>../../target/experiment_results.csv
    echo "" >>../../target/experiment_results_ops.csv
  done
done
