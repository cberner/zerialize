//! The benchmark model as rkyv types.

use rkyv::rancor::Error;
use rkyv::ser::allocator::ArenaHandle;
use rkyv::util::AlignedVec;
use rkyv::{Archive, Serialize, access, to_bytes};

#[derive(Archive, Serialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Archive, Serialize)]
pub enum Status {
    Active,
    Idle,
    Retired,
}

#[derive(Archive, Serialize)]
pub struct Person {
    pub name: String,
    pub id: u64,
    pub email: String,
    pub status: Status,
    pub scores: Vec<u32>,
}

#[derive(Archive, Serialize)]
pub struct Team {
    pub name: String,
    pub members: Vec<Person>,
}

#[derive(Archive, Serialize)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

pub fn point(data: &crate::Data) -> Point {
    Point {
        x: data.point.0,
        y: data.point.1,
        z: data.point.2,
    }
}

pub fn team(data: &crate::Data) -> Team {
    Team {
        name: data.team.name.clone(),
        members: data
            .team
            .members
            .iter()
            .map(|person| Person {
                name: person.name.clone(),
                id: person.id,
                email: person.email.clone(),
                status: match person.status {
                    0 => Status::Active,
                    1 => Status::Idle,
                    _ => Status::Retired,
                },
                scores: person.scores.clone(),
            })
            .collect(),
    }
}

pub fn series(data: &crate::Data) -> Series {
    Series {
        name: data.series.name.clone(),
        values: data.series.values.clone(),
    }
}

pub fn encode<T>(source: &T) -> AlignedVec
where
    T: for<'a> Serialize<rkyv::api::high::HighSerializer<AlignedVec, ArenaHandle<'a>, Error>>,
{
    to_bytes::<Error>(source).unwrap()
}

pub fn read_point(bytes: &[u8]) -> f32 {
    let point = access::<ArchivedPoint, Error>(bytes).unwrap();
    point.x.to_native() + point.y.to_native() + point.z.to_native()
}

pub fn read_team(bytes: &[u8]) -> u64 {
    let team = access::<ArchivedTeam, Error>(bytes).unwrap();
    let mut total = team.name.len() as u64;
    for member in team.members.iter() {
        total += member.id.to_native();
        total += member.name.len() as u64;
        total += member.email.len() as u64;
        total += match member.status {
            ArchivedStatus::Active => 0,
            ArchivedStatus::Idle => 1,
            ArchivedStatus::Retired => 2,
        };
        for score in member.scores.iter() {
            total += u64::from(score.to_native());
        }
    }
    total
}

pub fn read_series(bytes: &[u8]) -> f64 {
    let series = access::<ArchivedSeries, Error>(bytes).unwrap();
    let mut total = series.name.len() as f64;
    for value in series.values.iter() {
        total += value.to_native();
    }
    total
}

pub fn open_point(bytes: &[u8]) -> &ArchivedPoint {
    access::<ArchivedPoint, Error>(bytes).unwrap()
}

pub fn open_team(bytes: &[u8]) -> &ArchivedTeam {
    access::<ArchivedTeam, Error>(bytes).unwrap()
}

pub fn open_series(bytes: &[u8]) -> &ArchivedSeries {
    access::<ArchivedSeries, Error>(bytes).unwrap()
}
