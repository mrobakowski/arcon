extern crate arcon_state;
use arcon_state::*;
use std::{
    env,
    error::Error,
    iter,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tempfile::tempdir_in;

bundle! {
    struct PerfBundle {
        values: Handle<MapState<Vec<u8>, Vec<u8>>>
    }
}

bundle! {
    struct PerfAggregatorBundle {
        value: Handle<AggregatorState<XoringAggregator>, usize>
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

macro_rules! env_var {
    ($name:ident : $typ:ty = $default:expr) => {
        static $name: once_cell::sync::Lazy<$typ> = once_cell::sync::Lazy::new(|| {
            env::var(stringify!($name))
                .as_ref()
                .map(|v| v.as_ref())
                .unwrap_or(stringify!($default))
                .parse()
                .expect(concat!(
                    "Could not parse environment variable ",
                    stringify!($name),
                    " (expected type:",
                    stringify!($typ),
                    ")"
                ))
        });
    };
}

env_var!(SESSION_LENGTH: usize = 10);
env_var!(NUM_OPS: usize = 1000000);
env_var!(NUM_KEYS: usize = 100);
env_var!(KEY_SIZE: usize = 8);
env_var!(VALUE_SIZE: usize = 32);
// 352 megabytes is the minimum size that FASTER doesn't hang up on
env_var!(MEM_SIZE_HINT_MB: u64 = 352);

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

    let rng = fastrand::Rng::new();
    rng.seed(4); // chosen by fair dice roll

    eprintln!("Running bench #{} with {}", bench_num, backend);
    // print the first part of the csv line (the settings)
    print!(
        "{bench_num},{backend},{session_length},{num_ops},{num_keys},{key_size},{value_size},",
        bench_num = bench_num,
        backend = backend,
        session_length = *SESSION_LENGTH,
        num_ops = *NUM_OPS,
        num_keys = *NUM_KEYS,
        key_size = *KEY_SIZE,
        value_size = *VALUE_SIZE
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
    StorageConfig {
        mem_size_hint: Some(*MEM_SIZE_HINT_MB * 1024 * 1024),
    }
}

fn random_read(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir_in(std::env::current_dir()?)?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-read"),
        };

        let value_size = *VALUE_SIZE;
        let key_size = *KEY_SIZE;

        let num_entries = 5_000_000usize;

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
            let key: Vec<_> = make_key(rng.usize(0..num_entries), key_size);
            let _read_value = values.get(&key)?;
            Ok(())
        })?
    });

    Ok(())
}

fn append_write(backend: BackendType, rng: fastrand::Rng) -> Result<(), Box<dyn Error>> {
    let dir = tempdir_in(std::env::current_dir()?)?;
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

        let value_size = *VALUE_SIZE;
        let key_size = *KEY_SIZE;

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
    let dir = tempdir_in(std::env::current_dir()?)?;
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

        let num_keys = *NUM_KEYS;
        let value_size = *VALUE_SIZE;
        let key_size = *KEY_SIZE;

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
    let dir = tempdir_in(std::env::current_dir()?)?;
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

        let num_keys = *NUM_KEYS;
        let value_size = *VALUE_SIZE;
        let key_size = *KEY_SIZE;

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
    let dir = tempdir_in(std::env::current_dir()?)?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref(), &storage_config())?;
        let mut state = PerfAggregatorBundle {
            value: Handle::aggregator("perf-bundle-map-append", XoringAggregator)
                .with_item_key(0usize),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);
        }
        // init done

        let value_size = *VALUE_SIZE;
        let num_keys = *NUM_KEYS;

        let mut key = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut value = state.value();
            value.set_item_key(key);
            let random_bytes = make_value(value_size, &rng);

            value.aggregate(random_bytes)?;

            key += 1;
            key %= num_keys;

            Ok(())
        })?
    });

    Ok(())
}

static TIMED_OUT: AtomicBool = AtomicBool::new(true);
fn reset_timeout() {
    if !TIMED_OUT.load(Ordering::Relaxed) {
        panic!("Cannot reset an initialized timeout")
    }
    TIMED_OUT.store(false, Ordering::Relaxed);

    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10 * 60));
        TIMED_OUT.store(true, Ordering::Relaxed);
    });
}

fn check_timeout() -> bool {
    TIMED_OUT.load(Ordering::Relaxed)
}

fn measure<B: Backend>(
    backend: BackendContainer<B>,
    mut f: impl FnMut(&mut Session<B>) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    eprint!("Measurement started... ");
    let session_length = *SESSION_LENGTH;
    let num_ops = *NUM_OPS;

    let start = std::time::Instant::now();
    reset_timeout();

    let mut session = backend.session();
    let mut ops_done = 0usize;
    for i in 0..num_ops {
        if check_timeout() {
            eprintln!("Timed out after {} ops!", ops_done);
            break;
        }

        f(&mut session)?;
        ops_done += 1;

        if i % session_length == 0 {
            drop(session);
            session = backend.session();
        }
    }

    let elapsed = start.elapsed();
    eprintln!("Done! {:?}", elapsed);
    println!("{},{}", elapsed.as_nanos() / (ops_done as u128), ops_done);

    Ok(())
}
