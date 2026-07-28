use zerialize::{Error, List, OwnedList, decode, encode, zerializable};

mod location {
    use zerialize::zerializable;

    #[zerializable]
    pub trait Address {
        #[slot(0)]
        fn city(&self) -> &str;

        #[slot(1)]
        fn zip(&self) -> u32;
    }

    #[derive(Debug)]
    pub struct OwnedAddress {
        pub city: String,
        pub zip: u32,
    }

    impl Address for OwnedAddress {
        fn city(&self) -> &str {
            &self.city
        }

        fn zip(&self) -> u32 {
            self.zip
        }
    }
}

// A nested schema is named by its trait, so nothing generated has to be in
// scope for `Person` to refer to `Address`.
use location::{Address, OwnedAddress};

#[zerializable]
trait Person {
    #[slot(0)]
    fn name(&self) -> &str;

    #[slot(1)]
    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized;

    #[slot(2)]
    fn address(&self) -> impl Address + '_
    where
        Self: Sized;
}

#[derive(Debug)]
struct OwnedPerson {
    name: String,
    children: OwnedList<OwnedPerson>,
    address: OwnedAddress,
}

impl OwnedPerson {
    fn new(name: &str, children: Vec<OwnedPerson>) -> Self {
        Self {
            name: name.to_string(),
            children: children.into(),
            address: OwnedAddress {
                city: "Berkeley".to_string(),
                zip: 94704,
            },
        }
    }
}

impl Person for OwnedPerson {
    fn name(&self) -> &str {
        &self.name
    }

    fn children(&self) -> impl List<Item = impl Person + '_> + '_
    where
        Self: Sized,
    {
        &self.children
    }

    fn address(&self) -> impl Address + '_
    where
        Self: Sized,
    {
        &self.address
    }
}

fn family() -> OwnedPerson {
    OwnedPerson::new(
        "John",
        vec![
            OwnedPerson::new("Jimmy", vec![OwnedPerson::new("Jenny", vec![])]),
            OwnedPerson::new("Jane", vec![]),
        ],
    )
}

#[test]
fn round_trip() {
    let person = family();
    let encoded = encode::<dyn Person>(&person);
    let view = decode::<dyn Person>(&encoded).unwrap();

    assert_eq!(view.name(), "John");
    assert_eq!(view.address().city(), "Berkeley");
    assert_eq!(view.address().zip(), 94704);

    let children = view.children();
    assert_eq!(children.len(), 2);
    assert_eq!(children.get(0).unwrap().name(), "Jimmy");
    let grandchildren = children.get(0).unwrap().children();
    assert_eq!(grandchildren.get(0).unwrap().name(), "Jenny");
    assert_eq!(children.get(1).unwrap().name(), "Jane");
    assert!(children.get(1).unwrap().children().is_empty());
    assert!(children.get(2).is_none());
}

#[test]
fn view_borrows_from_the_buffer() {
    let encoded = encode::<dyn Person>(&family());
    let view = decode::<dyn Person>(&encoded).unwrap();
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();

    // Names are pointers into `encoded` rather than copies of it, at every
    // level of the tree: decoding a list does not materialize its elements.
    assert!(buffer.contains(&(view.name().as_ptr() as usize)));
    let child = view.children().get(0).unwrap();
    let grandchild = child.children().get(0).unwrap();
    assert!(buffer.contains(&(grandchild.name().as_ptr() as usize)));
}

#[test]
fn list_elements_are_decoded_on_access() {
    let encoded = encode::<dyn Person>(&family());
    let view = decode::<dyn Person>(&encoded).unwrap();

    let children = view.children();
    assert_eq!(children.iter().count(), 2);
    assert_eq!(
        children
            .iter()
            .map(|child| child.name())
            .collect::<Vec<_>>(),
        ["Jimmy", "Jane"]
    );
}

