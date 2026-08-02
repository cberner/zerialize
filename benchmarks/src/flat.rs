//! The benchmark model as flatbuffers, built through the generated builders.

use crate::bench_generated::bench;
use flatbuffers::FlatBufferBuilder;

pub fn encode_point(data: &crate::Data, builder: &mut FlatBufferBuilder<'_>) -> usize {
    builder.reset();
    let point = bench::PointTable::create(
        builder,
        &bench::PointTableArgs {
            x: data.point.0,
            y: data.point.1,
            z: data.point.2,
        },
    );
    builder.finish(point, None);
    builder.finished_data().len()
}

pub fn encode_team(data: &crate::Data, builder: &mut FlatBufferBuilder<'_>) -> usize {
    builder.reset();
    let members = data
        .team
        .members
        .iter()
        .map(|person| {
            let name = builder.create_string(&person.name);
            let email = builder.create_string(&person.email);
            let scores = builder.create_vector(&person.scores);
            bench::Person::create(
                builder,
                &bench::PersonArgs {
                    name: Some(name),
                    id: person.id,
                    email: Some(email),
                    status: bench::Status(person.status as i8),
                    scores: Some(scores),
                },
            )
        })
        .collect::<Vec<_>>();
    let members = builder.create_vector(&members);
    let name = builder.create_string(&data.team.name);
    let team = bench::Team::create(
        builder,
        &bench::TeamArgs {
            name: Some(name),
            members: Some(members),
        },
    );
    builder.finish(team, None);
    builder.finished_data().len()
}

pub fn encode_series(data: &crate::Data, builder: &mut FlatBufferBuilder<'_>) -> usize {
    builder.reset();
    let values = builder.create_vector(&data.series.values);
    let name = builder.create_string(&data.series.name);
    let series = bench::Series::create(
        builder,
        &bench::SeriesArgs {
            name: Some(name),
            values: Some(values),
        },
    );
    builder.finish(series, None);
    builder.finished_data().len()
}

pub fn read_point(bytes: &[u8]) -> f32 {
    let point = flatbuffers::root::<bench::PointTable>(bytes).unwrap();
    point.x() + point.y() + point.z()
}

pub fn read_team(bytes: &[u8]) -> u64 {
    let team = flatbuffers::root::<bench::Team>(bytes).unwrap();
    let mut total = team.name().unwrap().len() as u64;
    for member in team.members().unwrap().iter() {
        total += member.id();
        total += member.name().unwrap().len() as u64;
        total += member.email().unwrap().len() as u64;
        total += member.status().0 as u64;
        for score in member.scores().unwrap().iter() {
            total += u64::from(score);
        }
    }
    total
}

pub fn read_series(bytes: &[u8]) -> f64 {
    let series = flatbuffers::root::<bench::Series>(bytes).unwrap();
    let mut total = series.name().unwrap().len() as f64;
    for value in series.values().unwrap().iter() {
        total += value;
    }
    total
}

pub fn open_point(bytes: &[u8]) -> bench::PointTable<'_> {
    flatbuffers::root::<bench::PointTable>(bytes).unwrap()
}

pub fn open_team(bytes: &[u8]) -> bench::Team<'_> {
    flatbuffers::root::<bench::Team>(bytes).unwrap()
}

pub fn open_series(bytes: &[u8]) -> bench::Series<'_> {
    flatbuffers::root::<bench::Series>(bytes).unwrap()
}
