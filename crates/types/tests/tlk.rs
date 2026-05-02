#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    resman::CachePolicy,
    tlk::{read_single_tlk, write_single_tlk},
};
use support::{read_shipped_dialog_tlk_bytes, require_game_resource, test_roundtrip_bytes};

#[test]
fn tlk_preserves_descriptor_fields_when_only_text_changes() -> Result<(), Box<dyn Error>> {
    let mut parsed = read_single_tlk(
        Cursor::new(require_game_resource(read_shipped_dialog_tlk_bytes())?),
        CachePolicy::Bypass,
    )?;
    let mut edited = parsed.get(0).expect("load entry").expect("entry present");
    let original_flags = edited.flags;
    let original_volume_variance = edited.volume_variance;
    let original_pitch_variance = edited.pitch_variance;
    let original_sound_length_bits = edited.sound_length_bits;
    let original_raw_sound_res_ref = edited.raw_sound_res_ref;
    edited.text.push_str(" [nwnrs]");
    parsed.set_entry(0, edited);

    let mut rewritten = Cursor::new(Vec::new());
    write_single_tlk(&mut rewritten, &mut parsed)?;

    let mut reparsed = read_single_tlk(Cursor::new(rewritten.into_inner()), CachePolicy::Bypass)?;
    let entry = reparsed
        .get(0)
        .expect("load rewritten entry")
        .expect("entry present");

    assert!(entry.text.ends_with(" [nwnrs]"));
    assert_eq!(entry.flags, original_flags);
    assert_eq!(entry.volume_variance, original_volume_variance);
    assert_eq!(entry.pitch_variance, original_pitch_variance);
    assert_eq!(entry.sound_length_bits, original_sound_length_bits);
    assert_eq!(entry.raw_sound_res_ref, original_raw_sound_res_ref);
    Ok(())
}

#[test]
fn tlk_roundtrip() -> Result<(), Box<dyn Error>> {
    let original = require_game_resource(read_shipped_dialog_tlk_bytes())?;
    test_roundtrip_bytes(&original, "dialog.tlk")
}
