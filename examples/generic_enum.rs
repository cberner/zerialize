use zerialize::*;

#[zerializable]
trait Person {
    #[n(0)]
    fn name(&self) -> &str;

    #[n(1)]
    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;
}

#[zerializable]
enum Worker<P: Person> {
    #[variant(0)]
    Engineer {
        #[n(0)]
        person: P,
    },
    #[variant(1)]
    AI,
}

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

fn assert_eq_worker<T1: Person, T2: Person>(w1: &Worker<T1>, w2: &Worker<T2>) {
    match (w1, w2) {
        (Worker::Engineer { person: p1 }, Worker::Engineer { person: p2 }) => {
            assert_eq_person(p1, p2)
        }
        (Worker::AI, Worker::AI) => (), // no-op,
        _ => panic!(),
    }
}

fn main() {
    let jimmy = SimplePerson {
        name: "Jimmy".to_string(),
        children: vec![].into(),
    };
    let john = SimplePerson {
        name: "John".to_string(),
        children: vec![jimmy].into(),
    };

    // The type is spelled out because `Worker`'s payload is written in terms of
    // its parameter, which leaves nothing for inference to read it off of.
    let worker: Worker<SimplePerson> = Worker::Engineer { person: john };

    let encoded = encode::<Worker<dyn Person>>(&worker);
    let round_trip = decode::<Worker<dyn Person>>(&encoded).unwrap();
    assert_eq_worker(&worker, &round_trip);

    // Decoding gives back an ordinary enum, over views of the buffer.
    match round_trip {
        Worker::Engineer { person } => assert_eq!(person.name(), "John"),
        _ => panic!(),
    }

    // A variant carrying nothing round trips as itself.
    let ai: Worker<SimplePerson> = Worker::AI;
    let encoded = encode::<Worker<dyn Person>>(&ai);
    assert_eq_worker(&ai, &decode::<Worker<dyn Person>>(&encoded).unwrap());
}
