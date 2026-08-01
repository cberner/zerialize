//! One round trip per type the README lists as supported, in each of the three
//! places a type may appear: the fields of a message, the fields of a value,
//! and the fields of a choice's variant.

use zerialize::{Copied, Error, List, OwnedList, Zerializable, decode, encode, zerializable};

// ============================================================
// Schemas the sections below share
// ============================================================

#[zerializable(derive(Debug, PartialEq))]
trait Contact {
    #[n(0)]
    fn name(&self) -> &str;
}

#[derive(Debug)]
struct OwnedContact(String);

impl Contact for OwnedContact {
    fn name(&self) -> &str {
        &self.0
    }
}

/// A choice carried by an optional field, to check that what an `Option` wraps
/// may itself be a schema rather than a scalar.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Reachable<C: Contact> {
    #[variant(0)]
    By(#[n(0)] C),
    #[variant(1)]
    Never,
}

// ============================================================
// Primitives, as the fields of a message
// ============================================================

#[zerializable(derive(Debug, PartialEq))]
trait Primitives {
    #[n(0)]
    fn flag(&self) -> bool;

    #[n(1)]
    fn letter(&self) -> char;

    #[n(2)]
    fn text(&self) -> &str;

    #[n(3)]
    fn blob(&self) -> &[u8];

    #[n(4)]
    fn byte(&self) -> u8;

    #[n(5)]
    fn short(&self) -> u16;

    #[n(6)]
    fn word(&self) -> u32;

    #[n(7)]
    fn long(&self) -> u64;

    #[n(8)]
    fn wide(&self) -> u128;

    #[n(9)]
    fn signed_byte(&self) -> i8;

    #[n(10)]
    fn signed_short(&self) -> i16;

    #[n(11)]
    fn signed_word(&self) -> i32;

    #[n(12)]
    fn signed_long(&self) -> i64;

    #[n(13)]
    fn signed_wide(&self) -> i128;

    #[n(14)]
    fn single(&self) -> f32;

    #[n(15)]
    fn double(&self) -> f64;
}

#[derive(Debug)]
struct OwnedPrimitives {
    flag: bool,
    letter: char,
    text: String,
    blob: Vec<u8>,
    byte: u8,
    short: u16,
    word: u32,
    long: u64,
    wide: u128,
    signed_byte: i8,
    signed_short: i16,
    signed_word: i32,
    signed_long: i64,
    signed_wide: i128,
    single: f32,
    double: f64,
}

impl Primitives for OwnedPrimitives {
    fn flag(&self) -> bool {
        self.flag
    }

    fn letter(&self) -> char {
        self.letter
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn blob(&self) -> &[u8] {
        &self.blob
    }

    fn byte(&self) -> u8 {
        self.byte
    }

    fn short(&self) -> u16 {
        self.short
    }

    fn word(&self) -> u32 {
        self.word
    }

    fn long(&self) -> u64 {
        self.long
    }

    fn wide(&self) -> u128 {
        self.wide
    }

    fn signed_byte(&self) -> i8 {
        self.signed_byte
    }

    fn signed_short(&self) -> i16 {
        self.signed_short
    }

    fn signed_word(&self) -> i32 {
        self.signed_word
    }

    fn signed_long(&self) -> i64 {
        self.signed_long
    }

    fn signed_wide(&self) -> i128 {
        self.signed_wide
    }

    fn single(&self) -> f32 {
        self.single
    }

    fn double(&self) -> f64 {
        self.double
    }
}

/// Every primitive at the bottom of its range, with the empty string and the
/// empty slice for the two that have no bottom.
fn lowest() -> OwnedPrimitives {
    OwnedPrimitives {
        flag: false,
        letter: '\u{0}',
        text: String::new(),
        blob: Vec::new(),
        byte: u8::MIN,
        short: u16::MIN,
        word: u32::MIN,
        long: u64::MIN,
        wide: u128::MIN,
        signed_byte: i8::MIN,
        signed_short: i16::MIN,
        signed_word: i32::MIN,
        signed_long: i64::MIN,
        signed_wide: i128::MIN,
        single: f32::NEG_INFINITY,
        double: f64::MIN,
    }
}

/// Every primitive at the top of its range. The widths are what a round trip
/// has to preserve, so the ends of them are what is worth asserting on.
fn highest() -> OwnedPrimitives {
    OwnedPrimitives {
        flag: true,
        letter: char::MAX,
        // A `char` is a Unicode scalar value rather than a byte, and a `str` is
        // encoded as its bytes, so both are given something outside ASCII.
        text: "a\u{e9}\u{1f600}".to_string(),
        blob: vec![0, 1, 254, 255],
        byte: u8::MAX,
        short: u16::MAX,
        word: u32::MAX,
        long: u64::MAX,
        wide: u128::MAX,
        signed_byte: i8::MAX,
        signed_short: i16::MAX,
        signed_word: i32::MAX,
        signed_long: i64::MAX,
        signed_wide: i128::MAX,
        single: f32::MAX,
        double: f64::INFINITY,
    }
}

#[test]
fn every_primitive_round_trips() {
    for source in [lowest(), highest()] {
        let encoded = encode::<dyn Primitives>(&source);
        let view = decode::<dyn Primitives>(&encoded).unwrap();

        assert_eq!(view.flag(), source.flag);
        assert_eq!(view.letter(), source.letter);
        assert_eq!(view.text(), source.text);
        assert_eq!(view.blob(), source.blob);
        assert_eq!(view.byte(), source.byte);
        assert_eq!(view.short(), source.short);
        assert_eq!(view.word(), source.word);
        assert_eq!(view.long(), source.long);
        assert_eq!(view.wide(), source.wide);
        assert_eq!(view.signed_byte(), source.signed_byte);
        assert_eq!(view.signed_short(), source.signed_short);
        assert_eq!(view.signed_word(), source.signed_word);
        assert_eq!(view.signed_long(), source.signed_long);
        assert_eq!(view.signed_wide(), source.signed_wide);
        assert_eq!(view.single(), source.single);
        assert_eq!(view.double(), source.double);

        // The whole message at once, both ways: a view compares against the
        // source it was encoded from, and re-encodes as the same bytes.
        assert_eq!(view, source);
        assert_eq!(encode::<dyn Primitives>(&view), encoded);
    }
}

#[test]
fn a_str_borrows_from_the_buffer() {
    let source = highest();
    let encoded = encode::<dyn Primitives>(&source);
    let view = decode::<dyn Primitives>(&encoded).unwrap();
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();

    // The two primitives that are not `Copy` are the two that are read as
    // handles over the buffer rather than copied out of it.
    assert!(buffer.contains(&(view.text().as_ptr() as usize)));
    assert!(buffer.contains(&(view.blob().as_ptr() as usize)));
}

#[test]
fn corrupt_primitives_never_panic() {
    let encoded = encode::<dyn Primitives>(&highest());
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            if let Ok(view) = decode::<dyn Primitives>(&corrupted) {
                let _ = view.letter();
                let _ = view.text();
                let _ = view.blob();
                let _ = view.wide();
                let _ = view.signed_wide();
            }
        }
    }
}

