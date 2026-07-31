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

// Values are `Copy` types a schema holds by value. They are named outright,
// rather than as `impl Trait`, and nothing generated for them has to be in
// scope where the schema that holds them is.
mod cards {
    use zerialize::Zerializable;

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub enum Suit {
        #[variant(0)]
        Clubs,
        #[variant(1)]
        Diamonds,
        #[variant(2)]
        Hearts,
        #[variant(9)]
        Spades,
    }

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub struct Card {
        #[slot(0)]
        pub rank: u8,
        #[slot(1)]
        pub suit: Suit,
    }

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub struct Play {
        // A slot is the identity of a field, so a value may declare them out of
        // order and leave gaps in them, exactly as a schema's methods may.
        #[slot(3)]
        pub revealed: bool,
        #[slot(0)]
        pub card: Card,
        #[slot(1)]
        pub odds: f32,
    }
}

use cards::{Card, Play, Suit};

#[zerializable]
trait Hand {
    #[slot(0)]
    fn player(&self) -> &str;

    #[slot(1)]
    fn play(&self) -> Play;

    #[slot(2)]
    fn trump(&self) -> cards::Suit;
}

#[derive(Debug)]
struct OwnedHand {
    player: String,
    play: Play,
}

impl Hand for OwnedHand {
    fn player(&self) -> &str {
        &self.player
    }

    fn play(&self) -> Play {
        self.play
    }

    fn trump(&self) -> Suit {
        self.play.card.suit
    }
}

fn hand() -> OwnedHand {
    OwnedHand {
        player: "Ada".to_string(),
        play: Play {
            revealed: true,
            card: Card {
                rank: 12,
                suit: Suit::Spades,
            },
            odds: 0.25,
        },
    }
}

#[test]
fn values_round_trip() {
    let hand = hand();
    let encoded = encode::<dyn Hand>(&hand);
    let view = decode::<dyn Hand>(&encoded).unwrap();

    // A value decodes back into itself, so it is the same type on both sides of
    // the wire rather than a view of the buffer.
    assert_eq!(view.play(), hand.play);
    assert_eq!(view.play().card.suit, Suit::Spades);
    assert_eq!(view.trump(), Suit::Spades);
    assert_eq!(view, hand);
    assert!(format!("{view:?}").contains("Spades"));
    assert_eq!(encode::<dyn Hand>(&view), encoded);
}

#[test]
fn a_value_outlives_the_buffer_it_was_read_from() {
    // Nothing a value holds borrows, which is what lets it escape the buffer
    // the view that produced it is a handle over.
    let play = {
        let encoded = encode::<dyn Hand>(&hand());
        decode::<dyn Hand>(&encoded).unwrap().play()
    };

    assert_eq!(play, hand().play);
}

#[test]
fn a_value_does_not_widen_the_view_that_holds_it() {
    let encoded = encode::<dyn Hand>(&hand());
    let view = decode::<dyn Hand>(&encoded).unwrap();

    // Values are read out of the buffer on access like every other field, so a
    // view of them is still one slice wide.
    assert_eq!(size_of_val(&view), size_of::<&[u8]>());
}

#[zerializable]
trait Trumps {
    #[slot(0)]
    fn value(&self) -> Suit;
}

#[zerializable]
trait Numbered {
    #[slot(0)]
    fn value(&self) -> u32;
}

#[test]
fn a_value_enum_costs_what_its_number_costs() {
    struct Trumped;
    impl Trumps for Trumped {
        fn value(&self) -> Suit {
            Suit::Hearts
        }
    }

    struct Counted;
    impl Numbered for Counted {
        fn value(&self) -> u32 {
            2
        }
    }

    // A variant number is all a unit variant carries, so an enum is written as
    // that number rather than as a message of its own.
    assert_eq!(
        encode::<dyn Trumps>(&Trumped),
        encode::<dyn Numbered>(&Counted)
    );
}

#[test]
fn corrupt_values_never_panic() {
    let encoded = encode::<dyn Hand>(&hand());
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            if let Ok(view) = decode::<dyn Hand>(&corrupted) {
                let _ = view.play();
                let _ = view.trump();
            }
        }
    }
}

