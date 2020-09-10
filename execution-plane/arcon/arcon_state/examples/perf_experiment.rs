extern crate arcon_state;
use arcon_state::*;
use once_cell::sync::OnceCell;
use std::{env, error::Error, iter};
use tempfile::tempdir;

bundle! {
    struct PerfBundle {
        values: Handle<MapState<Vec<u8>, Vec<u8>>>
    }
}

bundle! {
    struct PerfAggregatorBundle {
        value: Handle<AggregatorState<XoringAggregator>>
    }
}

#[derive(Debug, Clone)]
pub struct XoringAggregator;
impl Aggregator for XoringAggregator {
    type Input = Vec<u8>;
    type Accumulator = Vec<u8>;
    type Result = String;

    fn create_accumulator(&self) -> Self::Accumulator {
        vec![]
    }

    fn add(&self, acc: &mut Self::Accumulator, value: Self::Input) {
        if acc.len() < value.len() {
            acc.resize(value.len(), 0);
        }
        for (a, v) in acc.iter_mut().zip(value) {
            *a ^= v;
        }
    }

    fn merge_accumulators(
        &self,
        mut fst: Self::Accumulator,
        snd: Self::Accumulator,
    ) -> Self::Accumulator {
        if fst.len() < snd.len() {
            fst.resize(snd.len(), 0);
        }
        for (f, s) in fst.iter_mut().zip(snd) {
            *f ^= s;
        }

        fst
    }

    fn accumulator_into_result(&self, acc: Self::Accumulator) -> Self::Result {
        format!("{:?}", acc)
    }
}

static SESSION_LENGTH: OnceCell<usize> = OnceCell::new();
static NUM_OPS: OnceCell<usize> = OnceCell::new();
static NUM_KEYS: OnceCell<usize> = OnceCell::new();
static KEY_SIZE: OnceCell<usize> = OnceCell::new();
static VALUE_SIZE: OnceCell<usize> = OnceCell::new();

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    let bin_name = args[0].clone();
    let print_usage_and_exit = move || {
        println!(
            "Usage: {} <bench-num> <backend-name>\nMore opts through env vars.",
            bin_name
        );
        std::process::exit(1)
    };

    if args.len() < 3 {
        let _ = print_usage_and_exit();
    }

    let bench_num: u8 = args[1].parse()?;
    let backend: BackendType = args[2].parse()?;

    macro_rules! from_env {
        ($var_name:ident, $default:literal) => {
            $var_name
                .set(
                    env::var(stringify!($var_name))
                        .as_ref()
                        .map(|v| v.as_ref())
                        .unwrap_or(stringify!($default))
                        .parse()?,
                )
                .map_err(|_| concat!(stringify!($var_name), " once-cell was set previously"))?
        };
    }

    from_env!(SESSION_LENGTH, 10);
    from_env!(NUM_OPS, 1000000);
    from_env!(NUM_KEYS, 100);
    from_env!(KEY_SIZE, 8);
    from_env!(VALUE_SIZE, 32);

    let rng = fastrand::Rng::new();
    rng.seed(4); // chosen by fair dice roll

    eprintln!("Running bench #{} with {}", bench_num, backend);
    // print the first part of the csv line (the settings)
    print!(
        "{bench_num},{backend},{session_length},{num_ops},{num_keys},{key_size},{value_size},",
        bench_num = bench_num,
        backend = backend,
        session_length = SESSION_LENGTH.get().unwrap(),
        num_ops = NUM_OPS.get().unwrap(),
        num_keys = NUM_KEYS.get().unwrap(),
        key_size = KEY_SIZE.get().unwrap(),
        value_size = VALUE_SIZE.get().unwrap()
    );

    match bench_num {
        1 => random_read(backend, rng),
        2 => append_write(backend, rng),
        3 => overwrite(backend, rng),
        4 => naive_rmw(backend, rng),
        5 => specialized_rmw(backend, rng),
        x => {
            println!("unknown bench num: {}", x);
            println!(
                "\
                1. read random values from map state\n\
                2. blind append-only writes\n\
                3. blind overwrites\n\
                4. read-modify-write ex. on a map state\n\
                5. native read-modify-write (aggregate / reduce)\
                "
            );
            print_usage_and_exit()
        }
    }
}

