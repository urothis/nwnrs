use std::{env, fs, time::SystemTime};

use nwnrs_resman::ResContainer;
use nwnrs_resref::new_res_ref;
use nwnrs_restype::ResType;

use super::*;

#[test]
fn supports_explicit_resref_override() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::temp_dir().join(format!(
        "nwnrs-resfile-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::write(&root, b"payload")?;

    let rr = new_res_ref("custom", ResType(2010))?;
    let resfile = read_resfile_as(&root, rr.clone())?;
    assert!(resfile.contains(&rr));
    assert_eq!(resfile.demand(&rr)?.read_all(false)?, b"payload");

    fs::remove_file(root)?;

    Ok(())
}