/// Value evolution: `after` adds a field to a value struct, which an older
/// reader skips, and a variant to a value enum, which it cannot.
mod before {
    use zerialize::{Zerializable, zerializable};

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub enum Color {
        #[variant(0)]
        Red,
    }

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub struct Pixel {
        #[slot(0)]
        pub color: Color,
    }

    #[zerializable]
    pub trait Image {
        #[slot(0)]
        fn pixel(&self) -> Pixel;
    }

    pub struct OwnedImage(pub Pixel);

    impl Image for OwnedImage {
        fn pixel(&self) -> Pixel {
            self.0
        }
    }
}

mod after {
    use zerialize::{Zerializable, zerializable};

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub enum Color {
        #[variant(0)]
        Red,
        #[variant(1)]
        Blue,
    }

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub struct Pixel {
        #[slot(0)]
        pub color: Color,
        #[slot(1)]
        pub alpha: u8,
    }

    #[zerializable]
    pub trait Image {
        #[slot(0)]
        fn pixel(&self) -> Pixel;
    }

    pub struct OwnedImage(pub Pixel);

    impl Image for OwnedImage {
        fn pixel(&self) -> Pixel {
            self.0
        }
    }
}

#[test]
fn unknown_value_slots_are_skipped() {
    let pixel = after::Pixel {
        color: after::Color::Red,
        alpha: 128,
    };
    let encoded = encode::<dyn after::Image>(&after::OwnedImage(pixel));

    // `alpha` is a slot the older value does not know, so it is never read.
    let view = decode::<dyn before::Image>(&encoded).unwrap();
    assert_eq!(
        view.pixel(),
        before::Pixel {
            color: before::Color::Red
        }
    );
}

#[test]
fn missing_value_slots_are_rejected() {
    let pixel = before::Pixel {
        color: before::Color::Red,
    };
    let encoded = encode::<dyn before::Image>(&before::OwnedImage(pixel));

    assert_eq!(
        decode::<dyn after::Image>(&encoded).unwrap_err(),
        Error::MissingField
    );
}