fn make_key(i: usize, key_size: usize) -> Vec<u8> {
    i.to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take(key_size)
        .collect()
}

fn make_value(value_size: usize, rng: &fastrand::Rng) -> Vec<u8> {
    iter::repeat_with(|| rng.u8(..)).take(value_size).collect()
}

fn storage_config() -> StorageConfig {
    Default::default()
}

fn random_read(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-read"),
        };

        let value_size = *VALUE_SIZE.get().unwrap();
        let key_size = *KEY_SIZE.get().unwrap();

        let num_entries = 500_000usize;

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);

            let mut state = state.activate(&mut session);
            let mut values = state.values();

            for i in 0..num_entries {
                let value: Vec<_> = make_value(value_size, &rng);
                let key: Vec<_> = make_key(i, key_size);
                values.fast_insert(key, value)?;
            }
        }
        // init done

        measure(backend, |session| {
            let mut state = state.activate(session);
            let values = state.values();
            let key: Vec<_> = make_key(rng.usize(..num_entries), key_size);
            let _read_value = values.get(&key)?;
            Ok(())
        })?
    });

    Ok(())
}

fn append_write(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-append"),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);
        }
        // init done

        let value_size = *VALUE_SIZE.get().unwrap();
        let key_size = *KEY_SIZE.get().unwrap();

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let key = make_key(key_idx, key_size);
            let value: Vec<_> = make_value(value_size, &rng);
            values.fast_insert(key, value)?;
            key_idx += 1;
            Ok(())
        })?
    });

    Ok(())
}

fn overwrite(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-append"),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);
        }
        // init done

        let num_keys = *NUM_KEYS.get().unwrap();
        let value_size = *VALUE_SIZE.get().unwrap();
        let key_size = *KEY_SIZE.get().unwrap();

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let key = make_key(key_idx, key_size);
            let value = make_value(value_size, &rng);
            values.fast_insert(key, value)?;
            key_idx += 1;
            // reset the key idx every so often to overwrite the old values
            key_idx %= num_keys;

            Ok(())
        })?
    });

    Ok(())
}

fn naive_rmw(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-append"),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);
        }
        // init done

        let num_keys = *NUM_KEYS.get().unwrap();
        let value_size = *VALUE_SIZE.get().unwrap();
        let key_size = *KEY_SIZE.get().unwrap();

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let key = make_key(key_idx, key_size);

            // read
            let mut value = values.get(&key)?.unwrap_or(vec![]);
            // modify
            let random_bytes = make_value(value_size, &rng);
            if value.len() < random_bytes.len() {
                value.resize(random_bytes.len(), 0);
            }
            for (v, r) in value.iter_mut().zip(random_bytes) {
                *v ^= r;
            }
            // write
            values.fast_insert(key, value)?;

            key_idx += 1;
            key_idx %= num_keys;

            Ok(())
        })?
    });

    Ok(())
}

fn specialized_rmw(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfAggregatorBundle {
            value: Handle::aggregator("perf-bundle-map-append", XoringAggregator),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);
        }
        // init done

        let value_size = *VALUE_SIZE.get().unwrap();

        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut value = state.value();
            let random_bytes = make_value(value_size, &rng);

            value.aggregate(random_bytes)?;

            Ok(())
        })?
    });

    Ok(())
}

fn measure<B: Backend>(
    backend: BackendContainer<B>,
    mut f: impl FnMut(&mut Session<B>) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    eprint!("Measurement started... ");
    let session_length = *SESSION_LENGTH.get().unwrap();
    let num_ops = *NUM_OPS.get().unwrap();

    let start = std::time::Instant::now();

    let mut session = backend.session();
    for i in 0..num_ops {
        f(&mut session)?;

        if i % session_length == 0 {
            drop(session);
            session = backend.session();
        }
    }

    let elapsed = start.elapsed();
    eprintln!("Done! {:?}", elapsed);
    println!("{}", elapsed.as_millis());

    Ok(())
}