// A `char` is written as the Unicode scalar value it is, which these two
// schemas read out of the same slot to check.
#[zerializable(derive(Debug, PartialEq))]
trait Letter {
    #[n(0)]
    fn value(&self) -> char;
}

#[zerializable(derive(Debug, PartialEq))]
trait Number {
    #[n(0)]
    fn value(&self) -> u32;
}

struct Numbered(u32);

impl Number for Numbered {
    fn value(&self) -> u32 {
        self.0
    }
}

#[test]
fn a_char_is_written_as_its_scalar_value() {
    struct Lettered(char);
    impl Letter for Lettered {
        fn value(&self) -> char {
            self.0
        }
    }

    let encoded = encode::<dyn Letter>(&Lettered('A'));
    assert_eq!(encoded, encode::<dyn Number>(&Numbered(0x41)));
    assert_eq!(decode::<dyn Letter>(&encoded).unwrap().value(), 'A');
}

#[test]
fn a_char_that_is_not_a_scalar_value_is_rejected() {
    // The two holes in the encoding: a surrogate, and a value past the last
    // code point. Neither is a `char`, so neither decodes as one.
    for invalid in [0xd800, 0x110000] {
        let encoded = encode::<dyn Number>(&Numbered(invalid));
        assert_eq!(
            decode::<dyn Letter>(&encoded).unwrap_err(),
            Error::InvalidChar
        );
    }

    let encoded = encode::<dyn Number>(&Numbered(0x10ffff));
    assert_eq!(decode::<dyn Letter>(&encoded).unwrap().value(), char::MAX);
}

// ============================================================
// Primitives, as the fields of a value and of a choice
// ============================================================

/// Every primitive a value may hold: a value is `Copy`, so `&str` and `&[u8]`,
/// which borrow from the buffer, are not among them.
#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
struct Scalars {
    #[n(0)]
    flag: bool,
    #[n(1)]
    letter: char,
    #[n(2)]
    byte: u8,
    #[n(3)]
    short: u16,
    #[n(4)]
    word: u32,
    #[n(5)]
    long: u64,
    #[n(6)]
    wide: u128,
    #[n(7)]
    signed_byte: i8,
    #[n(8)]
    signed_short: i16,
    #[n(9)]
    signed_word: i32,
    #[n(10)]
    signed_long: i64,
    #[n(11)]
    signed_wide: i128,
    #[n(12)]
    single: f32,
    #[n(13)]
    double: f64,
}

const SCALARS: Scalars = Scalars {
    flag: true,
    letter: '\u{1f600}',
    byte: u8::MAX,
    short: u16::MAX,
    word: u32::MAX,
    long: u64::MAX,
    wide: u128::MAX,
    signed_byte: i8::MIN,
    signed_short: i16::MIN,
    signed_word: i32::MIN,
    signed_long: i64::MIN,
    signed_wide: i128::MIN,
    single: -0.5,
    double: 0.25,
};

#[zerializable(derive(Debug, PartialEq))]
trait Measured {
    #[n(0)]
    fn scalars(&self) -> Scalars;
}

struct OwnedMeasured(Scalars);

impl Measured for OwnedMeasured {
    fn scalars(&self) -> Scalars {
        self.0
    }
}

#[test]
fn a_value_holds_every_scalar() {
    let encoded = encode::<dyn Measured>(&OwnedMeasured(SCALARS));
    let view = decode::<dyn Measured>(&encoded).unwrap();

    // A value decodes back into itself, so every field of it is compared at
    // once by comparing the value.
    assert_eq!(view.scalars(), SCALARS);
    assert_eq!(view.scalars().letter, '\u{1f600}');
    assert_eq!(view.scalars().wide, u128::MAX);
    assert_eq!(encode::<dyn Measured>(&view), encoded);
}

/// The same set of primitives, as the fields of one variant of a choice.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Signal {
    #[variant(0)]
    Reading {
        #[n(0)]
        flag: bool,
        #[n(1)]
        letter: char,
        #[n(2)]
        byte: u8,
        #[n(3)]
        short: u16,
        #[n(4)]
        word: u32,
        #[n(5)]
        long: u64,
        #[n(6)]
        wide: u128,
        #[n(7)]
        signed_byte: i8,
        #[n(8)]
        signed_short: i16,
        #[n(9)]
        signed_word: i32,
        #[n(10)]
        signed_long: i64,
        #[n(11)]
        signed_wide: i128,
        #[n(12)]
        single: f32,
        #[n(13)]
        double: f64,
    },
    #[variant(1)]
    Silent,
}

fn reading() -> Signal {
    Signal::Reading {
        flag: SCALARS.flag,
        letter: SCALARS.letter,
        byte: SCALARS.byte,
        short: SCALARS.short,
        word: SCALARS.word,
        long: SCALARS.long,
        wide: SCALARS.wide,
        signed_byte: SCALARS.signed_byte,
        signed_short: SCALARS.signed_short,
        signed_word: SCALARS.signed_word,
        signed_long: SCALARS.signed_long,
        signed_wide: SCALARS.signed_wide,
        single: SCALARS.single,
        double: SCALARS.double,
    }
}

