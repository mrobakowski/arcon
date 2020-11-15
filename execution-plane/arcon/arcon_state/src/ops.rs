// Copyright (c) 2020, KTH Royal Institute of Technology.
// SPDX-License-Identifier: AGPL-3.0-only
use crate::{
    error::*, handles::BoxedIteratorOfResult, Aggregator, AggregatorState, Handle, Key, MapState,
    Metakey, Reducer, ReducerState, Value, ValueState, VecState,
};
use std::convert::TryInto;

pub trait ValueOps {
    fn value_clear<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<ValueState<T>, IK, N>,
    ) -> Result<()>;

    fn value_get<T: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<ValueState<T>, IK, N>,
    ) -> Result<Option<T>>;

    fn value_set<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<ValueState<T>, IK, N>,
        value: T,
    ) -> Result<Option<T>>;

    fn value_fast_set<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<ValueState<T>, IK, N>,
        value: T,
    ) -> Result<()>;
}

pub trait MapOps {
    #[cfg(feature = "fill_up")]
    fn fill_up(&mut self, handle: &Handle<MapState<Vec<u8>, Vec<u8>>>, num_bytes: usize, key_size: usize, value_size: usize) -> Result<()> {
        let rnd = fastrand::Rng::new();
        const MAX_BYTES_ONE_INSERT: i64 = 2 * 1024 * 1024;
        let record_size = (key_size + value_size) as i64;
        let serialized_record_size = {
            let mut key = Vec::new();
            let mut value = Vec::new();
            key.resize_with(key_size, || rnd.u8(..));
            value.resize_with(value_size, || rnd.u8(..));

            let serialized_key = handle.serialize_metakeys_and_key(&key)?;
            let serialized_value = crate::serialization::protobuf::serialize(&value)?;

            (serialized_key.len() + serialized_value.len()) as i64
        };
        let mut left: i64 = num_bytes.try_into().unwrap();

        while left > 0 {
            let this_batch = i64::min(left, MAX_BYTES_ONE_INSERT);
            let num_records = (this_batch / serialized_record_size) + 1;
            let bytes_in_this_batch = num_records * serialized_record_size;

            let mut records = Vec::new();
            records.resize_with(num_records as usize, || {
                let mut key = Vec::new();
                let mut value = Vec::new();
                key.resize_with(key_size, || rnd.u8(..));
                value.resize_with(key_size, || rnd.u8(..));
                (key, value)
            });

            self.map_insert_all(handle, records)?;

            eprintln!("bytes in this batch: {}, left: {}", bytes_in_this_batch, left);

            left -= bytes_in_this_batch;
        }

        Ok(())
    }

    fn map_clear<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<()>;

    fn map_get<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: &K,
    ) -> Result<Option<V>>;

    fn map_fast_insert<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: K,
        value: V,
    ) -> Result<()>;

    fn map_insert<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: K,
        value: V,
    ) -> Result<Option<V>>;

    fn map_insert_all<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key_value_pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()>;

    fn map_remove<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: &K,
    ) -> Result<Option<V>>;

    fn map_fast_remove<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: &K,
    ) -> Result<()>;

    fn map_contains<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
        key: &K,
    ) -> Result<bool>;

    fn map_iter<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<BoxedIteratorOfResult<(K, V)>>;

    fn map_keys<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<BoxedIteratorOfResult<K>>;

    fn map_values<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<BoxedIteratorOfResult<V>>;

    fn map_len<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<usize>;

    fn map_is_empty<K: Key, V: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<MapState<K, V>, IK, N>,
    ) -> Result<bool>;
}

pub trait VecOps {
    fn vec_clear<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<VecState<T>, IK, N>,
    ) -> Result<()>;
    fn vec_append<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<VecState<T>, IK, N>,
        value: T,
    ) -> Result<()>;
    fn vec_get<T: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<VecState<T>, IK, N>,
    ) -> Result<Vec<T>>;
    fn vec_iter<T: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<VecState<T>, IK, N>,
    ) -> Result<BoxedIteratorOfResult<T>>;
    fn vec_set<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<VecState<T>, IK, N>,
        value: Vec<T>,
    ) -> Result<()>;
    fn vec_add_all<T: Value, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<VecState<T>, IK, N>,
        values: impl IntoIterator<Item = T>,
    ) -> Result<()>;
    fn vec_len<T: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<VecState<T>, IK, N>,
    ) -> Result<usize>;
    fn vec_is_empty<T: Value, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<VecState<T>, IK, N>,
    ) -> Result<bool>;
}

pub trait ReducerOps {
    fn reducer_clear<T: Value, F: Reducer<T>, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<ReducerState<T, F>, IK, N>,
    ) -> Result<()>;
    fn reducer_get<T: Value, F: Reducer<T>, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<ReducerState<T, F>, IK, N>,
    ) -> Result<Option<T>>;
    fn reducer_reduce<T: Value, F: Reducer<T>, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<ReducerState<T, F>, IK, N>,
        value: T,
    ) -> Result<()>;
}

pub trait AggregatorOps {
    fn aggregator_clear<A: Aggregator, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<AggregatorState<A>, IK, N>,
    ) -> Result<()>;
    fn aggregator_get<A: Aggregator, IK: Metakey, N: Metakey>(
        &self,
        handle: &Handle<AggregatorState<A>, IK, N>,
    ) -> Result<A::Result>;
    fn aggregator_aggregate<A: Aggregator, IK: Metakey, N: Metakey>(
        &mut self,
        handle: &Handle<AggregatorState<A>, IK, N>,
        value: A::Input,
    ) -> Result<()>;
}
