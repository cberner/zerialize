//! What [`Error`] is, apart from the failures that produce it: the standard
//! implementations that let a decoding error be printed, compared, and carried
//! by the error types of the code that calls `decode`.

use std::collections::HashSet;
use std::error::Error as StdError;

use zerialize::{Error, decode, encode, zerializable};

const ALL: [Error; 9] = [
    Error::UnexpectedEof,
    Error::InvalidUtf8,
    Error::InvalidBool,
    Error::InvalidChar,
    Error::TrailingBytes,
    Error::MissingField,
    Error::RecursionLimit,
    Error::InvalidFrame,
    Error::UnknownVariant,
];

#[zerializable]
trait Label {
    #[n(0)]
    fn text(&self) -> &str;
}

struct OwnedLabel(String);

impl Label for OwnedLabel {
    fn text(&self) -> &str {
        &self.0
    }
}

#[test]
fn every_variant_prints_a_distinct_description() {
    let printed: HashSet<String> = ALL.iter().map(|error| error.to_string()).collect();

    assert_eq!(printed.len(), ALL.len());
    for description in &printed {
        assert!(!description.is_empty());
        assert!(!description.ends_with('.'));
    }
}

#[test]
fn an_error_is_a_std_error_with_no_source() {
    for error in ALL {
        let error: &dyn StdError = &error;
        assert!(error.source().is_none());
    }
}

#[test]
fn a_caller_may_carry_an_error_in_its_own_error_type() {
    fn read(bytes: &[u8]) -> Result<usize, Box<dyn StdError>> {
        Ok(decode::<dyn Label>(bytes)?.text().len())
    }

    let encoded = encode::<dyn Label>(&OwnedLabel("hello".into()));
    assert_eq!(read(&encoded).unwrap(), 5);
    assert_eq!(
        read(&encoded[..encoded.len() - 1]).unwrap_err().to_string(),
        Error::UnexpectedEof.to_string()
    );
}

#[test]
fn an_error_is_copied_rather_than_moved_and_may_be_hashed() {
    let error = Error::MissingField;
    let copy = error;

    assert_eq!(error, copy);
    assert_eq!(HashSet::from(ALL).len(), ALL.len());
}