#[test]
fn a_variant_holds_every_scalar() {
    let encoded = encode::<Signal>(&reading());
    let decoded = decode::<Signal>(&encoded).unwrap();

    match decoded {
        Signal::Reading {
            letter,
            wide,
            signed_wide,
            double,
            ..
        } => {
            assert_eq!(letter, SCALARS.letter);
            assert_eq!(wide, u128::MAX);
            assert_eq!(signed_wide, i128::MIN);
            assert_eq!(double, 0.25);
        }
        Signal::Silent => panic!("decoded the wrong variant"),
    }
    assert_eq!(decoded, reading());
    assert_eq!(encode::<Signal>(&reading().as_ref()), encoded);
}

/// A value carried by a variant, which is what a choice holds by copying: a
/// value is not a schema, so it is named outright rather than by a parameter.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Sample {
    #[variant(0)]
    Measured(#[n(0)] Scalars, #[n(1)] Option<Scalars>),
    #[variant(1)]
    Missing,
}

#[test]
fn a_variant_holds_a_value() {
    for held in [Some(SCALARS), None] {
        let sample = Sample::Measured(SCALARS, held);
        let encoded = encode::<Sample>(&sample);
        let decoded = decode::<Sample>(&encoded).unwrap();

        match decoded {
            Sample::Measured(measured, optional) => {
                assert_eq!(measured, SCALARS);
                assert_eq!(optional, held);
            }
            Sample::Missing => panic!("decoded the wrong variant"),
        }
        assert_eq!(decoded, sample);
        // A value is carried by value, so borrowing what a variant holds
        // copies it rather than pointing at it.
        assert_eq!(encode::<Sample>(&sample.as_ref()), encoded);
    }
}

// ============================================================
// Option, in each of the three places a field may appear
// ============================================================

#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
struct Weight {
    #[n(0)]
    grams: u32,
}

/// One optional field per kind of field there is.
#[zerializable(derive(Debug, PartialEq))]
trait Parcel {
    #[n(0)]
    fn count(&self) -> Option<u32>;

    #[n(1)]
    fn label(&self) -> Option<&str>;

    #[n(2)]
    fn seal(&self) -> Option<&[u8]>;

    #[n(3)]
    fn weight(&self) -> Option<Weight>;

    #[n(4)]
    fn sender(&self) -> Option<impl Contact + '_>
    where
        Self: Sized;

    #[n(5)]
    fn reachable(&self) -> Option<Reachable<impl Contact + '_>>
    where
        Self: Sized;

    #[n(6)]
    fn recipients(&self) -> Option<impl List<Item = impl Contact + '_> + '_>
    where
        Self: Sized;
}

#[derive(Debug, Default)]
struct OwnedParcel {
    count: Option<u32>,
    label: Option<String>,
    seal: Option<Vec<u8>>,
    weight: Option<Weight>,
    sender: Option<OwnedContact>,
    reachable: Option<Reachable<OwnedContact>>,
    recipients: Option<OwnedList<OwnedContact>>,
}

