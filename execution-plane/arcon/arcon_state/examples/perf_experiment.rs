#![feature(proc_macro_hygiene)]

extern crate arcon_state;
use arcon_state::*;
use ct_python::ct_python;
use once_cell::sync::OnceCell;
use std::{env, error::Error};
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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    let bin_name = args[0].clone();
    let print_usage_and_exit = move || {
        println!("Usage: {} <bench-num> <backend-name>", bin_name);
        std::process::exit(1)
    };

    if args.len() < 3 {
        print_usage_and_exit();
    }

    let bench_num: u8 = args[1].parse()?;
    let backend: BackendType = args[2].parse()?;

    SESSION_LENGTH
        .set(
            env::var("SESSION_LENGTH")
                .unwrap_or_else(|_| "10".into())
                .parse()?,
        )
        .map_err(|_| String::from("Session length set previously"))?;
    NUM_OPS
        .set(
            env::var("NUM_OPS")
                .unwrap_or_else(|_| "1000000".into())
                .parse()?,
        )
        .map_err(|_| String::from("Num ops set previously"))?;
    NUM_KEYS
        .set(
            env::var("NUM_KEYS")
                .unwrap_or_else(|_| "100".into())
                .parse()?,
        )
        .map_err(|_| String::from("Num keys set previously"))?;

    fastrand::seed(4); // chosen by fair dice roll

    println!("Running bench #{} with {}", bench_num, backend);

    match bench_num {
        1 => random_read(backend),
        2 => append_write(backend),
        3 => overwrite(backend),
        4 => naive_rmw(backend),
        5 => specialized_rmw(backend),
        x => {
            println!("unknown bench num: {}", x);
            println!(
                "\
                1. read random values from mapstate\n\
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

const WORDS: &[&str] = ct_python! {
    import os.path
    import string
    import random

    print("&[")

    // if the system has a dict file, let's read that
    if os.path.isfile("/usr/share/dict/words"):
        with open("/usr/share/dict/words") as f:
            for line in f:
                l = line.rstrip()
                print(f"r##\"{l}\"##,")
    // otherwise let's generate some random stuff
    else:
        for _ in range(500000):
            word = "".join(random.choice(string.ascii_lowercase) for _ in range(7))
            print(f"r##\"{word}\"##,")

    print("]")
};

static SESSION_LENGTH: OnceCell<usize> = OnceCell::new();
static NUM_OPS: OnceCell<usize> = OnceCell::new();
static NUM_KEYS: OnceCell<usize> = OnceCell::new();

fn random_read(backend: BackendType) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref())?;
        let mut state = PerfBundle {
            values: Handle::map("perf-bundle-map-read"),
        };

        // init
        {
            let mut session = backend.session();
            let mut rtok = unsafe { RegistrationToken::new(&mut session) };
            state.register_states(&mut rtok);

            let mut state = state.activate(&mut session);
            let mut values = state.values();

            for (i, word) in WORDS.iter().enumerate() {
                let bytes = word.to_string().into_bytes();
                values.fast_insert(i.to_le_bytes().to_vec(), bytes)?;
            }
        }
        // init done

        measure(backend, |session| {
            let mut state = state.activate(session);
            let values = state.values();
            let key = fastrand::usize(..WORDS.len()).to_le_bytes().to_vec();
            let _read_value = values.get(&key)?;
            Ok(())
        })?
    });

    Ok(())
}

fn append_write(backend: BackendType) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref())?;
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

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let word_idx = fastrand::usize(..WORDS.len());
            let key = key_idx.to_le_bytes().to_vec();
            values.fast_insert(key, WORDS[word_idx].to_string().into_bytes())?;
            key_idx += 1;
            Ok(())
        })?
    });

    Ok(())
}

fn overwrite(backend: BackendType) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref())?;
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

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let word_idx = fastrand::usize(..WORDS.len());
            let key = key_idx.to_le_bytes().to_vec();
            values.fast_insert(key, WORDS[word_idx].to_string().into_bytes())?;
            key_idx += 1;
            // reset the key idx every so often to overwrite the old values
            key_idx %= num_keys;

            Ok(())
        })?
    });

    Ok(())
}

fn naive_rmw(backend: BackendType) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref())?;
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

        let mut key_idx = 0usize;
        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut values = state.values();
            let word_idx = fastrand::usize(..WORDS.len());
            let key = key_idx.to_le_bytes().to_vec();

            // read
            let mut value = values.get(&key)?.unwrap_or(vec![]);
            // modify
            let random_word = WORDS[word_idx].to_string().into_bytes();
            if value.len() < random_word.len() {
                value.resize(random_word.len(), 0);
            }
            for (v, r) in value.iter_mut().zip(random_word) {
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

fn specialized_rmw(backend: BackendType) -> Result<(), Box<dyn Error>> {
    let dir = tempdir()?;
    with_backend_type!(backend, |B| {
        let backend = B::create(dir.as_ref())?;
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

        measure(backend, |session| {
            let mut state = state.activate(session);
            let mut value = state.value();
            let word_idx = fastrand::usize(..WORDS.len());
            let random_word = WORDS[word_idx].to_string().into_bytes();

            value.aggregate(random_word)?;

            Ok(())
        })?
    });

    Ok(())
}

fn measure<B: Backend>(
    backend: BackendContainer<B>,
    mut f: impl FnMut(&mut Session<B>) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    println!("Measurement started...");
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
    println!("Total time: {:?}", elapsed);

    Ok(())
}