#[test]
fn view_compares_equal_to_the_source() {
    let person = family();
    let encoded = encode::<dyn Person>(&person);
    let view = decode::<dyn Person>(&encoded).unwrap();

    assert_eq!(view, person);
    // The source's children are opaque `impl Person`, so they are only
    // comparable, not printable, which is what assert_eq! would need.
    let children = person.children();
    assert!(view.children().get(0).unwrap() == children.get(0).unwrap());
    assert_ne!(view, OwnedPerson::new("Jack", vec![]));
    // Views are Debug, which is what makes the assertions above printable.
    assert!(format!("{view:?}").contains("Jenny"));
}

#[test]
fn view_re_encodes_identically() {
    let encoded = encode::<dyn Person>(&family());
    let view = decode::<dyn Person>(&encoded).unwrap();

    // A view implements the schema trait, so it is itself an encodable source.
    assert_eq!(encode::<dyn Person>(&view), encoded);
}

#[zerializable]
trait Primitives {
    #[slot(0)]
    fn flag(&self) -> bool;

    #[slot(1)]
    fn small(&self) -> u8;

    #[slot(2)]
    fn signed(&self) -> i64;

    #[slot(3)]
    fn ratio(&self) -> f64;

    #[slot(4)]
    fn blob(&self) -> &[u8];
}

#[derive(Debug)]
struct OwnedPrimitives;

impl Primitives for OwnedPrimitives {
    fn flag(&self) -> bool {
        true
    }

    fn small(&self) -> u8 {
        u8::MAX
    }

    fn signed(&self) -> i64 {
        i64::MIN
    }

    fn ratio(&self) -> f64 {
        -0.5
    }

    fn blob(&self) -> &[u8] {
        &[0, 1, 2, 255]
    }
}

#[test]
fn primitives_round_trip() {
    let encoded = encode::<dyn Primitives>(&OwnedPrimitives);
    let view = decode::<dyn Primitives>(&encoded).unwrap();

    assert!(view.flag());
    assert_eq!(view.small(), u8::MAX);
    assert_eq!(view.signed(), i64::MIN);
    assert_eq!(view.ratio(), -0.5);
    assert_eq!(view.blob(), &[0, 1, 2, 255]);
    assert_eq!(view, OwnedPrimitives);
}

/// Schema evolution: `v2` adds a slot that `v1` readers do not know about.
mod v1 {
    use zerialize::zerializable;

    #[zerializable]
    pub trait Record {
        #[slot(0)]
        fn id(&self) -> u32;
    }
}

mod v2 {
    use zerialize::zerializable;

    #[zerializable]
    pub trait Record {
        #[slot(0)]
        fn id(&self) -> u32;

        #[slot(7)]
        fn label(&self) -> &str;
    }

    pub struct OwnedRecord;

    impl Record for OwnedRecord {
        fn id(&self) -> u32 {
            7
        }

        fn label(&self) -> &str {
            "added later"
        }
    }
}

#[test]
fn unknown_slots_are_skipped() {
    let encoded = encode::<dyn v2::Record>(&v2::OwnedRecord);
    let view = decode::<dyn v1::Record>(&encoded).unwrap();

    assert_eq!(view.id(), 7);
}

#[test]
fn missing_slots_are_rejected() {
    struct OldRecord;
    impl v1::Record for OldRecord {
        fn id(&self) -> u32 {
            7
        }
    }

    let encoded = encode::<dyn v1::Record>(&OldRecord);
    assert_eq!(
        decode::<dyn v2::Record>(&encoded).unwrap_err(),
        Error::MissingField
    );
}

#[test]
fn truncated_input_is_rejected() {
    let encoded = encode::<dyn Person>(&family());
    for length in 0..encoded.len() {
        assert!(decode::<dyn Person>(&encoded[..length]).is_err());
    }
}

#[test]
fn trailing_bytes_are_rejected() {
    let mut encoded = encode::<dyn Person>(&family());
    encoded.push(0);
    assert_eq!(
        decode::<dyn Person>(&encoded).unwrap_err(),
        Error::TrailingBytes
    );
}