impl Parcel for OwnedParcel {
    fn count(&self) -> Option<u32> {
        self.count
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    fn seal(&self) -> Option<&[u8]> {
        self.seal.as_deref()
    }

    fn weight(&self) -> Option<Weight> {
        self.weight
    }

    fn sender(&self) -> Option<impl Contact + '_>
    where
        Self: Sized,
    {
        self.sender.as_ref()
    }

    fn reachable(&self) -> Option<Reachable<impl Contact + '_>>
    where
        Self: Sized,
    {
        self.reachable.as_ref().map(Reachable::as_ref)
    }

    fn recipients(&self) -> Option<impl List<Item = impl Contact + '_> + '_>
    where
        Self: Sized,
    {
        self.recipients.as_ref()
    }
}

fn parcel() -> OwnedParcel {
    OwnedParcel {
        count: Some(3),
        label: Some("fragile".to_string()),
        seal: Some(vec![7, 8, 9]),
        weight: Some(Weight { grams: 500 }),
        sender: Some(OwnedContact("Ada".to_string())),
        reachable: Some(Reachable::By(OwnedContact("Grace".to_string()))),
        recipients: Some(
            vec![
                OwnedContact("Alan".to_string()),
                OwnedContact("Edsger".to_string()),
            ]
            .into(),
        ),
    }
}

#[test]
fn every_optional_field_round_trips_present() {
    let source = parcel();
    let encoded = encode::<dyn Parcel>(&source);
    let view = decode::<dyn Parcel>(&encoded).unwrap();

    assert_eq!(view.count(), Some(3));
    assert_eq!(view.label(), Some("fragile"));
    assert_eq!(view.seal(), Some(&[7, 8, 9][..]));
    assert_eq!(view.weight(), Some(Weight { grams: 500 }));
    assert_eq!(view.sender().unwrap().name(), "Ada");
    match view.reachable() {
        Some(Reachable::By(contact)) => assert_eq!(contact.name(), "Grace"),
        _ => panic!("decoded the wrong variant"),
    }
    let recipients = view.recipients().unwrap();
    assert_eq!(recipients.len(), 2);
    assert_eq!(recipients.get(1).unwrap().name(), "Edsger");

    assert_eq!(view, source);
    assert_eq!(encode::<dyn Parcel>(&view), encoded);
}

#[test]
fn every_optional_field_round_trips_absent() {
    let source = OwnedParcel::default();
    let encoded = encode::<dyn Parcel>(&source);
    let view = decode::<dyn Parcel>(&encoded).unwrap();

    assert_eq!(view.count(), None);
    assert_eq!(view.label(), None);
    assert_eq!(view.seal(), None);
    assert_eq!(view.weight(), None);
    assert!(view.sender().is_none());
    assert!(view.reachable().is_none());
    assert!(view.recipients().is_none());

    assert_eq!(view, source);
    assert_eq!(encode::<dyn Parcel>(&view), encoded);

    // `None` is a slot left unwritten, so an absent field costs nothing beyond
    // the offset already reserved for it.
    assert!(encoded.len() < encode::<dyn Parcel>(&parcel()).len());
}

#[test]
fn an_absent_field_differs_from_a_present_one() {
    let some = parcel();
    let none = OwnedParcel::default();
    let encoded = encode::<dyn Parcel>(&some);
    let view = decode::<dyn Parcel>(&encoded).unwrap();

    // Comparison reaches through the `Option`, so `Some` and `None` differ
    // whatever they wrap, and an empty list is not an absent one.
    assert_ne!(view, none);
    assert_ne!(
        decode::<dyn Parcel>(&encode::<dyn Parcel>(&none)).unwrap(),
        some
    );

    let empty = OwnedParcel {
        label: Some(String::new()),
        seal: Some(Vec::new()),
        recipients: Some(OwnedList::new()),
        ..OwnedParcel::default()
    };
    let encoded = encode::<dyn Parcel>(&empty);
    let view = decode::<dyn Parcel>(&encoded).unwrap();
    assert_eq!(view.label(), Some(""));
    assert_eq!(view.seal(), Some(&[][..]));
    assert!(view.recipients().unwrap().is_empty());
    assert_ne!(view, none);
    assert_eq!(view, empty);
}

#[test]
fn an_optional_field_prints_as_what_it_holds() {
    let encoded = encode::<dyn Parcel>(&parcel());
    let view = decode::<dyn Parcel>(&encoded).unwrap();
    let printed = format!("{view:?}");

    assert!(printed.contains("Some(3)"));
    assert!(printed.contains("Edsger"));

    let encoded = encode::<dyn Parcel>(&OwnedParcel::default());
    let printed = format!("{:?}", decode::<dyn Parcel>(&encoded).unwrap());
    assert!(printed.contains("None"));
}

#[test]
fn corrupt_optional_input_never_panics() {
    for source in [parcel(), OwnedParcel::default()] {
        let encoded = encode::<dyn Parcel>(&source);
        for index in 0..encoded.len() {
            for bit in [0x01, 0x40, 0x80] {
                let mut corrupted = encoded.clone();
                corrupted[index] ^= bit;
                let Ok(view) = decode::<dyn Parcel>(&corrupted) else {
                    continue;
                };
                let _ = view.count();
                let _ = view.label();
                let _ = view.seal();
                let _ = view.weight();
                let _ = view.sender().map(|sender| sender.name());
                let _ = view.reachable();
                if let Some(recipients) = view.recipients() {
                    for recipient in recipients.iter() {
                        let _ = recipient.name();
                    }
                }
            }
        }
    }
}

/// Optional fields of a value, which are `Copy` as everything a value holds is.
#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
struct Shipment {
    #[n(0)]
    weight: Option<Weight>,
    #[n(1)]
    priority: Option<u8>,
}

#[zerializable(derive(Debug, PartialEq))]
trait Manifest {
    #[n(0)]
    fn shipment(&self) -> Shipment;
}

struct OwnedManifest(Shipment);

impl Manifest for OwnedManifest {
    fn shipment(&self) -> Shipment {
        self.0
    }
}

#[test]
fn a_value_holds_optional_fields() {
    for shipment in [
        Shipment {
            weight: Some(Weight { grams: 250 }),
            priority: Some(9),
        },
        Shipment {
            weight: None,
            priority: Some(9),
        },
        Shipment {
            weight: None,
            priority: None,
        },
    ] {
        let encoded = encode::<dyn Manifest>(&OwnedManifest(shipment));
        let view = decode::<dyn Manifest>(&encoded).unwrap();

        assert_eq!(view.shipment(), shipment);
        assert_eq!(encode::<dyn Manifest>(&view), encoded);
    }
}

/// Optional fields of a choice's variant: one carrying a schema, one a scalar.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Delivery<C: Contact> {
    #[variant(0)]
    Sent {
        #[n(0)]
        to: Option<C>,
        #[n(1)]
        tracking: Option<u64>,
    },
    #[variant(1)]
    Pending,
}

#[test]
fn a_variant_holds_optional_fields() {
    let sent: Delivery<OwnedContact> = Delivery::Sent {
        to: Some(OwnedContact("Ada".to_string())),
        tracking: None,
    };
    let encoded = encode::<Delivery<dyn Contact>>(&sent);

    match decode::<Delivery<dyn Contact>>(&encoded).unwrap() {
        Delivery::Sent { to, tracking } => {
            assert_eq!(to.unwrap().name(), "Ada");
            assert_eq!(tracking, None);
        }
        Delivery::Pending => panic!("decoded the wrong variant"),
    }

    // The comparison across instantiations reaches through the `Option`, and
    // what a variant carries is borrowed rather than copied out of it.
    assert_eq!(decode::<Delivery<dyn Contact>>(&encoded).unwrap(), sent);
    let borrowed: Delivery<&OwnedContact> = sent.as_ref();
    assert_eq!(encode::<Delivery<dyn Contact>>(&borrowed), encoded);

    let empty: Delivery<OwnedContact> = Delivery::Sent {
        to: None,
        tracking: Some(7),
    };
    let encoded = encode::<Delivery<dyn Contact>>(&empty);
    let decoded = decode::<Delivery<dyn Contact>>(&encoded).unwrap();
    assert!(matches!(
        decoded,
        Delivery::Sent {
            to: None,
            tracking: Some(7)
        }
    ));
    assert_eq!(decoded, empty);
    assert_ne!(decode::<Delivery<dyn Contact>>(&encoded).unwrap(), sent);
}

/// A field added as an optional one, which is the difference an `Option` makes
/// to a schema that is already in use: `later` reads what `earlier` writes.
mod earlier {
    use zerialize::zerializable;

    #[zerializable(derive(Debug, PartialEq))]
    pub trait Record {
        #[n(0)]
        fn id(&self) -> u32;
    }

    pub struct OwnedRecord;

    impl Record for OwnedRecord {
        fn id(&self) -> u32 {
            7
        }
    }
}

mod later {
    use zerialize::zerializable;

    #[zerializable(derive(Debug, PartialEq))]
    pub trait Record {
        #[n(0)]
        fn id(&self) -> u32;

        #[n(1)]
        fn note(&self) -> Option<&str>;
    }

