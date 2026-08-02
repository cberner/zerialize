//! Encode and decode speed for zerialize, rkyv and flatbuffers over the same
//! records.
//!
//! Decoding is measured as decode plus a full traversal, because a format that
//! defers work to field access has not saved that work, it has moved it. All
//! three decoders check the buffer they are given.

use criterion::{Criterion, criterion_group, criterion_main};
use flatbuffers::FlatBufferBuilder;
use rkyv::rancor::Error;
use rkyv::util::AlignedVec;
use std::hint::black_box;
use zerialize_benchmarks::{Data, data, flat, rk, zer};

/// The bound `rkyv::to_bytes` asks of what it serializes, named once rather
/// than spelled out at every call.
pub trait Serializable<'a>:
    rkyv::Serialize<
        rkyv::api::high::HighSerializer<AlignedVec, rkyv::ser::allocator::ArenaHandle<'a>, Error>,
    >
{
}

impl<'a, T> Serializable<'a> for T where
    T: rkyv::Serialize<
            rkyv::api::high::HighSerializer<
                AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                Error,
            >,
        >
{
}

fn rkyv_encoded<T: for<'a> Serializable<'a>>(source: &T) -> AlignedVec {
    rkyv::to_bytes::<Error>(source).unwrap()
}

fn encoding(criterion: &mut Criterion, data: &Data) {
    let mut group = criterion.benchmark_group("encode");
    let mut out = Vec::new();
    let mut builder = FlatBufferBuilder::new();

    let point = zer::point(data);
    let team = zer::team(data);
    let series = zer::series(data);
    let rk_point = rk::point(data);
    let rk_team = rk::team(data);
    let rk_series = rk::series(data);

    group.bench_function("point/zerialize", |b| {
        b.iter(|| zer::encode_point(black_box(&point), &mut out))
    });
    group.bench_function("point/rkyv", |b| {
        b.iter(|| black_box(rkyv_encoded(black_box(&rk_point)).len()))
    });
    group.bench_function("point/flatbuffers", |b| {
        b.iter(|| flat::encode_point(black_box(data), &mut builder))
    });

    group.bench_function("team/zerialize", |b| {
        b.iter(|| zer::encode_team(black_box(&team), &mut out))
    });
    group.bench_function("team/rkyv", |b| {
        b.iter(|| black_box(rkyv_encoded(black_box(&rk_team)).len()))
    });
    group.bench_function("team/flatbuffers", |b| {
        b.iter(|| flat::encode_team(black_box(data), &mut builder))
    });

    group.bench_function("series/zerialize", |b| {
        b.iter(|| zer::encode_series(black_box(&series), &mut out))
    });
    group.bench_function("series/rkyv", |b| {
        b.iter(|| black_box(rkyv_encoded(black_box(&rk_series)).len()))
    });
    group.bench_function("series/flatbuffers", |b| {
        b.iter(|| flat::encode_series(black_box(data), &mut builder))
    });

    group.finish();
}

fn decoding(criterion: &mut Criterion, data: &Data) {
    let mut group = criterion.benchmark_group("decode");
    let mut builder = FlatBufferBuilder::new();

    let mut zer_point = Vec::new();
    zer::encode_point(&zer::point(data), &mut zer_point);
    let mut zer_team = Vec::new();
    zer::encode_team(&zer::team(data), &mut zer_team);
    let mut zer_series = Vec::new();
    zer::encode_series(&zer::series(data), &mut zer_series);

    let rk_point = rkyv_encoded(&rk::point(data));
    let rk_team = rkyv_encoded(&rk::team(data));
    let rk_series = rkyv_encoded(&rk::series(data));

    flat::encode_point(data, &mut builder);
    let flat_point = builder.finished_data().to_vec();
    flat::encode_team(data, &mut builder);
    let flat_team = builder.finished_data().to_vec();
    flat::encode_series(data, &mut builder);
    let flat_series = builder.finished_data().to_vec();

    group.bench_function("point/zerialize", |b| {
        b.iter(|| zer::read_point(black_box(&zer_point)))
    });
    group.bench_function("point/rkyv", |b| {
        b.iter(|| rk::read_point(black_box(&rk_point)))
    });
    group.bench_function("point/flatbuffers", |b| {
        b.iter(|| flat::read_point(black_box(&flat_point)))
    });

    group.bench_function("team/zerialize", |b| {
        b.iter(|| zer::read_team(black_box(&zer_team)))
    });
    group.bench_function("team/rkyv", |b| {
        b.iter(|| rk::read_team(black_box(&rk_team)))
    });
    group.bench_function("team/flatbuffers", |b| {
        b.iter(|| flat::read_team(black_box(&flat_team)))
    });

    group.bench_function("series/zerialize", |b| {
        b.iter(|| zer::read_series(black_box(&zer_series)))
    });
    group.bench_function("series/rkyv", |b| {
        b.iter(|| rk::read_series(black_box(&rk_series)))
    });
    group.bench_function("series/flatbuffers", |b| {
        b.iter(|| flat::read_series(black_box(&flat_series)))
    });

    group.finish();
}

/// Decoding without traversing, which is the check every one of them makes up
/// front. What is left of the decode benchmark above is the walk that follows.
fn opening(criterion: &mut Criterion, data: &Data) {
    let mut group = criterion.benchmark_group("open");
    let mut builder = FlatBufferBuilder::new();

    let mut zer_point = Vec::new();
    zer::encode_point(&zer::point(data), &mut zer_point);
    let mut zer_team = Vec::new();
    zer::encode_team(&zer::team(data), &mut zer_team);
    let mut zer_series = Vec::new();
    zer::encode_series(&zer::series(data), &mut zer_series);

    let rk_point = rkyv_encoded(&rk::point(data));
    let rk_team = rkyv_encoded(&rk::team(data));
    let rk_series = rkyv_encoded(&rk::series(data));

    flat::encode_point(data, &mut builder);
    let flat_point = builder.finished_data().to_vec();
    flat::encode_team(data, &mut builder);
    let flat_team = builder.finished_data().to_vec();
    flat::encode_series(data, &mut builder);
    let flat_series = builder.finished_data().to_vec();

    group.bench_function("point/zerialize", |b| {
        b.iter(|| black_box(zer::open_point(black_box(&zer_point))))
    });
    group.bench_function("point/rkyv", |b| {
        b.iter(|| black_box(rk::open_point(black_box(&rk_point))))
    });
    group.bench_function("point/flatbuffers", |b| {
        b.iter(|| black_box(flat::open_point(black_box(&flat_point))))
    });

    group.bench_function("team/zerialize", |b| {
        b.iter(|| black_box(zer::open_team(black_box(&zer_team))))
    });
    group.bench_function("team/rkyv", |b| {
        b.iter(|| black_box(rk::open_team(black_box(&rk_team))))
    });
    group.bench_function("team/flatbuffers", |b| {
        b.iter(|| black_box(flat::open_team(black_box(&flat_team))))
    });

    group.bench_function("series/zerialize", |b| {
        b.iter(|| black_box(zer::open_series(black_box(&zer_series))))
    });
    group.bench_function("series/rkyv", |b| {
        b.iter(|| black_box(rk::open_series(black_box(&rk_series))))
    });
    group.bench_function("series/flatbuffers", |b| {
        b.iter(|| black_box(flat::open_series(black_box(&flat_series))))
    });

    group.finish();
}

fn benchmarks(criterion: &mut Criterion) {
    let data = data();
    encoding(criterion, &data);
    opening(criterion, &data);
    decoding(criterion, &data);
}

criterion_group!(speed, benchmarks);
criterion_main!(speed);
