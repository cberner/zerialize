//! What the encoding costs, and what it accepts.
//!
//! The wire format is not stable, so these do not pin down bytes for their own
//! sake. They pin down the properties the format is meant to have: a small
//! message costs a byte a field, a list of numbers costs the numbers, a frame
//! grows its offsets only when it has outgrown them, and a frame of one shape
//! is not read as another.

use zerialize::{Copied, Element, Error, List, Message, OwnedList, decode, encode, zerializable};

#[zerializable(derive(Debug, PartialEq))]
trait Triple {
    #[n(0)]
    fn a(&self) -> u8;

    #[n(1)]
    fn b(&self) -> u8;

    #[n(2)]
    fn c(&self) -> u8;
}

#[derive(Debug)]
struct OwnedTriple(u8, u8, u8);

impl Triple for OwnedTriple {
    fn a(&self) -> u8 {
        self.0
    }

    fn b(&self) -> u8 {
        self.1
    }

    fn c(&self) -> u8 {
        self.2
    }
}

#[test]
fn a_small_message_costs_a_byte_a_field() {
    // A control byte, a one byte length, one offset per slot, and the fields
    // themselves.
    let encoded = encode::<dyn Triple>(&OwnedTriple(1, 2, 3));
    assert_eq!(encoded.len(), 1 + 1 + 3 + 3);
    assert_eq!(
        decode::<dyn Triple>(&encoded).unwrap(),
        OwnedTriple(1, 2, 3)
    );
}

#[zerializable(derive(Debug, PartialEq))]
trait Numbers {
    #[n(0)]
    fn values(&self) -> impl List<Item = u64> + '_
    where
        Self: Sized;
}

struct OwnedNumbers(OwnedList<u64>);

impl Numbers for OwnedNumbers {
    fn values(&self) -> impl List<Item = u64> + '_ {
        Copied(&self.0)
    }
}

#[test]
fn a_list_of_numbers_costs_what_the_numbers_cost() {
    let values: OwnedList<u64> = (0..1000).collect();
    let encoded = encode::<dyn Numbers>(&OwnedNumbers(values));

    // A list of one width holds its elements one after another, so all it
    // costs beside them is its own header and the slot pointing at it.
    let payload = 1000 * size_of::<u64>();
    assert!(
        encoded.len() < payload + 16,
        "{} bytes for {payload} of numbers",
        encoded.len()
    );

    let decoded = decode::<dyn Numbers>(&encoded).unwrap();
    assert_eq!(decoded.values().len(), 1000);
    assert_eq!(decoded.values().get(999), Some(999));
    assert_eq!(decoded.values().iter().sum::<u64>(), (0..1000).sum());
}

#[zerializable(derive(Debug, PartialEq))]
trait Text {
    #[n(0)]
    fn head(&self) -> &str;

    #[n(1)]
    fn tail(&self) -> &str;
}

#[derive(Debug)]
struct OwnedText(String, String);

impl Text for OwnedText {
    fn head(&self) -> &str {
        &self.0
    }

    fn tail(&self) -> &str {
        &self.1
    }
}

#[test]
fn a_frame_widens_its_offsets_only_once_it_has_outgrown_them() {
    // Two strings that fit under a one byte offset, and the same two grown
    // past it. Both round trip, and the wide one pays only for the width.
    let narrow = OwnedText("a".repeat(100), "b".repeat(100));
    let wide = OwnedText("a".repeat(100), "b".repeat(300));

    let encoded = encode::<dyn Text>(&narrow);
    assert_eq!(encoded.len(), 1 + 1 + 2 + (1 + 100) + (1 + 100));
    assert_eq!(decode::<dyn Text>(&encoded).unwrap(), narrow);

    let encoded = encode::<dyn Text>(&wide);
    assert_eq!(encoded.len(), 1 + 2 + 4 + (1 + 100) + (2 + 300));
    assert_eq!(decode::<dyn Text>(&encoded).unwrap(), wide);
}