    pub struct OwnedRecord(pub Option<&'static str>);

    impl Record for OwnedRecord {
        fn id(&self) -> u32 {
            7
        }

        fn note(&self) -> Option<&str> {
            self.0
        }
    }
}

#[test]
fn a_field_added_as_optional_reads_both_ways() {
    // A slot the writer did not have and one it deliberately left out are the
    // same absent slot, so an older writer's message reads as `None` where a
    // required field would have been missing.
    let encoded = encode::<dyn earlier::Record>(&earlier::OwnedRecord);
    let view = decode::<dyn later::Record>(&encoded).unwrap();
    assert_eq!(view.id(), 7);
    assert_eq!(view.note(), None);

    // And the other way: an older reader skips the slot whether or not it was
    // written.
    for note in [Some("added later"), None] {
        let encoded = encode::<dyn later::Record>(&later::OwnedRecord(note));
        assert_eq!(decode::<dyn earlier::Record>(&encoded).unwrap().id(), 7);
    }
}

// ============================================================
// List
// ============================================================

#[zerializable(derive(Debug, PartialEq))]
trait Directory {
    #[n(0)]
    fn contacts(&self) -> impl List<Item = impl Contact + '_> + '_
    where
        Self: Sized;

    #[n(1)]
    fn reachable(&self) -> impl List<Item = Reachable<impl Contact + '_>> + '_
    where
        Self: Sized;
}

#[derive(Debug)]
struct OwnedDirectory {
    contacts: OwnedList<OwnedContact>,
    reachable: OwnedList<Reachable<OwnedContact>>,
}

/// A list of enums, built as its elements are asked for: what a list holds is
/// decided by the schema it names, so a choice is an element like any other.
struct Reachables<'a>(&'a OwnedList<Reachable<OwnedContact>>);

impl<'a> List for Reachables<'a> {
    type Item = Reachable<&'a OwnedContact>;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<Self::Item> {
        Some(self.0.as_slice().get(index)?.as_ref())
    }
}

impl Directory for OwnedDirectory {
    fn contacts(&self) -> impl List<Item = impl Contact + '_> + '_
    where
        Self: Sized,
    {
        &self.contacts
    }

    fn reachable(&self) -> impl List<Item = Reachable<impl Contact + '_>> + '_
    where
        Self: Sized,
    {
        Reachables(&self.reachable)
    }
}

fn directory() -> OwnedDirectory {
    OwnedDirectory {
        contacts: vec![
            OwnedContact("Ada".to_string()),
            OwnedContact("Grace".to_string()),
        ]
        .into(),
        reachable: vec![
            Reachable::By(OwnedContact("Alan".to_string())),
            Reachable::Never,
        ]
        .into(),
    }
}

#[test]
fn a_list_holds_messages_and_choices() {
    let source = directory();
    let encoded = encode::<dyn Directory>(&source);
    let view = decode::<dyn Directory>(&encoded).unwrap();

    let contacts = view.contacts();
    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts.get(0).unwrap().name(), "Ada");
    assert_eq!(contacts.get(1).unwrap().name(), "Grace");
    assert!(contacts.get(2).is_none());

    let reachable = view.reachable();
    assert_eq!(reachable.len(), 2);
    match reachable.get(0).unwrap() {
        Reachable::By(contact) => assert_eq!(contact.name(), "Alan"),
        Reachable::Never => panic!("decoded the wrong variant"),
    }
    assert!(matches!(reachable.get(1).unwrap(), Reachable::Never));

    assert_eq!(view, source);
    assert_eq!(encode::<dyn Directory>(&view), encoded);
}

#[test]
fn an_empty_list_round_trips() {
    let source = OwnedDirectory {
        contacts: OwnedList::new(),
        reachable: OwnedList::new(),
    };
    let encoded = encode::<dyn Directory>(&source);
    let view = decode::<dyn Directory>(&encoded).unwrap();

    assert!(view.contacts().is_empty());
    assert_eq!(view.reachable().iter().count(), 0);
    assert_eq!(view, source);
}

/// A list holds what a field holds: a primitive, a value, a view schema trait,
/// or a view enum.
#[zerializable(derive(Debug, PartialEq))]
trait Inventory {
    #[n(0)]
    fn counts(&self) -> impl List<Item = u32> + '_
    where
        Self: Sized;

    #[n(1)]
    fn marks(&self) -> impl List<Item = char> + '_
    where
        Self: Sized;

    #[n(2)]
    fn names(&self) -> impl List<Item = &str> + '_
    where
        Self: Sized;

    #[n(3)]
    fn blobs(&self) -> impl List<Item = &[u8]> + '_
    where
        Self: Sized;

    #[n(4)]
    fn weights(&self) -> impl List<Item = Weight> + '_
    where
        Self: Sized;
}

#[derive(Debug, Default)]
struct OwnedInventory {
    counts: OwnedList<u32>,
    marks: OwnedList<char>,
    names: Vec<String>,
    blobs: Vec<Vec<u8>>,
    weights: OwnedList<Weight>,
}

/// A list of what an implementation stores as `String`s and `Vec`s, handed out
/// as the handles a list of them holds.
struct Borrowed<'a, T>(&'a [T]);

impl<'a> List for Borrowed<'a, String> {
    type Item = &'a str;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&'a str> {
        Some(self.0.get(index)?.as_str())
    }
}

impl<'a> List for Borrowed<'a, Vec<u8>> {
    type Item = &'a [u8];

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&'a [u8]> {
        Some(self.0.get(index)?.as_slice())
    }
}

impl Inventory for OwnedInventory {
    fn counts(&self) -> impl List<Item = u32> + '_
    where
        Self: Sized,
    {
        // What a list of primitives holds, it holds by value, which is what
        // `Copied` hands out of a list that stores them.
        Copied(&self.counts)
    }

    fn marks(&self) -> impl List<Item = char> + '_
    where
        Self: Sized,
    {
        Copied(&self.marks)
    }

    fn names(&self) -> impl List<Item = &str> + '_
    where
        Self: Sized,
    {
        Borrowed(&self.names)
    }

    fn blobs(&self) -> impl List<Item = &[u8]> + '_
    where
        Self: Sized,
    {
        Borrowed(&self.blobs)
    }

    fn weights(&self) -> impl List<Item = Weight> + '_
    where
        Self: Sized,
    {
        Copied(&self.weights)
    }
}

