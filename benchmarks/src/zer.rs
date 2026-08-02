//! The benchmark model as zerialize schemas, and owned implementations of them
//! to encode from.

use zerialize::{Copied, List, OwnedList, Zerializable, decode, encode_in, zerializable};

#[zerializable]
pub trait Point {
    #[n(0)]
    fn x(&self) -> f32;

    #[n(1)]
    fn y(&self) -> f32;

    #[n(2)]
    fn z(&self) -> f32;
}

#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
pub enum Status {
    #[variant(0)]
    Active,
    #[variant(1)]
    Idle,
    #[variant(2)]
    Retired,
}

#[zerializable]
pub trait Person {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn id(&self) -> u64;

    #[n(2)]
    fn email(&self) -> &str;

    #[n(3)]
    fn status(&self) -> Status;

    #[n(4)]
    fn scores(&self) -> impl List<Item = u32> + '_
    where
        Self: Sized;
}

#[zerializable]
pub trait Team {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn members(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;
}

#[zerializable]
pub trait Series {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn values(&self) -> impl List<Item = f64> + '_
    where
        Self: Sized;
}

pub struct OwnedPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point for OwnedPoint {
    fn x(&self) -> f32 {
        self.x
    }

    fn y(&self) -> f32 {
        self.y
    }

    fn z(&self) -> f32 {
        self.z
    }
}

pub struct OwnedPerson {
    pub name: String,
    pub id: u64,
    pub email: String,
    pub status: Status,
    pub scores: OwnedList<u32>,
}

impl Person for OwnedPerson {
    fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> u64 {
        self.id
    }

    fn email(&self) -> &str {
        &self.email
    }

    fn status(&self) -> Status {
        self.status
    }

    fn scores(&self) -> impl List<Item = u32> + '_ {
        Copied(&self.scores)
    }
}

pub struct OwnedTeam {
    pub name: String,
    pub members: OwnedList<OwnedPerson>,
}

impl Team for OwnedTeam {
    fn name(&self) -> &str {
        &self.name
    }

    fn members(&self) -> impl List<Item = impl Person + '_> + '_ {
        &self.members
    }
}

pub struct OwnedSeries {
    pub name: String,
    pub values: OwnedList<f64>,
}

impl Series for OwnedSeries {
    fn name(&self) -> &str {
        &self.name
    }

    fn values(&self) -> impl List<Item = f64> + '_ {
        Copied(&self.values)
    }
}

pub fn point(data: &crate::Data) -> OwnedPoint {
    OwnedPoint {
        x: data.point.0,
        y: data.point.1,
        z: data.point.2,
    }
}

pub fn team(data: &crate::Data) -> OwnedTeam {
    OwnedTeam {
        name: data.team.name.clone(),
        members: data
            .team
            .members
            .iter()
            .map(|person| OwnedPerson {
                name: person.name.clone(),
                id: person.id,
                email: person.email.clone(),
                status: match person.status {
                    0 => Status::Active,
                    1 => Status::Idle,
                    _ => Status::Retired,
                },
                scores: person.scores.clone().into(),
            })
            .collect(),
    }
}

pub fn series(data: &crate::Data) -> OwnedSeries {
    OwnedSeries {
        name: data.series.name.clone(),
        values: data.series.values.clone().into(),
    }
}

pub fn encode_point(source: &OwnedPoint, out: &mut Vec<u8>) {
    out.clear();
    encode_in::<dyn Point>(source, out);
}

pub fn encode_team(source: &OwnedTeam, out: &mut Vec<u8>) {
    out.clear();
    encode_in::<dyn Team>(source, out);
}

pub fn encode_series(source: &OwnedSeries, out: &mut Vec<u8>) {
    out.clear();
    encode_in::<dyn Series>(source, out);
}

/// Reads every field of a point, which is what a decode is measured by: the
/// work a format defers to access is work it has not saved.
pub fn read_point(bytes: &[u8]) -> f32 {
    let point = decode::<dyn Point>(bytes).unwrap();
    point.x() + point.y() + point.z()
}

pub fn read_team(bytes: &[u8]) -> u64 {
    let team = decode::<dyn Team>(bytes).unwrap();
    let mut total = team.name().len() as u64;
    for member in team.members().iter() {
        total += member.id();
        total += member.name().len() as u64;
        total += member.email().len() as u64;
        total += member.status() as u64;
        for score in member.scores().iter() {
            total += u64::from(score);
        }
    }
    total
}

pub fn read_series(bytes: &[u8]) -> f64 {
    let series = decode::<dyn Series>(bytes).unwrap();
    let mut total = series.name().len() as f64;
    for value in series.values().iter() {
        total += value;
    }
    total
}

// Decoding without traversing, to separate the up front check from the walk
// that follows it: decoding validates the whole message, and reading it then
// walks the same bytes again.

pub fn open_point(bytes: &[u8]) -> PointView<'_> {
    decode::<dyn Point>(bytes).unwrap()
}

pub fn open_team(bytes: &[u8]) -> TeamView<'_> {
    decode::<dyn Team>(bytes).unwrap()
}

pub fn open_series(bytes: &[u8]) -> SeriesView<'_> {
    decode::<dyn Series>(bytes).unwrap()
}
