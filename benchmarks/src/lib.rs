//! One data model in three encodings, so that the same records can be measured
//! through zerialize, rkyv and flatbuffers.
//!
//! The shapes are chosen to separate the costs that differ between the three:
//! a three field record isolates per message overhead, a team of people mixes
//! strings, numbers, an enum and nested records, and a series of samples is a
//! list of numbers and nothing else.

#![allow(clippy::all)]

#[allow(warnings)]
mod bench_generated;

pub mod flat;
pub mod rk;
pub mod zer;

/// Records the benchmarks encode, built once and shared by all three
/// encodings so that every one of them sees the same data.
pub struct Data {
    pub point: (f32, f32, f32),
    pub team: Team,
    pub series: Series,
}

pub struct Team {
    pub name: String,
    pub members: Vec<Person>,
}

pub struct Person {
    pub name: String,
    pub id: u64,
    pub email: String,
    pub status: u8,
    pub scores: Vec<u32>,
}

pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

/// A deterministic generator, so that every run measures the same bytes.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

pub fn data() -> Data {
    let mut random = Random(0x5eed_1234_9abc_def0);
    let members = (0..32)
        .map(|index| {
            let scores = (0..8).map(|_| (random.next() % 1000) as u32).collect();
            Person {
                name: format!("Person Number {index}"),
                id: random.next(),
                email: format!("person{index}@example.com"),
                status: (index % 3) as u8,
                scores,
            }
        })
        .collect();
    Data {
        point: (1.5, -2.25, 3.125),
        team: Team {
            name: "Engineering".to_string(),
            members,
        },
        series: Series {
            name: "cpu.load".to_string(),
            values: (0..1024)
                .map(|index| f64::from(index) * 0.125_f64)
                .collect(),
        },
    }
}
