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

#[zerializable]
enum Worker<P: Person> {
    #[variant(0)]
    Engineer(#[slot(0)] P),
    #[variant(1)]
    ProductManager(#[slot(0)] P),
    #[variant(2)]
    AI,
}

#[zerializable]
trait Team {
    #[slot(0)]
    fn name(&self) -> &str;

    #[slot(1)]
    fn lead(&self) -> Worker<impl Person + '_>
    where
        Self: Sized;
}

struct SimplePerson {
    name: String,
    children: OwnedList<SimplePerson>,
}

struct SimpleTeam {
    name: String,
    lead: Worker<SimplePerson>,
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

impl Team for SimpleTeam {
    fn name(&self) -> &str {
        &self.name
    }

    fn lead(&self) -> Worker<impl Person + '_>
    where
        Self: Sized,
    {
        // A team stores a worker of its own, and hands out the one that borrows
        // from it, so that nothing it carries is copied to encode the team.
        self.lead.as_ref()
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
        (Worker::Engineer(p1), Worker::Engineer(p2))
        | (Worker::ProductManager(p1), Worker::ProductManager(p2)) => assert_eq_person(p1, p2),
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
    let worker: Worker<SimplePerson> = Worker::Engineer(john);

    let encoded = encode::<Worker<dyn Person>>(&worker);
    let round_trip = decode::<Worker<dyn Person>>(&encoded).unwrap();
    assert_eq_worker(&worker, &round_trip);

    // Decoding gives back an ordinary enum, over views of the buffer.
    match round_trip {
        Worker::Engineer(person) => assert_eq!(person.name(), "John"),
        _ => panic!(),
    }

    // A variant carrying nothing round trips as itself.
    let ai: Worker<SimplePerson> = Worker::AI;
    let encoded = encode::<Worker<dyn Person>>(&ai);
    assert_eq_worker(&ai, &decode::<Worker<dyn Person>>(&encoded).unwrap());

    // A message carries the enum as a field of its own, so encoding the team
    // encodes the worker it leads with.
    let team = SimpleTeam {
        name: "Zerialize".to_string(),
        lead: worker,
    };

    let encoded = encode::<dyn Team>(&team);
    let round_trip = decode::<dyn Team>(&encoded).unwrap();
    assert_eq!(round_trip.name(), team.name());
    assert_eq_worker(&team.lead(), &round_trip.lead());
}
