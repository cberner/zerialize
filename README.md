# zerialize

![CI](https://github.com/cberner/zerialize/actions/workflows/ci.yml/badge.svg)
[![Crates.io](https://img.shields.io/crates/v/zerialize.svg)](https://crates.io/crates/zerialize)
[![Documentation](https://docs.rs/zerialize/badge.svg)](https://docs.rs/zerialize)
[![License](https://img.shields.io/crates/l/zerialize)](https://crates.io/crates/zerialize)
[![dependency status](https://deps.rs/repo/github/cberner/zerialize/status.svg)](https://deps.rs/repo/github/cberner/zerialize)

Zerialize provides zero-cost deserialization and a natural interface that allows code to be written
against the traits, structs, and enums in the codebase, without referencing any generated types.

The wire format is [FlatBuffers](https://flatbuffers.dev), so what it writes is readable by any
FlatBuffers implementation, and a schema written here is one that could equally have been written
in a `.fbs` file. Buffers are size prefixed.


## License

Licensed under either of

* [Apache License, Version 2.0](LICENSE-APACHE)
* [MIT License](LICENSE-MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