#[test]
fn a_string_longer_than_a_byte_of_length_round_trips() {
    // The length of a string is a variable width integer, so one past 127
    // characters is the first to need a second byte of it.
    for length in [0, 1, 127, 128, 16_383, 16_384] {
        let text = OwnedText("x".repeat(length), String::new());
        let encoded = encode::<dyn Text>(&text);
        assert_eq!(decode::<dyn Text>(&encoded).unwrap().head().len(), length);
    }
}

#[zerializable(derive(Debug, PartialEq))]
trait Wide {
    #[n(0)]
    fn a(&self) -> u8;
    #[n(7)]
    fn h(&self) -> u8;
    #[n(14)]
    fn o(&self) -> u8;
    #[n(20)]
    fn u(&self) -> u8;
}

#[derive(Debug)]
struct OwnedWide;

impl Wide for OwnedWide {
    fn a(&self) -> u8 {
        1
    }

    fn h(&self) -> u8 {
        8
    }

    fn o(&self) -> u8 {
        15
    }

    fn u(&self) -> u8 {
        21
    }
}

#[test]
fn a_message_of_more_slots_than_the_control_byte_holds_round_trips() {
    // Counts up to fourteen live in the control byte; past that the count
    // follows it, which is the only difference this makes.
    let encoded = encode::<dyn Wide>(&OwnedWide);
    assert_eq!(decode::<dyn Wide>(&encoded).unwrap(), OwnedWide);
}

#[zerializable(derive(PartialEq))]
#[derive(Debug)]
enum Choice {
    #[variant(0)]
    Nothing,
    #[variant(1)]
    Number(#[n(0)] u8),
    #[variant(300)]
    Far,
}

#[test]
fn an_enum_carrying_nothing_is_a_tag_and_a_header() {
    let encoded = encode::<Choice>(&Choice::Nothing);
    assert_eq!(encoded.len(), 1 + 1 + 1);
    assert_eq!(decode::<Choice>(&encoded).unwrap(), Choice::Nothing);

    let encoded = encode::<Choice>(&Choice::Number(7));
    assert_eq!(encoded.len(), 1 + 1 + 1 + 1 + 1);
    assert_eq!(decode::<Choice>(&encoded).unwrap(), Choice::Number(7));
}

#[test]
fn a_variant_numbered_past_a_byte_costs_a_second_one() {
    // A tag is a variable width integer, so a variant numbered past 127 is the
    // first to need two bytes of it and nothing else changes.
    let encoded = encode::<Choice>(&Choice::Far);
    assert_eq!(encoded.len(), 1 + 1 + 2);
    assert_eq!(decode::<Choice>(&encoded).unwrap(), Choice::Far);
}

#[test]
fn a_message_is_not_read_as_an_enum_nor_an_enum_as_a_message() {
    let message = encode::<dyn Triple>(&OwnedTriple(1, 2, 3));
    assert_eq!(decode::<Choice>(&message), Err(Error::InvalidFrame));

    let choice = encode::<Choice>(&Choice::Number(7));
    assert_eq!(decode::<dyn Triple>(&choice), Err(Error::InvalidFrame));
}

#[zerializable(derive(Debug, PartialEq))]
trait Labels {
    #[n(0)]
    fn values(&self) -> impl List<Item = &str> + '_
    where
        Self: Sized;
}

struct OwnedLabels(OwnedList<String>);

impl Labels for OwnedLabels {
    fn values(&self) -> impl List<Item = &str> + '_ {
        Strings(&self.0)
    }
}

struct Strings<'a>(&'a OwnedList<String>);

impl<'a> List for Strings<'a> {
    type Item = &'a str;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&'a str> {
        self.0.as_slice().get(index).map(String::as_str)
    }
}

#[test]
fn an_element_refuses_the_reader_it_is_not_reached_by() {
    // An element is reached either by its position among packed elements or
    // through a frame's table, and implements only the one its packing says it
    // is reached by. Asked for the other, it says so rather than reading
    // whatever the bytes it was handed happen to hold.
    let encoded = encode::<dyn Numbers>(&OwnedNumbers((0..4).collect()));
    assert_eq!(
        <u64 as Element>::decode_element(Message::trusted(&encoded), 0),
        Err(Error::InvalidFrame)
    );
    assert_eq!(
        <str as Element>::decode_packed(&encoded),
        Err(Error::InvalidFrame)
    );
}

