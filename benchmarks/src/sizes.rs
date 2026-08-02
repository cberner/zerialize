//! Prints how many bytes each encoding takes for each of the benchmark
//! records.

use flatbuffers::FlatBufferBuilder;
use zerialize_benchmarks::{data, flat, rk, zer};

fn row(name: &str, zerialize: usize, rkyv: usize, flatbuffers: usize) {
    let best = zerialize.min(rkyv).min(flatbuffers) as f64;
    println!(
        "| {name:<22} | {zerialize:>9} | {rkyv:>9} | {flatbuffers:>11} | {:>5.2}x | {:>5.2}x | {:>5.2}x |",
        zerialize as f64 / best,
        rkyv as f64 / best,
        flatbuffers as f64 / best,
    );
}

fn main() {
    let data = data();
    let mut builder = FlatBufferBuilder::new();
    let mut out = Vec::new();

    println!(
        "| {:<22} | {:>9} | {:>9} | {:>11} | {:>6} | {:>6} | {:>6} |",
        "record", "zerialize", "rkyv", "flatbuffers", "zer", "rkyv", "fb"
    );
    println!(
        "| {:-<22} | {:->9} | {:->9} | {:->11} | {:->6} | {:->6} | {:->6} |",
        "", "", "", "", "", "", ""
    );

    zer::encode_point(&zer::point(&data), &mut out);
    row(
        "point (3 x f32)",
        out.len(),
        rk::encode(&rk::point(&data)).len(),
        flat::encode_point(&data, &mut builder),
    );

    zer::encode_team(&zer::team(&data), &mut out);
    row(
        "team (32 people)",
        out.len(),
        rk::encode(&rk::team(&data)).len(),
        flat::encode_team(&data, &mut builder),
    );

    zer::encode_series(&zer::series(&data), &mut out);
    row(
        "series (1024 x f64)",
        out.len(),
        rk::encode(&rk::series(&data)).len(),
        flat::encode_series(&data, &mut builder),
    );
}