fn inventory() -> OwnedInventory {
    OwnedInventory {
        counts: vec![0, 1, u32::MAX].into(),
        marks: vec!['a', '\u{1f600}'].into(),
        names: vec!["one".to_string(), String::new()],
        blobs: vec![vec![1, 2], Vec::new()],
        weights: vec![Weight { grams: 5 }, Weight { grams: 6 }].into(),
    }
}

#[test]
fn a_list_holds_primitives_and_values() {
    let source = inventory();
    let encoded = encode::<dyn Inventory>(&source);
    let view = decode::<dyn Inventory>(&encoded).unwrap();

    assert_eq!(view.counts().iter().collect::<Vec<_>>(), [0, 1, u32::MAX]);
    assert_eq!(view.marks().get(1).unwrap(), '\u{1f600}');
    assert_eq!(view.names().iter().collect::<Vec<_>>(), ["one", ""]);
    assert_eq!(view.blobs().get(0).unwrap(), &[1, 2][..]);
    assert!(view.blobs().get(1).unwrap().is_empty());
    assert_eq!(view.weights().get(1).unwrap(), Weight { grams: 6 });
    assert!(view.counts().get(3).is_none());

    // A list of elements is a view of them, the same size as any other list.
    assert_eq!(size_of_val(&view.counts()), size_of::<&[u8]>());
    assert_eq!(view, source);
    assert_eq!(encode::<dyn Inventory>(&view), encoded);
}

#[test]
fn an_empty_list_of_primitives_round_trips() {
    let source = OwnedInventory::default();
    let encoded = encode::<dyn Inventory>(&source);
    let view = decode::<dyn Inventory>(&encoded).unwrap();

    assert!(view.counts().is_empty());
    assert!(view.names().is_empty());
    assert_eq!(view, source);
}

#[test]
fn a_borrowed_element_points_into_the_buffer() {
    let encoded = encode::<dyn Inventory>(&inventory());
    let view = decode::<dyn Inventory>(&encoded).unwrap();
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();

    // An element that borrows is a handle over the buffer, exactly as the same
    // primitive is where a message holds it directly.
    let name = view.names().get(0).unwrap();
    assert!(buffer.contains(&(name.as_ptr() as usize)));
}

#[test]
fn corrupt_lists_of_primitives_never_panic() {
    let encoded = encode::<dyn Inventory>(&inventory());
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            let Ok(view) = decode::<dyn Inventory>(&corrupted) else {
                continue;
            };
            for count in view.counts().iter() {
                let _ = count;
            }
            for mark in view.marks().iter() {
                let _ = mark;
            }
            for name in view.names().iter() {
                let _ = name;
            }
            for blob in view.blobs().iter() {
                let _ = blob;
            }
            for weight in view.weights().iter() {
                let _ = weight;
            }
        }
    }
}

// ============================================================
// A value enum whose variants carry fields
// ============================================================

/// A value enum carrying fields, which is a value like any other: `Copy`,
/// decoded back into itself, and holding nothing that borrows.
#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
enum Protocol {
    #[variant(0)]
    Tcp {
        #[n(0)]
        port: u16,
    },
    #[variant(1)]
    Udp {
        #[n(0)]
        port: u16,
        #[n(1)]
        checked: Option<bool>,
    },
    #[variant(2)]
    Raw(#[n(0)] u8, #[n(1)] Weight),
    #[variant(3)]
    Unknown,
}

#[zerializable(derive(Debug, PartialEq))]
trait Endpoint {
    #[n(0)]
    fn protocol(&self) -> Protocol;

    #[n(1)]
    fn fallback(&self) -> Option<Protocol>;

    #[n(2)]
    fn tried(&self) -> impl List<Item = Protocol> + '_
    where
        Self: Sized;
}

#[derive(Debug)]
struct OwnedEndpoint {
    protocol: Protocol,
    fallback: Option<Protocol>,
    tried: OwnedList<Protocol>,
}

impl Endpoint for OwnedEndpoint {
    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn fallback(&self) -> Option<Protocol> {
        self.fallback
    }

    fn tried(&self) -> impl List<Item = Protocol> + '_
    where
        Self: Sized,
    {
        Copied(&self.tried)
    }
}

const PROTOCOLS: [Protocol; 5] = [
    Protocol::Tcp { port: 443 },
    Protocol::Udp {
        port: 53,
        checked: Some(true),
    },
    Protocol::Udp {
        port: 53,
        checked: None,
    },
    Protocol::Raw(7, Weight { grams: 9 }),
    Protocol::Unknown,
];

#[test]
fn a_value_enum_carries_fields() {
    for protocol in PROTOCOLS {
        let source = OwnedEndpoint {
            protocol,
            fallback: Some(Protocol::Unknown),
            tried: PROTOCOLS.to_vec().into(),
        };
        let encoded = encode::<dyn Endpoint>(&source);
        let view = decode::<dyn Endpoint>(&encoded).unwrap();

        // A value decodes back into itself, whatever its variant carries.
        assert_eq!(view.protocol(), protocol);
        assert_eq!(view.fallback(), Some(Protocol::Unknown));
        assert_eq!(view.tried().iter().collect::<Vec<_>>(), PROTOCOLS);
        assert_eq!(view, source);
        assert_eq!(encode::<dyn Endpoint>(&view), encoded);
    }
}

#[test]
fn a_value_enum_outlives_the_buffer_it_was_read_from() {
    // Nothing a value holds borrows, which holds of one carrying fields too.
    let protocol = {
        let source = OwnedEndpoint {
            protocol: Protocol::Raw(1, Weight { grams: 2 }),
            fallback: None,
            tried: OwnedList::new(),
        };
        let encoded = encode::<dyn Endpoint>(&source);
        decode::<dyn Endpoint>(&encoded).unwrap().protocol()
    };

    assert_eq!(protocol, Protocol::Raw(1, Weight { grams: 2 }));
}

#[test]
fn corrupt_value_enums_never_panic() {
    let source = OwnedEndpoint {
        protocol: Protocol::Udp {
            port: 53,
            checked: Some(false),
        },
        fallback: Some(Protocol::Tcp { port: 1 }),
        tried: PROTOCOLS.to_vec().into(),
    };
    let encoded = encode::<dyn Endpoint>(&source);
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            if let Ok(view) = decode::<dyn Endpoint>(&corrupted) {
                let _ = view.protocol();
                let _ = view.fallback();
                for protocol in view.tried().iter() {
                    let _ = protocol;
                }
            }
        }
    }
}

