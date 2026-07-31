//! The wire format read from the other side: these tests decode what
//! `encode` produced with the reference FlatBuffers implementation alone,
//! which is what a reader in another language, built from a `.fbs` schema,
//! would be doing.
//!
//! Each buffer is verified before it is read, since the accessors below are
//! unsafe exactly as far as they assume verification has happened.

use flatbuffers::{
    ForwardsUOffset, InvalidFlatbuffer, SkipSizePrefix, Table, VOffsetT, Vector, Verifiable,
    Verifier, VerifierOptions, field_index_to_field_offset,
};
use zerialize::{OwnedList, Zerializable, encode, zerializable};

/// Where the field a schema declares as `#[slot(N)]` is recorded in a vtable.
fn slot(number: VOffsetT) -> VOffsetT {
    field_index_to_field_offset(number)
}

fn verify<T: Verifiable>(buffer: &[u8]) {
    let options = VerifierOptions::default();
    let mut verifier = Verifier::new(&options, buffer);
    <SkipSizePrefix<ForwardsUOffset<T>>>::run_verifier(&mut verifier, 0)
        .expect("the buffer is a valid flatbuffer");
}

/// The root table of a size prefixed buffer, which is only sound to read
/// because [`verify`] ran over the same bytes first.
fn root(buffer: &[u8]) -> Table<'_> {
    // Safety: verified by the caller.
    unsafe { flatbuffers::size_prefixed_root_unchecked::<Table<'_>>(buffer) }
}

fn field<'buf, T: flatbuffers::Follow<'buf> + 'buf>(
    table: &Table<'buf>,
    number: VOffsetT,
) -> T::Inner {
    // Safety: the field's type is the one the schema declares for that slot,
    // and the buffer was verified against the same shape.
    unsafe { table.get::<T>(slot(number), None) }.expect("the field is present")
}

#[zerializable]
trait Address {
    #[slot(0)]
    fn city(&self) -> &str;

    #[slot(1)]
    fn zip(&self) -> u32;
}

#[zerializable]
trait Person {
    #[slot(0)]
    fn name(&self) -> &str;

    #[slot(1)]
    fn children(&self) -> impl zerialize::List<Item = impl Person + '_> + '_
    where
        Self: Sized;

    #[slot(2)]
    fn address(&self) -> impl Address + '_
    where
        Self: Sized;
}

struct OwnedAddress {
    city: String,
    zip: u32,
}

impl Address for OwnedAddress {
    fn city(&self) -> &str {
        &self.city
    }

    fn zip(&self) -> u32 {
        self.zip
    }
}

struct OwnedPerson {
    name: String,
    children: OwnedList<OwnedPerson>,
    address: OwnedAddress,
}

impl Person for OwnedPerson {
    fn name(&self) -> &str {
        &self.name
    }

    fn children(&self) -> impl zerialize::List<Item = impl Person + '_> + '_
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

fn person(name: &str, children: Vec<OwnedPerson>) -> OwnedPerson {
    OwnedPerson {
        name: name.to_string(),
        children: children.into(),
        address: OwnedAddress {
            city: "Berkeley".to_string(),
            zip: 94704,
        },
    }
}

/// `Address` as a `.fbs` schema would declare it.
struct AddressTable;

impl Verifiable for AddressTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<ForwardsUOffset<&str>>("city", slot(0), true)?
            .visit_field::<u32>("zip", slot(1), true)?
            .finish();
        Ok(())
    }
}

struct PersonTable;

impl Verifiable for PersonTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<ForwardsUOffset<&str>>("name", slot(0), true)?
            .visit_field::<ForwardsUOffset<Vector<'_, ForwardsUOffset<PersonTable>>>>(
                "children",
                slot(1),
                true,
            )?
            .visit_field::<ForwardsUOffset<AddressTable>>("address", slot(2), true)?
            .finish();
        Ok(())
    }
}

#[test]
fn a_buffer_is_size_prefixed() {
    let encoded = encode::<dyn Person>(&person("John", vec![]));
    let prefix = u32::from_le_bytes(encoded[..4].try_into().unwrap()) as usize;

    // Which is what gives a flatbuffer, whose root offset says nothing about
    // where the buffer ends, an extent of its own.
    assert_eq!(prefix, encoded.len() - 4);
}

