use zerialize::*;

#[zerializable]
trait Person {
    #[slot(0)]
    fn name(&self) -> &str;

    #[slot(1)]
    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;
}

#[derive(Eq, PartialEq, Debug)]
struct SimplePerson {
    name: String,
    children: OwnedList<SimplePerson>,
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
}

fn assert_eq_person<T1: Person, T2: Person>(p1: &T1, p2: &T2) {
    assert_eq!(p1.name(), p2.name());
    assert_eq!(p1.children().len(), p2.children().len());
    for i in 0..p1.children().len() {
        let child1 = p1.children().get(i).unwrap();
        let child2 = p2.children().get(i).unwrap();
        assert_eq!(child1.name(), child2.name());
        assert!(child1.children().is_empty());
        assert!(child2.children().is_empty());
    }
}

fn main() {
    let jimmy = SimplePerson {
        name: "Jimmy".to_string(),
        children: vec![].into(),
    };
    let person = SimplePerson {
        name: "John".to_string(),
        children: vec![jimmy].into(),
    };

    let encoded = encode::<dyn Person>(&person);
    let round_trip = decode::<dyn Person>(&encoded).unwrap();
    assert_eq_person(&person, &round_trip);
}
