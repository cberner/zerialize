use crate::Month::{February, March, May};
use zerialize::*;

#[zerializable]
trait Person {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;

    #[n(2)]
    fn date_of_birth(&self) -> DateOfBirth;
}

#[derive(Zerializable, Copy, Clone, PartialEq, Eq, Debug)]
enum Month {
    #[variant(0)]
    January,
    #[variant(1)]
    February,
    #[variant(2)]
    March,
    #[variant(3)]
    April,
    #[variant(4)]
    May,
    #[variant(5)]
    June,
    #[variant(6)]
    July,
    #[variant(7)]
    August,
    #[variant(8)]
    September,
    #[variant(9)]
    October,
    #[variant(10)]
    November,
    #[variant(11)]
    December,
}

#[derive(Zerializable, Copy, Clone, PartialEq, Eq, Debug)]
struct DateOfBirth {
    #[n(0)]
    day: u8,
    #[n(1)]
    month: Month,
    #[n(2)]
    year: u16,
}

struct SimplePerson {
    name: String,
    children: OwnedList<SimplePerson>,
    date_of_birth: DateOfBirth,
}

impl Person for SimplePerson {
    fn name(&self) -> &str {
        &self.name
    }

    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized,
    {
        &self.children
    }

    fn date_of_birth(&self) -> DateOfBirth {
        self.date_of_birth
    }
}

fn assert_eq_person<T1: Person, T2: Person>(p1: &T1, p2: &T2) {
    assert_eq!(p1.name(), p2.name());
    assert_eq!(p1.date_of_birth(), p2.date_of_birth());
    assert_eq!(p1.children().len(), p2.children().len());
    for i in 0..p1.children().len() {
        let child1 = p1.children().get(i).unwrap();
        let child2 = p2.children().get(i).unwrap();
        assert_eq!(child1.name(), child2.name());
        assert_eq!(child1.date_of_birth(), child2.date_of_birth());
        assert!(child1.children().is_empty());
        assert!(child2.children().is_empty());
    }
}

fn main() {
    let jimmy = SimplePerson {
        name: "Jimmy".to_string(),
        children: vec![].into(),
        date_of_birth: DateOfBirth {
            day: 1,
            month: February,
            year: 2020,
        },
    };
    let person = SimplePerson {
        name: "John".to_string(),
        children: vec![jimmy].into(),
        date_of_birth: DateOfBirth {
            day: 10,
            month: May,
            year: 1990,
        },
    };

    let encoded = encode::<dyn Person>(&person);
    let round_trip = decode::<dyn Person>(&encoded).unwrap();
    assert_eq_person(&person, &round_trip);

    // Encoding into a buffer of one's own appends to it, so a single
    // allocation may carry several messages, or be cleared and reused for the
    // next one. Each message is decoded from exactly the range it was written
    // to.
    let alice = SimplePerson {
        name: "Alice".to_string(),
        children: vec![].into(),
        date_of_birth: DateOfBirth {
            day: 3,
            month: March,
            year: 1985,
        },
    };

    let mut buffer = Vec::new();
    let first = encode_in::<dyn Person>(&person, &mut buffer);
    let second = encode_in::<dyn Person>(&alice, &mut buffer);
    assert_eq!(&buffer[first.clone()], encoded.as_slice());

    assert_eq_person(&person, &decode::<dyn Person>(&buffer[first]).unwrap());
    assert_eq_person(&alice, &decode::<dyn Person>(&buffer[second]).unwrap());
}