#[test]
fn unknown_variants_are_rejected() {
    // Unlike a slot, which a reader skips by never asking for it, a variant it
    // does not know is data it cannot represent.
    let pixel = after::Pixel {
        color: after::Color::Blue,
        alpha: 0,
    };
    let encoded = encode::<dyn after::Image>(&after::OwnedImage(pixel));
    assert_eq!(
        decode::<dyn before::Image>(&encoded).unwrap_err(),
        Error::UnknownVariant
    );

    // The variant it does know still decodes, out of the same slot.
    let pixel = after::Pixel {
        color: after::Color::Red,
        alpha: 0,
    };
    let encoded = encode::<dyn after::Image>(&after::OwnedImage(pixel));
    assert!(decode::<dyn before::Image>(&encoded).is_ok());
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

/// A schema declared as an enum, carrying two schemas of its own.
#[zerializable]
enum Role<P: Person, A: Address> {
    #[variant(0)]
    Resident(#[slot(0)] P),
    #[variant(1)]
    Office(#[slot(0)] A, #[slot(1)] u32),
    #[variant(2)]
    Vacant,
}

fn office() -> Role<OwnedPerson, OwnedAddress> {
    Role::Office(
        OwnedAddress {
            city: "Oakland".to_string(),
            zip: 94607,
        },
        12,
    )
}

#[test]
fn enum_round_trips() {
    let resident: Role<OwnedPerson, OwnedAddress> = Role::Resident(family());
    let encoded = encode::<Role<dyn Person, dyn Address>>(&resident);

    match decode::<Role<dyn Person, dyn Address>>(&encoded).unwrap() {
        Role::Resident(person) => {
            assert_eq!(person.name(), "John");
            assert_eq!(person.children().len(), 2);
        }
        _ => panic!("decoded the wrong variant"),
    }
}

#[test]
fn every_variant_carries_its_own_fields() {
    let encoded = encode::<Role<dyn Person, dyn Address>>(&office());
    match decode::<Role<dyn Person, dyn Address>>(&encoded).unwrap() {
        Role::Office(address, floor) => {
            assert_eq!(address.city(), "Oakland");
            assert_eq!(address.zip(), 94607);
            assert_eq!(floor, 12);
        }
        _ => panic!("decoded the wrong variant"),
    }

    let vacant: Role<OwnedPerson, OwnedAddress> = Role::Vacant;
    let encoded = encode::<Role<dyn Person, dyn Address>>(&vacant);
    assert!(matches!(
        decode::<Role<dyn Person, dyn Address>>(&encoded).unwrap(),
        Role::Vacant
    ));
}

#[test]
fn a_stored_enum_is_handed_out_borrowed() {
    let stored = office();

    // What an implementation stores is handed out by borrowing the messages
    // its variant carries and copying the scalars, so the two encode alike.
    let borrowed: Role<&OwnedPerson, &OwnedAddress> = stored.as_ref();
    assert_eq!(
        encode::<Role<dyn Person, dyn Address>>(&borrowed),
        encode::<Role<dyn Person, dyn Address>>(&stored)
    );

    let vacant: Role<OwnedPerson, OwnedAddress> = Role::Vacant;
    assert!(matches!(vacant.as_ref(), Role::Vacant));
}

#[test]
fn enum_payloads_borrow_from_the_buffer() {
    let resident: Role<OwnedPerson, OwnedAddress> = Role::Resident(family());
    let encoded = encode::<Role<dyn Person, dyn Address>>(&resident);
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();

    // Decoding an enum decides its variant, and nothing else: what the variant
    // carries is still a view of the buffer it was decoded from.
    match decode::<Role<dyn Person, dyn Address>>(&encoded).unwrap() {
        Role::Resident(person) => {
            assert!(buffer.contains(&(person.name().as_ptr() as usize)));
        }
        _ => panic!("decoded the wrong variant"),
    }
}

#[test]
fn decoded_enum_re_encodes_identically() {
    let resident: Role<OwnedPerson, OwnedAddress> = Role::Resident(family());
    let encoded = encode::<Role<dyn Person, dyn Address>>(&resident);
    let decoded = decode::<Role<dyn Person, dyn Address>>(&encoded).unwrap();

    // The decoded enum is the enum over views, and a view implements the schema
    // it was decoded as, so the whole of it is encodable again.
    assert_eq!(encode::<Role<dyn Person, dyn Address>>(&decoded), encoded);
}

#[test]
fn corrupt_enum_input_never_panics() {
    let resident: Role<OwnedPerson, OwnedAddress> = Role::Resident(family());
    for encoded in [
        encode::<Role<dyn Person, dyn Address>>(&resident),
        encode::<Role<dyn Person, dyn Address>>(&office()),
    ] {
        for index in 0..encoded.len() {
            for bit in [0x01, 0x40, 0x80] {
                let mut corrupted = encoded.clone();
                corrupted[index] ^= bit;
                // Corrupting the tag decodes one variant's fields as another's,
                // which is as validated as any other input: it may be nonsense,
                // but reading it cannot fail.
                match decode::<Role<dyn Person, dyn Address>>(&corrupted) {
                    Ok(Role::Resident(person)) => {
                        let _ = person.name();
                        for child in person.children().iter() {
                            let _ = child.name();
                        }
                    }
                    Ok(Role::Office(address, floor)) => {
                        let _ = address.city();
                        let _ = floor;
                    }
                    Ok(Role::Vacant) | Err(_) => (),
                }
            }
        }
    }
}

/// A message carrying enums: one on its own, and one per element of a list.
#[zerializable]
trait Building {
    #[slot(0)]
    fn owner(&self) -> Role<impl Person + '_, impl Address + '_>
    where
        Self: Sized;

    #[slot(1)]
    fn tenants(&self) -> impl List<Item = Role<impl Person + '_, impl Address + '_>> + '_
    where
        Self: Sized;
}

#[derive(Debug)]
struct OwnedBuilding {
    owner: OwnedPerson,
    tenants: OwnedList<OwnedAddress>,
}

impl Building for OwnedBuilding {
    fn owner(&self) -> Role<impl Person + '_, impl Address + '_>
    where
        Self: Sized,
    {
        // Borrowed out of self, exactly as a nested message is.
        Role::<&OwnedPerson, &OwnedAddress>::Resident(&self.owner)
    }

    fn tenants(&self) -> impl List<Item = Role<impl Person + '_, impl Address + '_>> + '_
    where
        Self: Sized,
    {
        Offices(&self.tenants)
    }
}

/// A list whose elements are built as they are asked for, which is what lets an
/// implementation hand out enums it does not store.
struct Offices<'a>(&'a OwnedList<OwnedAddress>);

impl<'a> List for Offices<'a> {
    type Item = Role<&'a OwnedPerson, &'a OwnedAddress>;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<Self::Item> {
        Some(Role::Office(self.0.as_slice().get(index)?, index as u32))
    }
}

fn building() -> OwnedBuilding {
    OwnedBuilding {
        owner: family(),
        tenants: vec![
            OwnedAddress {
                city: "Oakland".to_string(),
                zip: 94607,
            },
            OwnedAddress {
                city: "Alameda".to_string(),
                zip: 94501,
            },
        ]
        .into(),
    }
}

#[test]
fn a_message_carries_an_enum() {
    let encoded = encode::<dyn Building>(&building());
    let view = decode::<dyn Building>(&encoded).unwrap();

    match view.owner() {
        Role::Resident(person) => assert_eq!(person.name(), "John"),
        _ => panic!("decoded the wrong variant"),
    }

    let tenants = view.tenants();
    assert_eq!(tenants.len(), 2);
    match tenants.get(1).unwrap() {
        Role::Office(address, floor) => {
            assert_eq!(address.city(), "Alameda");
            assert_eq!(floor, 1);
        }
        _ => panic!("decoded the wrong variant"),
    }
}

#[test]
fn corrupt_input_carrying_an_enum_never_panics() {
    // Reading a carried enum through a view is infallible, which holds only
    // because decoding the message decided every enum it holds, including the
    // ones its lists hold.
    let encoded = encode::<dyn Building>(&building());
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            let Ok(view) = decode::<dyn Building>(&corrupted) else {
                continue;
            };
            let _ = view.owner();
            for tenant in view.tenants().iter() {
                let _ = tenant;
            }
        }
    }
}

