use std::collections::HashMap;

use nwnrs_types::{install, prelude::*};
use tracing::{info, instrument};

use crate::pack::{KeyPackageBif, write_key_package};

const PACKAGE_CACHE_SIZE_MB: usize = 64;
const PACKAGE_BIF_PREFIX: &str = "data";
const PACKAGE_FILES_PER_BIF: usize = 5000;

#[derive(Clone)]
pub(crate) struct PackageOptions {
    pub(crate) directory:        std::path::PathBuf,
    pub(crate) key:              String,
    pub(crate) root:             Option<std::path::PathBuf>,
    pub(crate) userdirectory:    Option<std::path::PathBuf>,
    pub(crate) language:         String,
    pub(crate) data_version:     String,
    pub(crate) data_compression: String,
    pub(crate) force:            bool,
}

#[derive(Clone)]
struct PackageEntry {
    rr:       resman::ResRef,
    sort_key: String,
    bytes:    Vec<u8>,
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(directory = %options.directory.display(), key = %options.key)
)]
pub(crate) fn run_package(options: PackageOptions) -> Result<(), String> {
    info!("packaging install-backed resource view");

    let root_override = options
        .root
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user_override = options
        .userdirectory
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    let root = install::find_nwnrs_root(&root_override).map_err(|error| error.to_string())?;
    let user = install::find_user_root(&user_override).map_err(|error| error.to_string())?;
    let mut resman = install::new_default_resman(
        &root,
        &user,
        &options.language,
        PACKAGE_CACHE_SIZE_MB,
        true,
        true,
        &[],
        &[],
        &[],
        &[],
    )
    .map_err(|error| error.to_string())?;

    let mut entries = collect_package_entries(&mut resman)?;
    entries.sort_by(|left, right| {
        left.sort_key
            .cmp(&right.sort_key)
            .then_with(|| left.rr.cmp(&right.rr))
    });

    let bif_count = 1 + entries.len() / PACKAGE_FILES_PER_BIF;
    let entries_per_bif = entries.len() / bif_count;
    let bifs_with_extra_entry = entries.len() % bif_count;
    let mut remaining_entries = entries.iter();
    let bifs = (0..bif_count)
        .map(|index| {
            let entry_count = entries_per_bif + usize::from(index < bifs_with_extra_entry);
            KeyPackageBif {
                directory: String::new(),
                name:      format!("pkg{index}"),
                entries:   remaining_entries
                    .by_ref()
                    .take(entry_count)
                    .map(|entry| entry.rr.clone())
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let payloads = entries
        .into_iter()
        .map(|entry| (entry.rr, entry.bytes))
        .collect::<HashMap<_, _>>();

    write_key_package(
        &options.directory,
        options.force,
        &options.key,
        PACKAGE_BIF_PREFIX,
        &bifs,
        &options.data_version,
        &options.data_compression,
        |rr| {
            payloads
                .get(rr)
                .cloned()
                .ok_or_else(|| format!("no packaged payload for {rr}"))
        },
    )
}

fn collect_package_entries(resman: &mut resman::ResMan) -> Result<Vec<PackageEntry>, String> {
    let mut refs = resman.contents().into_iter().collect::<Vec<_>>();
    refs.sort();

    let mut result = Vec::new();
    for rr in refs {
        let Some(ext) = resman::lookup_res_ext(rr.res_type()).map(|ext| ext.to_ascii_lowercase())
        else {
            continue;
        };

        let bytes = if is_included_package_extension(&ext) {
            resman
                .demand(&rr, resman::CachePolicy::Bypass)
                .map_err(|error| format!("failed to resolve packaged resource {rr}: {error}"))?
                .read_all(resman::CachePolicy::Bypass)
                .map_err(|error| format!("failed to read packaged resource {rr}: {error}"))?
        } else if is_stub_package_extension(&ext) {
            Vec::new()
        } else {
            continue;
        };

        result.push(PackageEntry {
            sort_key: resolved_resource_name(&rr).to_ascii_uppercase(),
            rr,
            bytes,
        });
    }

    Ok(result)
}

fn resolved_resource_name(rr: &resman::ResRef) -> String {
    rr.resolve()
        .map_or_else(|| rr.to_string(), |resolved| resolved.to_file())
}

fn is_included_package_extension(ext: &str) -> bool {
    matches!(
        ext,
        "wok"
            | "pwk"
            | "dwk"
            | "ncs"
            | "nss"
            | "uti"
            | "utc"
            | "utp"
            | "ssf"
            | "uts"
            | "utt"
            | "ute"
            | "utm"
            | "dlg"
            | "utw"
            | "utd"
            | "itp"
            | "2da"
            | "ini"
            | "set"
            | "ltr"
    )
}

fn is_stub_package_extension(ext: &str) -> bool {
    matches!(ext, "dds" | "tga" | "mdl" | "plt")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_types::prelude::{
        key::read_key_table_from_file,
        resman::{CachePolicy, ResContainer, ResolvedResRef},
    };

    use super::{PackageOptions, run_package};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-package-{prefix}-{nanos}"))
    }

    fn base_package_cmd(root: &Path, user: &Path, directory: &Path) -> PackageOptions {
        PackageOptions {
            directory:        directory.to_path_buf(),
            key:              "nwn_base".to_string(),
            root:             Some(root.to_path_buf()),
            userdirectory:    Some(user.to_path_buf()),
            language:         "english".to_string(),
            data_version:     "V1".to_string(),
            data_compression: "none".to_string(),
            force:            false,
        }
    }

    fn create_minimal_install_fixture(prefix: &str) -> (PathBuf, PathBuf, PathBuf) {
        let temp_dir = unique_test_dir(prefix);
        let root = temp_dir.join("root");
        let user = temp_dir.join("user");
        let ovr = root.join("ovr");
        let lang = root.join("lang").join("en");
        fs::create_dir_all(&ovr).expect("create ovr dir");
        fs::create_dir_all(&lang).expect("create language dir");
        fs::create_dir_all(&user).expect("create user dir");
        (temp_dir, root, user)
    }

    #[test]
    fn package_builds_slim_key_set_from_install_view() {
        let (temp_dir, root, user) = create_minimal_install_fixture("fixture");
        let destination = temp_dir.join("out");
        let ovr = root.join("ovr");
        fs::write(ovr.join("foo.nss"), b"void main() {}\n").expect("write nss fixture");
        fs::write(
            ovr.join("bar.2da"),
            b"2DA V2.0\nDEFAULT: ****\n\nLABEL\n0 value\n",
        )
        .expect("write 2da fixture");
        fs::write(ovr.join("baz.dds"), b"not-a-real-dds").expect("write dds fixture");
        fs::write(ovr.join("skip.tlk"), b"ignored").expect("write excluded tlk fixture");

        run_package(base_package_cmd(&root, &user, &destination)).expect("package install view");

        let key = read_key_table_from_file(destination.join("nwn_base.key")).expect("read key");
        let foo = ResolvedResRef::from_filename("foo.nss").expect("resolve foo.nss");
        let bar = ResolvedResRef::from_filename("bar.2da").expect("resolve bar.2da");
        let baz = ResolvedResRef::from_filename("baz.dds").expect("resolve baz.dds");
        let skip = ResolvedResRef::from_filename("skip.tlk").expect("resolve skip.tlk");

        assert_eq!(
            key.demand(foo.base())
                .expect("read foo")
                .read_all(CachePolicy::Bypass)
                .expect("read foo bytes"),
            b"void main() {}\n".to_vec()
        );
        assert_eq!(
            key.demand(bar.base())
                .expect("read bar")
                .read_all(CachePolicy::Bypass)
                .expect("read bar bytes"),
            b"2DA V2.0\nDEFAULT: ****\n\nLABEL\n0 value\n".to_vec()
        );
        assert_eq!(
            key.demand(baz.base())
                .expect("read baz")
                .read_all(CachePolicy::Bypass)
                .expect("read baz bytes"),
            Vec::<u8>::new()
        );
        assert!(
            key.demand(skip.base()).is_err(),
            "excluded resource should not be packaged"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_output_is_deterministic() {
        let (temp_dir, root, user) = create_minimal_install_fixture("deterministic");
        let ovr = root.join("ovr");
        fs::write(ovr.join("zeta.nss"), b"zeta").expect("write zeta");
        fs::write(ovr.join("alpha.2da"), b"alpha").expect("write alpha");
        fs::write(ovr.join("middle.dds"), b"middle").expect("write middle");

        let first = temp_dir.join("first");
        let second = temp_dir.join("second");
        run_package(base_package_cmd(&root, &user, &first)).expect("first package");
        run_package(base_package_cmd(&root, &user, &second)).expect("second package");

        assert_eq!(
            fs::read(first.join("nwn_base.key")).expect("read first key"),
            fs::read(second.join("nwn_base.key")).expect("read second key")
        );
        assert_eq!(
            fs::read(first.join("pkg0.bif")).expect("read first bif"),
            fs::read(second.join("pkg0.bif")).expect("read second bif")
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_splits_large_output_into_multiple_bifs() {
        let (temp_dir, root, user) = create_minimal_install_fixture("split");
        let destination = temp_dir.join("out");
        let ovr = root.join("ovr");
        for index in 0..5001 {
            let path = ovr.join(format!("r{index:04}.2da"));
            fs::write(path, format!("row-{index}\n")).expect("write 2da entry");
        }

        run_package(base_package_cmd(&root, &user, &destination)).expect("package large output");

        let key = read_key_table_from_file(destination.join("nwn_base.key")).expect("read key");
        assert_eq!(
            key.bifs(),
            vec!["data\\pkg0.bif".to_string(), "data\\pkg1.bif".to_string()]
        );
        assert_eq!(key.contents().len(), 5001);
        let bif_contents = key.bif_contents().expect("read packaged bif contents");
        let first_bif = bif_contents.first().expect("first packaged bif");
        let second_bif = bif_contents.get(1).expect("second packaged bif");
        assert_eq!(first_bif.resources.len(), 2501);
        assert_eq!(second_bif.resources.len(), 2500);
        assert!(destination.join("pkg0.bif").is_file());
        assert!(destination.join("pkg1.bif").is_file());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_rejects_missing_install_root() {
        let temp_dir = unique_test_dir("missing-root");
        let root = temp_dir.join("missing-root");
        let user = temp_dir.join("user");
        let destination = temp_dir.join("out");
        fs::create_dir_all(&user).expect("create user dir");

        let err = run_package(base_package_cmd(&root, &user, &destination))
            .expect_err("missing root should fail");
        assert!(err.contains("requested NWN root does not exist"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_rejects_missing_user_directory() {
        let (temp_dir, root, user) = create_minimal_install_fixture("missing-user");
        let destination = temp_dir.join("out");
        let missing_user = user.join("missing");

        let err = run_package(base_package_cmd(&root, &missing_user, &destination))
            .expect_err("missing user should fail");
        assert!(err.contains("requested user directory does not exist"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_rejects_missing_language_root() {
        let temp_dir = unique_test_dir("missing-language");
        let root = temp_dir.join("root");
        let user = temp_dir.join("user");
        let destination = temp_dir.join("out");
        fs::create_dir_all(root.join("ovr")).expect("create ovr");
        fs::create_dir_all(&user).expect("create user");

        let err = run_package(base_package_cmd(&root, &user, &destination))
            .expect_err("missing language should fail");
        assert!(err.contains("language"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn package_rejects_non_empty_destination_without_force() {
        let (temp_dir, root, user) = create_minimal_install_fixture("non-empty-destination");
        let destination = temp_dir.join("out");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(destination.join("existing.txt"), b"existing").expect("write existing file");

        let err = run_package(base_package_cmd(&root, &user, &destination))
            .expect_err("non-empty destination should fail");
        assert!(err.contains("target directory not empty"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
