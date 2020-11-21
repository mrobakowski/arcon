#!/usr/bin/env bash
cd "${0%/*}" || exit 1

run="../../target/release/examples/resource_usage_experiment"
cargo build --example resource_usage_experiment --features=sled,rocks,faster,fill_up --release || exit 1

export SESSION_LENGTH=10
# used only in a few experiments with limited number of keys
export NUM_KEYS=2500000
export TIMEOUT_SECS=120

EXPERIMENT_RESULTS="res_usage/experiment_results.csv"
EXPERIMENT_OPS="res_usage/experiment_results_ops.csv"

mkdir res_usage
#rm $EXPERIMENT_RESULTS
#rm $EXPERIMENT_OPS
#rm res_usage/raw_results_*

for experiment in 33 #1 2 3 4 5
do
  echo "experiment,key_size,value_size,Rocks,Sled,Faster (10)" >>$EXPERIMENT_RESULTS
  echo "experiment,key_size,value_size,Rocks,Sled,Faster (10)" >>$EXPERIMENT_OPS
  for size_mul in 1 # 4 16 64
  do
    export KEY_SIZE=$((8 * size_mul))
    export VALUE_SIZE=$((32 * size_mul))
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>$EXPERIMENT_RESULTS
    echo -n "$experiment,$KEY_SIZE,$VALUE_SIZE" >>$EXPERIMENT_OPS
    for backend in Rocks Sled Faster # InMemory
    do
      export OUT_FILE="STDOUT"

      echo -n "," >>$EXPERIMENT_RESULTS
      echo -n "," >>$EXPERIMENT_OPS
#      echo "Warmup... ($experiment, $backend)"
#      $run $experiment $backend >/dev/null 2>&1
#      $run $experiment $backend >/dev/null 2>&1

      echo "session reset = $SESSION_LENGTH, key size = $KEY_SIZE, value size = $VALUE_SIZE"

      export OUT_FILE="res_usage/raw_results_${experiment}_${KEY_SIZE}_${VALUE_SIZE}_${backend}"

      res=$(pidstat -druh 1 -e nocache $run $experiment $backend)
      if [ $? -eq 0 ]; then
        RES=$(cut -d, -f 8 "$OUT_FILE" | tr -d '\n')
        echo -n "${RES:--1}" >>$EXPERIMENT_RESULTS
        OPS=$(cut -d, -f 9 "$OUT_FILE" | tr -d '\n')
        echo -n "${OPS:--1}" >>$EXPERIMENT_OPS
        echo -e "\n\nresource usage stats\n" >>$OUT_FILE
        echo "$res" >>$OUT_FILE
      else
        # did not finish, time not printed
        echo -n "-1" >>$EXPERIMENT_RESULTS
        echo -n "-1" >>$EXPERIMENT_OPS
        echo -e "\n\nresource usage stats\n" >>$OUT_FILE
        echo "$res" >>$OUT_FILE
      fi
    done
    echo "" >>$EXPERIMENT_RESULTS
    echo "" >>$EXPERIMENT_OPS
  done
done