#[test]
fn a_list_built_in_memory_holds_what_was_pushed_into_it() {
    // The list an implementation builds is the other side of the one decoding
    // gives back, and a list of either is handed out by the same trait.
    let mut values = OwnedList::new();
    assert!(values.is_empty());
    for value in 0..4u64 {
        values.push(value);
    }
    assert!(!values.is_empty());

    let encoded = encode::<dyn Numbers>(&OwnedNumbers(values));
    let decoded = decode::<dyn Numbers>(&encoded).unwrap();
    let copied = decoded.values();
    assert!(!copied.is_empty());
    assert_eq!(copied.iter().collect::<Vec<_>>(), [0, 1, 2, 3]);
    // A list is a handle over the buffer, so copying one costs nothing and
    // both halves read the same elements.
    assert_eq!(copied.get(2), decoded.values().get(2));
}

#[test]
fn a_list_of_one_shape_is_not_read_as_a_list_of_another() {
    // A list of numbers is packed and a list of strings is not, so the two
    // frames are told apart rather than read as each other.
    let numbers = encode::<dyn Numbers>(&OwnedNumbers((0..4).collect()));
    assert_eq!(decode::<dyn Labels>(&numbers), Err(Error::InvalidFrame));

    let labels = encode::<dyn Labels>(&OwnedLabels(
        ["a", "b"].map(str::to_string).into_iter().collect(),
    ));
    assert_eq!(decode::<dyn Numbers>(&labels), Err(Error::InvalidFrame));
}

#[zerializable(derive(Debug, PartialEq))]
trait Outer {
    #[n(0)]
    fn padding(&self) -> &str;

    #[n(1)]
    fn inner(&self) -> impl Text + '_
    where
        Self: Sized;
}

struct OwnedOuter {
    padding: String,
    inner: OwnedText,
}

impl Outer for OwnedOuter {
    fn padding(&self) -> &str {
        &self.padding
    }

    fn inner(&self) -> impl Text + '_ {
        &self.inner
    }
}

#[test]
fn widening_a_frame_leaves_the_frames_it_holds_alone() {
    // A frame that outgrows a byte of offsets is widened once it is finished,
    // which moves everything nested in it. Every one of those addresses itself
    // relative to its own start, so moving them leaves them readable.
    for padding in [1, 200, 300, 70_000] {
        let source = OwnedOuter {
            padding: "p".repeat(padding),
            inner: OwnedText("head".to_string(), "tail".to_string()),
        };
        let encoded = encode::<dyn Outer>(&source);
        let decoded = decode::<dyn Outer>(&encoded).unwrap();
        assert_eq!(decoded.padding().len(), padding);
        assert_eq!(decoded.inner().head(), "head");
        assert_eq!(decoded.inner().tail(), "tail");
    }
}

#[test]
fn a_count_said_to_follow_the_control_byte_is_read_where_it_follows() {
    // Whether the count follows the control byte is what the control byte
    // says, not what the count turns out to be. A frame that says it follows
    // and then writes one small enough to have fit inside is still read a word
    // further along, rather than a byte out of step with its own header.
    let inline = encode::<dyn Triple>(&OwnedTriple(1, 2, 3));

    let mut follows = vec![inline[0] | (0xf << 4), inline[1] + 1, 3];
    follows.extend(inline[2..5].iter().map(|offset| offset + 1));
    follows.extend_from_slice(&inline[5..]);

    assert_eq!(
        decode::<dyn Triple>(&follows).unwrap(),
        OwnedTriple(1, 2, 3)
    );
}

