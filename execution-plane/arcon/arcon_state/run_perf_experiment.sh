#!/usr/bin/env bash
cd "${0%/*}" || exit 1

run="../../target/release/examples/perf_experiment"
cargo build --example perf_experiment --features=sled,rocks,faster --release

export NUM_OPS=5000000
# used only in a few experiments with limited number of keys
export NUM_KEYS=2500000

EXPERIMENT_RESULTS="res/experiment_results.csv"
EXPERIMENT_OPS="res/experiment_results_ops.csv"

mkdir res
rm $EXPERIMENT_RESULTS
rm $EXPERIMENT_OPS
rm res/raw_results_*

for experiment in 1 2 3 4 5; do
  echo "experiment,key_size,value_size,InMemory,Rocks,Sled,Faster (1),Faster (3),Faster (10),Faster (100)" >>$EXPERIMENT_RESULTS
  echo "experiment,key_size,value_size,InMemory,Rocks,Sled,Faster (1),Faster (3),Faster (10),Faster (100)" >>$EXPERIMENT_OPS
  for size_mul in 1 4 16 64; do
    export KEY_SIZE=$((8 * size_mul))
    export VALUE_SIZE=$((32 * size_mul))
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>$EXPERIMENT_RESULTS
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>$EXPERIMENT_OPS
    for backend in InMemory Rocks Sled Faster; do
      # session lengths matter only for the Faster backend
      for sess_len in $(if [[ $backend == "Faster" ]]; then echo 1 3 10 100; else echo 1; fi); do
        export SESSION_LENGTH=$sess_len
        export OUT_FILE="STDOUT"

        echo -n "," >>$EXPERIMENT_RESULTS
        echo -n "," >>$EXPERIMENT_OPS
        echo Warmup...
        $run $experiment $backend >/dev/null 2>&1
        $run $experiment $backend >/dev/null 2>&1

        echo "session reset = $sess_len, key size = $KEY_SIZE, value size = $VALUE_SIZE"

        export OUT_FILE="res/raw_results_${experiment}_${KEY_SIZE}_${VALUE_SIZE}_${backend}_${sess_len}"

        res=$(pidstat -druh 1 -e nocache $run $experiment $backend)
        if [ $? -eq 0 ]; then
          cut -d, -f 8 "$OUT_FILE" | tr -d '\n' >>$EXPERIMENT_RESULTS
          cut -d, -f 9 "$OUT_FILE" | tr -d '\n' >>$EXPERIMENT_OPS
          echo -e "\n\nresource usage stats\n" >>$OUT_FILE
          echo "$res" >>$OUT_FILE
        else
          # did not finish, time not printed
          echo -n "-1" >>$EXPERIMENT_RESULTS
          echo -n "-1" >>$EXPERIMENT_OPS
        fi
      done
    done
    echo "" >>$EXPERIMENT_RESULTS
    echo "" >>$EXPERIMENT_OPS
  done
done
