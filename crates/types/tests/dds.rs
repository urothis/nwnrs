#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::dds::{
    DDS_RES_TYPE, DdsFormat, DdsTexture, NWN_DDS_HEADER_SIZE, read_dds, write_dds,
};
use support::{read_first_shipped_resource_bytes, require_game_resource};

#[test]
fn parses_shipped_nwn_dds_fixture() -> Result<(), Box<dyn Error>> {
    let (_resref, bytes) = require_game_resource(read_first_shipped_resource_bytes(DDS_RES_TYPE))?;
    let mut cursor = Cursor::new(bytes);
    let dds = read_dds(&mut cursor)?;

    assert!(matches!(dds.format, DdsFormat::Dxt1 | DdsFormat::Dxt5));
    assert!(dds.width > 0);
    assert!(dds.height > 0);
    assert!(dds.mip_count() > 0);
    assert!(
        dds.mip_levels
            .first()
            .is_some_and(|level| !level.data.is_empty())
    );
    assert!(dds.nwn_header.channels > 0);
    assert_eq!(NWN_DDS_HEADER_SIZE, 20);
    assert_eq!(DDS_RES_TYPE.0, 2033);
    Ok(())
}

#[test]
fn raw_texture_can_parse_as_dds() -> Result<(), Box<dyn Error>> {
    let (_resref, bytes) = require_game_resource(read_first_shipped_resource_bytes(DDS_RES_TYPE))?;
    let texture = DdsTexture::read_from_texture_bytes(&bytes)?;

    assert!(texture.width > 0);
    assert!(texture.height > 0);
    Ok(())
}

#[test]
fn shipped_fixture_decodes_top_level_rgba8() -> Result<(), Box<dyn Error>> {
    let (_resref, bytes) = require_game_resource(read_first_shipped_resource_bytes(DDS_RES_TYPE))?;
    let mut cursor = Cursor::new(bytes);
    let dds = read_dds(&mut cursor)?;
    let rgba = dds.decode_rgba8()?;

    assert_eq!(rgba.len(), dds.width as usize * dds.height as usize * 4);
    Ok(())
}

#[test]
fn shipped_fixture_roundtrips_exact_bytes() -> Result<(), Box<dyn Error>> {
    let (_resref, original_bytes) =
        require_game_resource(read_first_shipped_resource_bytes(DDS_RES_TYPE))?;
    let texture = DdsTexture::read_from_texture_bytes(&original_bytes)?;

    let mut encoded = Vec::new();
    write_dds(&mut encoded, &texture)?;

    assert_eq!(encoded, original_bytes);
    Ok(())
}
