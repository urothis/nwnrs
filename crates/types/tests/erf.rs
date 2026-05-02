#![allow(missing_docs)]

mod support;

use std::error::Error;

use support::test_shipped_archive;

#[test]
fn erf_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_archive("erf")
}

#[test]
fn mod_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_archive("mod")
}

#[test]
fn hak_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_archive("hak")
}