/// Value enum evolution: a reader rejects a variant it does not know, and skips
/// a field of one it does.
mod flags_v1 {
    use zerialize::{Zerializable, zerializable};

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub enum Flag {
        #[variant(0)]
        Set {
            #[n(0)]
            bits: u8,
        },
        #[variant(1)]
        Clear,
    }

    #[zerializable(derive(Debug, PartialEq))]
    pub trait Flagged {
        #[n(0)]
        fn flag(&self) -> Flag;
    }

    pub struct OwnedFlagged(pub Flag);

    impl Flagged for OwnedFlagged {
        fn flag(&self) -> Flag {
            self.0
        }
    }
}

mod flags_v2 {
    use zerialize::{Zerializable, zerializable};

    #[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
    pub enum Flag {
        #[variant(0)]
        Set {
            #[n(0)]
            bits: u8,
            #[n(1)]
            wide: u32,
        },
        #[variant(1)]
        Clear {
            #[n(0)]
            reason: Option<u8>,
        },
        #[variant(2)]
        Unknown,
    }

    #[zerializable(derive(Debug, PartialEq))]
    pub trait Flagged {
        #[n(0)]
        fn flag(&self) -> Flag;
    }

    pub struct OwnedFlagged(pub Flag);

    impl Flagged for OwnedFlagged {
        fn flag(&self) -> Flag {
            self.0
        }
    }
}

#[test]
fn a_value_enums_variants_evolve_like_a_choices() {
    // A field added to a variant is a slot the older reader never asks for.
    let encoded = encode::<dyn flags_v2::Flagged>(&flags_v2::OwnedFlagged(flags_v2::Flag::Set {
        bits: 3,
        wide: 9,
    }));
    assert_eq!(
        decode::<dyn flags_v1::Flagged>(&encoded).unwrap().flag(),
        flags_v1::Flag::Set { bits: 3 }
    );

    // A variant added to it is data that reader cannot represent.
    let encoded = encode::<dyn flags_v2::Flagged>(&flags_v2::OwnedFlagged(flags_v2::Flag::Unknown));
    assert_eq!(
        decode::<dyn flags_v1::Flagged>(&encoded).unwrap_err(),
        Error::UnknownVariant
    );

    // And a field it does not have is missing rather than absent.
    let encoded =
        encode::<dyn flags_v1::Flagged>(&flags_v1::OwnedFlagged(flags_v1::Flag::Set { bits: 3 }));
    assert_eq!(
        decode::<dyn flags_v2::Flagged>(&encoded).unwrap_err(),
        Error::MissingField
    );
}

#[test]
fn a_variant_that_gains_its_first_field_carries_an_absent_one() {
    // A variant carrying nothing wrote no payload at all, so the field it
    // gained is absent rather than missing: an optional one reads as `None`,
    // exactly as it would out of a message that gained it.
    let encoded = encode::<dyn flags_v1::Flagged>(&flags_v1::OwnedFlagged(flags_v1::Flag::Clear));
    assert_eq!(
        decode::<dyn flags_v2::Flagged>(&encoded).unwrap().flag(),
        flags_v2::Flag::Clear { reason: None }
    );
}

// ============================================================
// A view enum whose variants borrow from the buffer
// ============================================================

/// A view enum carrying borrowed fields, which is what its lifetime stands for:
/// the buffer they point into.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Note<'a, C: Contact> {
    #[variant(0)]
    Written {
        #[n(0)]
        text: &'a str,
        #[n(1)]
        seal: &'a [u8],
        #[n(2)]
        signed: Option<&'a str>,
    },
    #[variant(1)]
    From(#[n(0)] C),
    #[variant(2)]
    Blank,
}

/// One that borrows and carries no schema at all, which is named by its
/// lifetime alone.
#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Label<'a> {
    #[variant(0)]
    Text(#[n(0)] &'a str),
    #[variant(1)]
    None,
}

fn notes<'a>() -> [Note<'a, OwnedContact>; 3] {
    [
        Note::Written {
            text: "hello",
            seal: &[1, 2, 3],
            signed: Some("Ada"),
        },
        Note::Written {
            text: "",
            seal: &[],
            signed: None,
        },
        Note::Blank,
    ]
}

#[test]
fn a_variant_holds_borrowed_fields() {
    for note in notes() {
        let encoded = encode::<Note<'_, dyn Contact>>(&note);
        let decoded = decode::<Note<'_, dyn Contact>>(&encoded).unwrap();

        // The enum over views compares against the enum it was encoded from,
        // reaching through the fields that borrow.
        assert_eq!(decoded, note);
        assert_eq!(encode::<Note<'_, dyn Contact>>(&note.as_ref()), encoded);
    }

    let encoded = encode::<Note<'_, dyn Contact>>(&notes()[0]);
    match decode::<Note<'_, dyn Contact>>(&encoded).unwrap() {
        Note::Written { text, seal, signed } => {
            assert_eq!(text, "hello");
            assert_eq!(seal, &[1, 2, 3]);
            assert_eq!(signed, Some("Ada"));
        }
        _ => panic!("decoded the wrong variant"),
    }

    // A schema the enum carries is still a view of the buffer beside them.
    let from: Note<'_, OwnedContact> = Note::From(OwnedContact("Grace".to_string()));
    let encoded = encode::<Note<'_, dyn Contact>>(&from);
    match decode::<Note<'_, dyn Contact>>(&encoded).unwrap() {
        Note::From(contact) => assert_eq!(contact.name(), "Grace"),
        _ => panic!("decoded the wrong variant"),
    }
}

#[test]
fn a_borrowed_field_points_into_the_buffer() {
    let encoded = encode::<Note<'_, dyn Contact>>(&notes()[0]);
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();

    match decode::<Note<'_, dyn Contact>>(&encoded).unwrap() {
        Note::Written { text, seal, .. } => {
            assert!(buffer.contains(&(text.as_ptr() as usize)));
            assert!(buffer.contains(&(seal.as_ptr() as usize)));
        }
        _ => panic!("decoded the wrong variant"),
    }
}