#[test]
fn a_carried_enum_is_part_of_its_message() {
    let building = building();
    let encoded = encode::<dyn Building>(&building);
    let view = decode::<dyn Building>(&encoded).unwrap();

    // A view compares against the source and prints, both of which reach
    // through the enum it carries.
    assert_eq!(view, building);
    assert!(format!("{view:?}").contains("Resident"));

    // And it re-encodes as itself, so a carried enum round trips both ways.
    assert_eq!(encode::<dyn Building>(&view), encoded);

    // The enum is decided when it is read, but what it carries is still a view
    // of the buffer.
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();
    match view.owner() {
        Role::Resident(person) => {
            assert!(buffer.contains(&(person.name().as_ptr() as usize)));
        }
        _ => panic!("decoded the wrong variant"),
    }
}

/// Choice evolution: `command_v2` adds a field to a variant, and a variant of
/// its own. A choice carrying no schemas is its own name and its own view.
mod command_v1 {
    use zerialize::zerializable;

    #[zerializable]
    pub enum Command {
        #[variant(0)]
        Wait(#[slot(0)] u32),
    }
}

mod command_v2 {
    use zerialize::zerializable;

    #[zerializable]
    pub enum Command {
        #[variant(0)]
        Wait(#[slot(0)] u32, #[slot(1)] bool),
        #[variant(1)]
        Stop,
    }
}

#[test]
fn unknown_fields_of_a_variant_are_skipped() {
    let encoded = encode::<command_v2::Command>(&command_v2::Command::Wait(3, true));
    assert_eq!(
        decode::<command_v1::Command>(&encoded).unwrap(),
        command_v1::Command::Wait(3)
    );
}

#[test]
fn missing_fields_of_a_variant_are_rejected() {
    let encoded = encode::<command_v1::Command>(&command_v1::Command::Wait(3));
    assert_eq!(
        decode::<command_v2::Command>(&encoded).unwrap_err(),
        Error::MissingField
    );
}

#[test]
fn an_unknown_variant_of_a_choice_is_rejected() {
    // A reader has nothing to decode a variant it does not know as, which is
    // what separates adding a variant from adding a field.
    let encoded = encode::<command_v2::Command>(&command_v2::Command::Stop);
    assert_eq!(
        decode::<command_v1::Command>(&encoded).unwrap_err(),
        Error::UnknownVariant
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
