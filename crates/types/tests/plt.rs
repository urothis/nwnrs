#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    plt::{
        PLT_HEADER_SIZE, PLT_RES_TYPE, PLT_SIGNATURE, PltLayer, PltPixel, PltTexture, read_plt,
        write_plt,
    },
    resman::CachePolicy,
};
use support::{
    demand_resource, read_resource_bytes, require_game_resource, skip_if_game_resources_unavailable,
};

#[test]
fn fixture_plt_parses_expected_header_fields() -> Result<(), Box<dyn Error>> {
    let res = match require_game_resource(demand_resource("cloak_001", PLT_RES_TYPE)) {
        Ok(res) => res,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let plt = PltTexture::from_res(&res, CachePolicy::Use).unwrap_or_else(|error| {
        panic!("read shipped plt fixture: {error}");
    });

    assert_eq!(PLT_SIGNATURE, b"PLT V1  ");
    assert_eq!(PLT_HEADER_SIZE, 24);
    assert_eq!(PLT_RES_TYPE.0, 6);
    assert_eq!(plt.file_type, *b"PLT ");
    assert_eq!(plt.file_version, *b"V1  ");
    assert_eq!(plt.unused1, [10, 0, 0, 0]);
    assert_eq!(plt.unused2, [0, 0, 0, 0]);
    assert_eq!(plt.width, 512);
    assert_eq!(plt.height, 512);
    assert_eq!(plt.pixels.len(), 512 * 512);
    assert_eq!(
        plt.pixels.first(),
        Some(&PltPixel {
            value:    71,
            layer_id: 5,
        })
    );
    assert_eq!(
        plt.pixels.first().copied().and_then(PltPixel::layer),
        Some(PltLayer::Cloth2)
    );
    assert!(plt.trailing_data.is_empty());
    Ok(())
}

#[test]
fn write_plt_roundtrips_fixture_bytes() -> Result<(), Box<dyn Error>> {
    let original = match require_game_resource(read_resource_bytes("cloak_001", PLT_RES_TYPE)) {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let mut cursor = Cursor::new(original.clone());
    let plt = read_plt(&mut cursor).unwrap_or_else(|error| {
        panic!("parse fixture plt: {error}");
    });

    let mut encoded = Vec::new();
    if let Err(error) = write_plt(&mut encoded, &plt) {
        panic!("write plt: {error}");
    }

    assert_eq!(encoded, original);
    Ok(())
}