#[test]
fn invalid_utf8_is_rejected() {
    let mut encoded = encode::<dyn Person>(&OwnedPerson::new("John", vec![]));
    let start = encoded
        .windows(4)
        .position(|window| window == b"John")
        .expect("the name is stored verbatim");
    encoded[start] = 0xff;

    assert_eq!(
        decode::<dyn Person>(&encoded).unwrap_err(),
        Error::InvalidUtf8
    );
}

#[test]
fn deep_nesting_is_rejected() {
    fn chain(depth: usize) -> OwnedPerson {
        let mut person = OwnedPerson::new("leaf", vec![]);
        for _ in 0..depth {
            person = OwnedPerson::new("node", vec![person]);
        }
        person
    }

    let shallow = encode::<dyn Person>(&chain(32));
    assert!(decode::<dyn Person>(&shallow).is_ok());

    let deep = encode::<dyn Person>(&chain(200));
    assert_eq!(
        decode::<dyn Person>(&deep).unwrap_err(),
        Error::RecursionLimit
    );
}

#[test]
fn corrupt_input_never_panics() {
    fn walk<P: Person>(person: &P) {
        let _ = person.name();
        let _ = person.address().city();
        let children = person.children();
        for child in children.iter() {
            walk(&child);
        }
    }

    // Decoding validates the whole message, which is what lets every accessor
    // on a view, including lazily decoded list elements, be infallible.
    let encoded = encode::<dyn Person>(&family());
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            if let Ok(view) = decode::<dyn Person>(&corrupted) {
                walk(&view);
            }
        }
    }
}

#[test]
fn view_is_a_thin_handle() {
    fn assert_copy<T: Copy>(_: &T) {}

    let encoded = encode::<dyn Person>(&family());
    let view = decode::<dyn Person>(&encoded).unwrap();

    // Copy is the structural half of the claim: a view that owned any of its
    // contents, a String or a Vec of decoded children, could not be Copy.
    assert_copy(&view);

    // A view is the bytes of its message and nothing else. Fields are read out
    // of them on access, so a view is one slice wide whatever its schema holds:
    // a nested message and a list cost exactly what the message itself does.
    let handle = size_of::<&[u8]>();
    assert_eq!(size_of_val(&view), handle);
    assert_eq!(size_of_val(&view.address()), handle);
    assert_eq!(size_of_val(&view.children()), handle);

    // Nor does the count of fields change it: five slots cost what three do,
    // and one scalar costs the same again.
    let encoded = encode::<dyn Primitives>(&OwnedPrimitives);
    assert_eq!(
        size_of_val(&decode::<dyn Primitives>(&encoded).unwrap()),
        handle
    );
    let encoded = encode::<dyn v2::Record>(&v2::OwnedRecord);
    assert_eq!(
        size_of_val(&decode::<dyn v1::Record>(&encoded).unwrap()),
        handle
    );
}

#[test]
fn view_size_does_not_depend_on_the_data() {
    fn wide(children: usize) -> OwnedPerson {
        OwnedPerson::new(
            "root",
            (0..children)
                .map(|_| OwnedPerson::new("child", vec![]))
                .collect(),
        )
    }

    let small = encode::<dyn Person>(&wide(1));
    let large = encode::<dyn Person>(&wide(10_000));
    assert!(large.len() > 100 * small.len());

    // Decoding is a handle over the buffer, so a view of a megabyte of people
    // is the same size as a view of one, and so is any child reached from it.
    let small_view = decode::<dyn Person>(&small).unwrap();
    let large_view = decode::<dyn Person>(&large).unwrap();
    assert_eq!(size_of_val(&small_view), size_of_val(&large_view));
    assert_eq!(
        size_of_val(&large_view.children().get(9_999).unwrap()),
        size_of_val(&large_view)
    );
}
