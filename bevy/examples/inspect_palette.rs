//! Prints basic information about installed NWN palette textures.

use std::collections::BTreeSet;

use nwnrs_game::prelude::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_resref::prelude::ResolvedResRef;
use nwnrs_tga::prelude::read_tga_from_res;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!("usage: inspect_palette <resref> [resref...]");
        std::process::exit(2);
    }

    let root = find_nwnrs_root("").unwrap_or_else(|error| {
        panic!("resolve NWN root: {error}");
    });
    let user_root = find_user_root("").unwrap_or_else(|error| {
        panic!("resolve NWN user root: {error}");
    });
    let mut resman = new_default_resman(
        &root,
        &user_root,
        "english",
        0,
        true,
        false,
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap_or_else(|error| {
        panic!("build default resman: {error}");
    });

    for stem in args {
        let resolved = ResolvedResRef::from_filename(&format!("{stem}.tga"))
            .unwrap_or_else(|error| panic!("invalid resref {stem}.tga: {error}"));
        let res = resman
            .get_resolved(&resolved)
            .unwrap_or_else(|| panic!("resource not found: {}", resolved.to_file()));
        let tga = read_tga_from_res(&res, true).unwrap_or_else(|error| {
            panic!("read {}: {error}", resolved.to_file());
        });
        let rgba = tga.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode {}: {error}", resolved.to_file());
        });

        let mut unique = BTreeSet::new();
        let mut ordered = Vec::new();
        for color in rgba
            .chunks_exact(4)
            .map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3]])
        {
            if color[3] == 0 {
                continue;
            }
            if unique.insert(color) {
                ordered.push(color);
            }
        }

        println!(
            "palette={} width={} height={} unique_opaque={}",
            stem,
            tga.width,
            tga.height,
            ordered.len()
        );
        for (index, color) in ordered.iter().take(24).enumerate() {
            println!(
                "  color[{index}]={:02X}{:02X}{:02X}{:02X}",
                color[0], color[1], color[2], color[3]
            );
        }
    }
}