#[test]
fn a_message_is_a_table() {
    let encoded = encode::<dyn Person>(&person(
        "John",
        vec![person("Jimmy", vec![person("Jenny", vec![])])],
    ));
    verify::<PersonTable>(&encoded);

    let john = root(&encoded);
    assert_eq!(field::<ForwardsUOffset<&str>>(&john, 0), "John");

    let address = field::<ForwardsUOffset<Table<'_>>>(&john, 2);
    assert_eq!(field::<ForwardsUOffset<&str>>(&address, 0), "Berkeley");
    assert_eq!(field::<u32>(&address, 1), 94704);

    // A list is a vector of offsets, so reaching an element is an index into
    // it rather than a walk over the elements before it.
    let children = field::<ForwardsUOffset<Vector<'_, ForwardsUOffset<Table<'_>>>>>(&john, 1);
    assert_eq!(children.len(), 1);
    let jimmy = children.get(0);
    assert_eq!(field::<ForwardsUOffset<&str>>(&jimmy, 0), "Jimmy");

    let grandchildren = field::<ForwardsUOffset<Vector<'_, ForwardsUOffset<Table<'_>>>>>(&jimmy, 1);
    assert_eq!(
        field::<ForwardsUOffset<&str>>(&grandchildren.get(0), 0),
        "Jenny"
    );
}

#[zerializable]
enum Role<P: Person> {
    #[variant(0)]
    Resident(#[slot(0)] P),
    #[variant(1)]
    Vacant,
}

/// An enum is a table of two slots, whose payload is a table of the fields the
/// variant the tag names carries.
struct RoleTable;

impl Verifiable for RoleTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<u32>("tag", slot(0), true)?
            .visit_field::<ForwardsUOffset<ResidentTable>>("payload", slot(1), false)?
            .finish();
        Ok(())
    }
}

struct ResidentTable;

impl Verifiable for ResidentTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<ForwardsUOffset<PersonTable>>("resident", slot(0), true)?
            .finish();
        Ok(())
    }
}

#[test]
fn an_enum_is_a_tag_and_a_payload() {
    let resident: Role<OwnedPerson> = Role::Resident(person("John", vec![]));
    let encoded = encode::<Role<dyn Person>>(&resident);
    verify::<RoleTable>(&encoded);

    let role = root(&encoded);
    assert_eq!(field::<u32>(&role, 0), 0);
    let payload = field::<ForwardsUOffset<Table<'_>>>(&role, 1);
    let john = field::<ForwardsUOffset<Table<'_>>>(&payload, 0);
    assert_eq!(field::<ForwardsUOffset<&str>>(&john, 0), "John");

    // A variant carrying nothing is the tag alone: the payload slot is absent
    // rather than empty.
    let vacant: Role<OwnedPerson> = Role::Vacant;
    let encoded = encode::<Role<dyn Person>>(&vacant);
    verify::<RoleTable>(&encoded);

    let role = root(&encoded);
    assert_eq!(field::<u32>(&role, 0), 1);
    // Safety: verified above.
    assert!(unsafe { role.get::<ForwardsUOffset<Table<'_>>>(slot(1), None) }.is_none());
}

#[derive(Zerializable, Copy, Clone)]
enum Suit {
    #[variant(0)]
    Clubs,
    #[variant(9)]
    Spades,
}

#[derive(Zerializable, Copy, Clone)]
struct Card {
    #[slot(0)]
    rank: u8,
    #[slot(1)]
    suit: Suit,
}

#[zerializable]
trait Hand {
    #[slot(0)]
    fn player(&self) -> &str;

    #[slot(1)]
    fn card(&self) -> Card;
}

struct OwnedHand;

impl Hand for OwnedHand {
    fn player(&self) -> &str {
        "Ada"
    }

    fn card(&self) -> Card {
        Card {
            rank: 12,
            suit: Suit::Spades,
        }
    }
}

struct HandTable;

impl Verifiable for HandTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<ForwardsUOffset<&str>>("player", slot(0), true)?
            .visit_field::<ForwardsUOffset<CardTable>>("card", slot(1), true)?
            .finish();
        Ok(())
    }
}

struct CardTable;

impl Verifiable for CardTable {
    fn run_verifier(verifier: &mut Verifier, position: usize) -> Result<(), InvalidFlatbuffer> {
        verifier
            .visit_table(position)?
            .visit_field::<u8>("rank", slot(0), true)?
            .visit_field::<u32>("suit", slot(1), true)?
            .finish();
        Ok(())
    }
}

#[test]
fn a_value_chooses_its_own_shape() {
    let encoded = encode::<dyn Hand>(&OwnedHand);
    verify::<HandTable>(&encoded);

    let hand = root(&encoded);
    assert_eq!(field::<ForwardsUOffset<&str>>(&hand, 0), "Ada");

    // A value struct is a table of its own, so it evolves the way a message
    // does, while a value enum is the number of its variant and nothing else.
    let card = field::<ForwardsUOffset<Table<'_>>>(&hand, 1);
    assert_eq!(field::<u8>(&card, 0), 12);
    assert_eq!(field::<u32>(&card, 1), 9);
}
