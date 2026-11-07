use rand::{rngs::StdRng, Rng, SeedableRng};
use std::{
    fs::File,
    io::{BufReader, Cursor, Write},
    path::PathBuf,
};

use ccsds::framing::{
    Block, DefaultDerandomizer, DefaultReedSolomon, Derandomizer, Integrity, ReedSolomon, SyncOpts,
    ASM,
};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push(name);
    path
}

/// Length of a CADU block (not including the ASM).
const BLOCK_LEN: usize = 1020;
/// Number of CADUs in the generated streams.
const NUM_CADUS: usize = 256;

/// Create `n` CADUs, i.e., ASM followed by `BLOCK_LEN` pseudo-random bytes. The seed
/// is fixed so every run sees the same data.
fn make_cadus(n: usize) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(0xCC5D5);
    let mut out = Vec::with_capacity(n * (ASM.len() + BLOCK_LEN));
    for _ in 0..n {
        out.extend_from_slice(&ASM);
        out.extend((0..BLOCK_LEN).map(|_| rng.gen::<u8>()));
    }
    out
}

/// Shift the whole bit stream right by `k` bits (0 < k < 8), i.e., the data is no longer
/// byte-aligned. The output is one byte longer than the input.
fn shift_right(dat: &[u8], k: u32) -> Vec<u8> {
    assert!(k > 0 && k < 8);
    let mut out = vec![0u8; dat.len() + 1];
    for (i, b) in dat.iter().enumerate() {
        out[i] |= b >> k;
        out[i + 1] = b << (8 - k);
    }
    out
}

fn bench_synchronization(c: &mut Criterion) {
    let mut group = c.benchmark_group("synchronize");

    // Original case: no sync marker present, so every position is searched.
    let zeros = vec![0u8; 1024];
    group.throughput(Throughput::Bytes(zeros.len() as u64));
    group.bench_function("loop", |b| {
        b.iter_batched(
            || Cursor::new(zeros.clone()),
            |r| {
                let sync = ccsds::framing::synchronize(r, SyncOpts::new(BLOCK_LEN));
                let _: Vec<Block> = sync.into_iter().map_while(Result::ok).collect();
            },
            BatchSize::LargeInput,
        );
    });

    let aligned = make_cadus(NUM_CADUS);

    // Locked: back-to-back CADUs, byte aligned.
    // Locked, shifted: same, but the stream is shifted by 3 bits.
    // Acquisition: 1 MiB of non-sync data in front of the first CADU.
    let mut noise_rng = StdRng::seed_from_u64(0x5EED);
    let mut acquire: Vec<u8> = (0..(1usize << 20)).map(|_| noise_rng.gen::<u8>()).collect();
    acquire.extend_from_slice(&aligned);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("locked_aligned", aligned.clone()),
        ("locked_shifted_3", shift_right(&aligned, 3)),
        ("acquire_aligned", acquire),
    ];
    for (name, data) in cases {
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_function(name, |b| {
            b.iter_batched(
                || Cursor::new(data.clone()),
                |r| {
                    let sync = ccsds::framing::synchronize(r, SyncOpts::new(BLOCK_LEN));
                    let count = sync.into_iter().map_while(Result::ok).count();
                    assert_eq!(count, NUM_CADUS, "unexpected block count for {name}");
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Same synchronization work as `bench_synchronization`, but reading from a real file so
/// the cost of the 1-byte `read()` calls in `Bytes::next` shows up. Each stream is read
/// with a raw `File` (one system call per byte) and with a `BufReader<File>`.
///
/// The file is in the page cache after the first iteration, so this measures system call
/// overhead, not disk speed.
fn bench_synchronization_file(c: &mut Criterion) {
    let aligned = make_cadus(NUM_CADUS);
    let mut noise_rng = StdRng::seed_from_u64(0x5EED);
    let mut acquire: Vec<u8> = (0..(1usize << 20)).map(|_| noise_rng.gen::<u8>()).collect();
    acquire.extend_from_slice(&aligned);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("locked_aligned", aligned.clone()),
        ("locked_shifted_3", shift_right(&aligned, 3)),
        ("acquire_aligned", acquire),
    ];

    let mut group = c.benchmark_group("synchronize_file");
    // A raw File does a system call per byte, so each iteration is slow.
    group.sample_size(10);

    for (name, data) in cases {
        let mut tmp = tempfile::NamedTempFile::new().expect("temp file");
        tmp.write_all(&data).expect("write temp file");
        tmp.flush().expect("flush temp file");
        group.throughput(Throughput::Bytes(data.len() as u64));

        group.bench_function(format!("{name}/file"), |b| {
            b.iter_batched(
                || File::open(tmp.path()).expect("open temp file"),
                |r| {
                    let sync = ccsds::framing::synchronize(r, SyncOpts::new(BLOCK_LEN));
                    let count = sync.into_iter().map_while(Result::ok).count();
                    assert_eq!(count, NUM_CADUS, "unexpected block count for {name}");
                },
                BatchSize::PerIteration,
            );
        });
        group.bench_function(format!("{name}/bufreader"), |b| {
            b.iter_batched(
                || BufReader::new(File::open(tmp.path()).expect("open temp file")),
                |r| {
                    let sync = ccsds::framing::synchronize(r, SyncOpts::new(BLOCK_LEN));
                    let count = sync.into_iter().map_while(Result::ok).count();
                    assert_eq!(count, NUM_CADUS, "unexpected block count for {name}");
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn bench_rs_correct_codeblock(c: &mut Criterion) {
    let mut block = std::fs::read(fixture_path("benches/snpp_block.dat")).unwrap();

    // introduced some errors
    let block = {
        let mut rng = rand::thread_rng();
        let b = &mut block[4..];
        let idx: usize = rng.gen::<u8>().into();
        b[idx] = b[idx].wrapping_add(1);
        b
    };

    let mut group = c.benchmark_group("rs");
    group.throughput(Throughput::Bytes(1020));
    group.bench_function("correct_codeblock", |b| {
        b.iter(|| {
            let rs = DefaultReedSolomon::new(4);
            let (i, _) = rs.perform(block).unwrap();
            assert_eq!(
                i,
                Integrity::Corrected,
                "expected to have corrected block; got {i:?}"
            );
        });
    });
    group.finish();
}

// Pn decode a random slice of data.
fn bench_derandomize(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 1020];
    for b in buf.iter_mut() {
        let f: u8 = rng.gen();
        *b = f;
    }

    let mut group = c.benchmark_group("derandomize");
    group.throughput(Throughput::Bytes(buf.len() as u64));
    group.bench_function("loop", |b| {
        b.iter(|| {
            let pn = DefaultDerandomizer;
            let _ = pn.derandomize(&buf.clone());
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_derandomize,
    bench_rs_correct_codeblock,
    bench_synchronization,
    bench_synchronization_file,
);
criterion_main!(benches);
