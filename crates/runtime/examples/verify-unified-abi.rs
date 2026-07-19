//! Verifies a generated Unified ABI snapshot against committed target packs.

use std::{error::Error, ffi::OsString, fs, path::PathBuf};

use nwnrs_runtime::{TargetPack, load_abi_snapshot, validate_abi_snapshot};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let snapshot_path = required_argument(&mut arguments, "ABI snapshot")?;
    let targets_root = PathBuf::from(required_argument(&mut arguments, "target-pack root")?);
    if arguments.next().is_some() {
        return Err("usage: verify-unified-abi ABI_SNAPSHOT TARGET_ROOT".into());
    }

    let snapshot = load_abi_snapshot(snapshot_path)?;
    let platform_root = targets_root.join(snapshot.platform.directory_name());
    let mut verified = 0_u32;
    for entry in fs::read_dir(&platform_root)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let pack = toml::from_str::<TargetPack>(&fs::read_to_string(&path)?)?;
        validate_abi_snapshot(&snapshot, &pack)?;
        verified = verified
            .checked_add(1)
            .ok_or("verified target-pack count overflowed")?;
    }
    if verified == 0 {
        return Err(format!(
            "no target packs matched ABI snapshot platform {} in {}",
            snapshot.platform,
            platform_root.display()
        )
        .into());
    }
    Ok(())
}

fn required_argument(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn Error>> {
    arguments.next().ok_or_else(|| {
        format!("missing {name}; usage: verify-unified-abi ABI_SNAPSHOT TARGET_ROOT").into()
    })
}