#[test]
fn an_offset_into_an_enum_tag_is_not_an_entry() {
    // The entries of an enum begin past the tag naming its variant, so a field
    // pointed at the tag is no more an entry than one pointed into the table.
    // A variant carrying one field is a control byte, a length, that field's
    // offset, the tag, and the field itself, so walking the offset back a byte
    // lands it on the tag, which would otherwise read as a field.
    let encoded = encode::<Choice>(&Choice::Number(7));
    assert_eq!(encoded.len(), 1 + 1 + 1 + 1 + 1);

    let mut corrupted = encoded.clone();
    corrupted[2] -= 1;
    assert_eq!(decode::<Choice>(&corrupted), Err(Error::UnexpectedEof));
}

#[test]
fn a_tag_naming_no_number_a_u64_holds_is_rejected() {
    // A variable width integer is refused where it names no number a u64
    // holds: one whose last group carries bits past the end of one, which
    // would otherwise be read with those bits dropped, and one that never ends
    // within the ten bytes a u64 reaches into.
    let overflowing = [&[0x80; 9][..], &[0x02]].concat();
    let unending = vec![0x80; 10];

    for tag in [overflowing, unending] {
        let mut encoded = encode::<Choice>(&Choice::Nothing);
        encoded.truncate(encoded.len() - 1);
        encoded.extend_from_slice(&tag);
        encoded[1] = encoded.len() as u8;

        assert_eq!(decode::<Choice>(&encoded), Err(Error::UnexpectedEof));
    }
}

#[test]
fn a_frame_naming_a_shape_the_format_does_not_have_is_rejected() {
    let mut encoded = encode::<dyn Triple>(&OwnedTriple(1, 2, 3));
    // The two bits naming the shape of a frame have a fourth value, which no
    // writer produces and no reader accepts.
    encoded[0] |= 0b1100;
    assert_eq!(decode::<dyn Triple>(&encoded), Err(Error::InvalidFrame));
}

#[zerializable(derive(Debug, PartialEq))]
trait Flags {
    #[n(0)]
    fn values(&self) -> impl List<Item = bool> + '_
    where
        Self: Sized;
}

struct OwnedFlags(OwnedList<bool>);

impl Flags for OwnedFlags {
    fn values(&self) -> impl List<Item = bool> + '_ {
        Copied(&self.0)
    }
}

#[test]
fn a_packed_element_that_is_not_one_is_rejected() {
    // A packed list of numbers is checked by its length alone, since any bytes
    // of their width are a number. A bool is one byte of which only two are a
    // bool, so a list of them is read element by element to be checked. The
    // elements are the end of the frame, so the last of them is the last byte.
    let mut encoded = encode::<dyn Flags>(&OwnedFlags(vec![true, false].into()));
    assert_eq!(
        decode::<dyn Flags>(&encoded)
            .unwrap()
            .values()
            .iter()
            .collect::<Vec<_>>(),
        [true, false]
    );

    *encoded.last_mut().expect("the list is not empty") = 2;
    assert_eq!(decode::<dyn Flags>(&encoded), Err(Error::InvalidBool));
}

/// Reads everything a decoded inventory holds, so that a message which decoded
/// is also walked.
fn walk(view: &LabelsView<'_>) -> usize {
    view.values().iter().map(str::len).sum()
}

#[test]
fn every_corruption_of_a_header_is_rejected_or_read() {
    // The control byte decides how the rest of the frame is read, so every bit
    // of every byte is flipped rather than the few a spot check would cover: a
    // buffer that decodes must be walkable, and one that does not must say so
    // rather than panic.
    let encoded = encode::<dyn Labels>(&OwnedLabels(
        ["one", "two", "three"]
            .map(str::to_string)
            .into_iter()
            .collect(),
    ));
    for index in 0..encoded.len() {
        for bit in 0..8 {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= 1 << bit;
            if let Ok(view) = decode::<dyn Labels>(&corrupted) {
                walk(&view);
            }
        }
    }
}

#[test]
fn every_truncation_of_a_message_is_rejected() {
    let encoded = encode::<dyn Outer>(&OwnedOuter {
        padding: "p".repeat(400),
        inner: OwnedText("head".to_string(), "tail".to_string()),
    });
    for length in 0..encoded.len() {
        assert!(decode::<dyn Outer>(&encoded[..length]).is_err());
    }
}