#[test]
fn an_enum_that_borrows_and_carries_nothing_round_trips() {
    for label in [Label::Text("hi"), Label::Text(""), Label::None] {
        let encoded = encode::<Label<'_>>(&label);
        assert_eq!(decode::<Label<'_>>(&encoded).unwrap(), label);
        assert_eq!(encode::<Label<'_>>(&label.as_ref()), encoded);
    }
}

/// A message carrying enums that borrow: one on its own, and one per element of
/// a list.
#[zerializable(derive(Debug, PartialEq))]
trait Book {
    #[n(0)]
    fn title(&self) -> Label<'_>;

    #[n(1)]
    fn notes(&self) -> impl List<Item = Note<'_, impl Contact + '_>> + '_
    where
        Self: Sized;
}

#[derive(Debug)]
struct OwnedBook {
    title: String,
    notes: Vec<String>,
}

struct Notes<'a>(&'a [String]);

impl<'a> List for Notes<'a> {
    type Item = Note<'a, &'a OwnedContact>;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<Self::Item> {
        Some(Note::Written {
            text: self.0.get(index)?.as_str(),
            seal: &[],
            signed: None,
        })
    }
}

impl Book for OwnedBook {
    fn title(&self) -> Label<'_> {
        Label::Text(&self.title)
    }

    fn notes(&self) -> impl List<Item = Note<'_, impl Contact + '_>> + '_
    where
        Self: Sized,
    {
        Notes(&self.notes)
    }
}

#[test]
fn a_message_carries_enums_that_borrow() {
    let source = OwnedBook {
        title: "Notes".to_string(),
        notes: vec!["first".to_string(), "second".to_string()],
    };
    let encoded = encode::<dyn Book>(&source);
    let view = decode::<dyn Book>(&encoded).unwrap();

    assert_eq!(view.title(), Label::Text("Notes"));
    assert_eq!(view.notes().len(), 2);
    match view.notes().get(1).unwrap() {
        Note::Written { text, .. } => assert_eq!(text, "second"),
        _ => panic!("decoded the wrong variant"),
    }

    // What the carried enums borrow is the buffer the message was decoded from.
    let buffer = encoded.as_ptr() as usize..encoded.as_ptr() as usize + encoded.len();
    match view.title() {
        Label::Text(title) => assert!(buffer.contains(&(title.as_ptr() as usize))),
        Label::None => panic!("decoded the wrong variant"),
    }

    assert_eq!(view, source);
    assert!(format!("{view:?}").contains("second"));
    assert_eq!(encode::<dyn Book>(&view), encoded);
}

#[test]
fn corrupt_borrowed_enums_never_panic() {
    let source = OwnedBook {
        title: "Notes".to_string(),
        notes: vec!["first".to_string()],
    };
    let encoded = encode::<dyn Book>(&source);
    for index in 0..encoded.len() {
        for bit in [0x01, 0x40, 0x80] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= bit;
            let Ok(view) = decode::<dyn Book>(&corrupted) else {
                continue;
            };
            let _ = view.title();
            for note in view.notes().iter() {
                let _ = note;
            }
        }
    }
}

/// A value enum whose variants are each written a different way, and carry
/// nothing whichever way that is.
#[derive(Zerializable, Copy, Clone, Debug, PartialEq)]
enum Vacancy {
    #[variant(0)]
    Unit,
    #[variant(1)]
    Tuple(),
    #[variant(2)]
    Named {},
}

#[zerializable(derive(Debug, PartialEq))]
trait Vacancies {
    #[n(0)]
    fn vacancy(&self) -> Vacancy;
}

struct OwnedVacancies(Vacancy);

impl Vacancies for OwnedVacancies {
    fn vacancy(&self) -> Vacancy {
        self.0
    }
}

#[test]
fn a_value_variant_carrying_nothing_keeps_the_shape_it_was_written_with() {
    // `V`, `V()`, and `V {}` are three different declarations to Rust even
    // where all three carry nothing, so each is built and matched as written.
    for vacancy in [Vacancy::Unit, Vacancy::Tuple(), Vacancy::Named {}] {
        let encoded = encode::<dyn Vacancies>(&OwnedVacancies(vacancy));
        let view = decode::<dyn Vacancies>(&encoded).unwrap();

        assert_eq!(view.vacancy(), vacancy);
        assert_eq!(encode::<dyn Vacancies>(&view), encoded);
    }

    assert!(matches!(
        decode::<dyn Vacancies>(&encode::<dyn Vacancies>(&OwnedVacancies(Vacancy::Tuple())))
            .unwrap()
            .vacancy(),
        Vacancy::Tuple()
    ));
}

/// Choice evolution: `after` gives a variant that carried nothing its first
/// field, which is one an older writer's message does not have.
mod signal_before {
    use zerialize::zerializable;

    #[zerializable(derive(PartialEq))]
    #[derive(Debug)]
    pub enum Signal {
        #[variant(0)]
        Raise(#[n(0)] u32),
        #[variant(1)]
        Lower,
    }
}

mod signal_after {
    use zerialize::zerializable;

    #[zerializable(derive(PartialEq))]
    #[derive(Debug)]
    pub enum Signal {
        #[variant(0)]
        Raise(#[n(0)] u32),
        #[variant(1)]
        Lower {
            #[n(0)]
            reason: Option<u32>,
            #[n(1)]
            forced: Option<bool>,
        },
    }
}

#[test]
fn a_choices_variant_that_gains_its_first_field_carries_an_absent_one() {
    let encoded = encode::<signal_before::Signal>(&signal_before::Signal::Lower);
    assert_eq!(
        decode::<signal_after::Signal>(&encoded).unwrap(),
        signal_after::Signal::Lower {
            reason: None,
            forced: None,
        }
    );

    // A required field is missing rather than absent, as it is anywhere else.
    let encoded = encode::<signal_before::Signal>(&signal_before::Signal::Lower);
    assert!(decode::<signal_after::Signal>(&encoded).is_ok());
    assert_eq!(
        decode::<signal_before::Signal>(&encode::<signal_after::Signal>(
            &signal_after::Signal::Lower {
                reason: Some(1),
                forced: None,
            }
        ))
        .unwrap(),
        signal_before::Signal::Lower
    );
}
