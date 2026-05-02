#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::tga::{TgaImageType, read_tga, write_tga};
use support::{read_first_shipped_file_bytes_matching, require_game_resource};

fn read_shipped_tga_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let (_path, bytes) = require_game_resource(read_first_shipped_file_bytes_matching(
        "tga",
        |_path, bytes| read_tga(&mut Cursor::new(bytes)).is_ok(),
    ))?;
    Ok(bytes)
}

#[test]
fn shipped_tga_parses_expected_header_fields() -> Result<(), Box<dyn Error>> {
    let tga = read_tga(&mut Cursor::new(read_shipped_tga_bytes()?))?;

    assert_ne!(tga.image_type, TgaImageType::NoImage);
    assert!(tga.width > 0);
    assert!(tga.height > 0);
    assert!(matches!(tga.pixel_depth, 8 | 16 | 24 | 32));
    assert!(!tga.image_data.is_empty());
    Ok(())
}

#[test]
fn shipped_tga_decodes_to_rgba8() -> Result<(), Box<dyn Error>> {
    let tga = read_tga(&mut Cursor::new(read_shipped_tga_bytes()?))?;
    let rgba = tga.decode_rgba8()?;

    assert_eq!(rgba.len(), tga.width as usize * tga.height as usize * 4);
    Ok(())
}

#[test]
fn write_tga_roundtrips_shipped_bytes() -> Result<(), Box<dyn Error>> {
    let original = read_shipped_tga_bytes()?;
    let mut cursor = Cursor::new(original.clone());
    let tga = read_tga(&mut cursor)?;

    let mut encoded = Vec::new();
    write_tga(&mut encoded, &tga)?;

    assert_eq!(encoded, original);
    Ok(())
}
