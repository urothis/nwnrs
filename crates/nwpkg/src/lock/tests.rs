use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn snapshot_honors_cancellation_before_filesystem_access() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = snapshot_packable_files(Path::new("path-that-must-not-be-read"), &cancellation)
        .expect_err("cancel snapshot");
    assert_eq!(error.kind(), NwpkgErrorKind::Cancelled);
}

fn unique_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("nwnrs-nwpkg-{prefix}-{nanos}"))
}

#[test]
fn erf_metadata_file_round_trips_with_derived_dto() -> TestResult {
    let metadata = ErfPackMetadata {
        source:                PathBuf::from("example.erf"),
        source_sha256:         "a".repeat(64),
        file_type:             "ERF ".to_string(),
        file_version:          erf::ErfVersion::V1,
        build_year:            2025,
        build_day:             92,
        str_ref:               7,
        loc_strings:           BTreeMap::from([(
            0,
            "Hello\\x81<c\\x01\\x80\\xFF></c>".to_string(),
        )]),
        loc_string_bytes:      BTreeMap::from([(0, b"Hello\x81<c\x01\x80\xff></c>".to_vec())]),
        oid:                   Some("0123456789abcdef01234567".to_string()),
        resource_list_padding: 12,
        entry_order:           vec![resman::ResolvedResRef::from_filename("alpha.uti")?.into()],
        entry_algorithms:      BTreeMap::from([(
            resman::ResolvedResRef::from_filename("alpha.uti")?.into(),
            compressedbuf::Algorithm::Zlib,
        )]),
        file_sha256s:          BTreeMap::from([("alpha.uti".to_string(), "b".repeat(64))]),
    };

    let file = ErfPackMetadataFile {
        kind:                  MetadataKind::Erf,
        source:                metadata.source.clone(),
        source_sha256:         metadata.source_sha256.clone(),
        file_type:             metadata.file_type.clone(),
        file_version:          metadata.file_version.into(),
        build_year:            metadata.build_year,
        build_day:             metadata.build_day,
        str_ref:               metadata.str_ref,
        loc_strings:           metadata.loc_strings.clone(),
        loc_string_bytes:      metadata.loc_string_bytes.clone(),
        oid:                   metadata.oid.clone(),
        resource_list_padding: metadata.resource_list_padding,
        entry_order:           serialize_entry_order(&metadata.entry_order),
        entry_algorithms:      {
            let mut algorithms = BTreeMap::new();
            algorithms.insert(
                "alpha.uti".to_string(),
                compressedbuf::Algorithm::Zlib as u32,
            );
            algorithms
        },
        file_sha256s:          metadata.file_sha256s.clone(),
        dependency_resolution: None,
    };

    let encoded = toml::to_string(&file)?;
    let parsed = toml::from_str::<ErfPackMetadataFile>(&encoded)?;
    assert_eq!(
        ErfPackMetadata::from_file(parsed, Path::new("pack_metadata.json"))?,
        metadata
    );

    Ok(())
}

#[test]
fn key_metadata_file_round_trips_with_derived_dto() -> TestResult {
    let metadata = KeyPackMetadata {
        source_key:        PathBuf::from("test.key"),
        source_key_sha256: "a".repeat(64),
        bifs:              vec!["data/test.bif".to_string()],
        bif_sha256s:       BTreeMap::from([("data/test.bif".to_string(), "b".repeat(64))]),
        file_sha256s:      BTreeMap::from([("alpha.uti".to_string(), "c".repeat(64))]),
    };

    let encoded = toml::to_string(&KeyPackMetadataFile::from(metadata.clone()))?;
    let parsed = toml::from_str::<KeyPackMetadataFile>(&encoded)?;
    assert_eq!(
        KeyPackMetadata::from_file(parsed, Path::new("pack_metadata.json"))?,
        metadata
    );

    Ok(())
}

#[test]
fn lockfile_rejects_unknown_fields() -> TestResult {
    let root = unique_test_dir("strict-lock");
    fs::create_dir_all(&root)?;
    fs::write(
        root.join(PACKAGE_LOCK_FILENAME),
        r#"{
            "kind": "resource",
            "source": "example.utc",
            "source_sha256": "abc123",
            "source_kind": "gff-json",
            "file_name": "example.utc.json",
            "file_sha256s": {},
            "unexpected": true
        }"#,
    )?;

    let error = read_resource_pack_metadata(&root).expect_err("reject unknown lock field");
    assert_eq!(error.kind(), NwpkgErrorKind::Parse);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn entry_order_parser_uses_resolved_resref_strings() -> TestResult {
    let entries = parse_entry_order(vec!["Alpha.uti".to_string()], Path::new("meta.json"))?;
    assert_eq!(
        entries,
        vec![resman::ResRef::new("Alpha", resman::ResType(2025))?]
    );
    Ok(())
}

#[test]
fn snapshot_excludes_control_files() {
    let root = unique_test_dir("snapshot");
    fs::create_dir_all(&root).expect("create temp dir");
    fs::write(root.join(PACKAGE_LOCK_FILENAME), "{}").expect("write lock");
    fs::write(root.join(crate::PROJECT_MANIFEST_FILENAME), "[project]").expect("write manifest");
    fs::write(root.join("alpha.utc"), b"bytes").expect("write resource");

    let snapshot =
        snapshot_packable_files(&root, &CancellationToken::new()).expect("snapshot files");
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot.contains_key("alpha.utc"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unchanged_sha256_snapshot_reuses_original_erf() -> TestResult {
    let root = unique_test_dir("original-erf-reuse");
    let source = root.join("original.mod");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    fs::write(&source, b"original archive bytes")?;
    fs::write(project.join("module.ifo.json"), b"editable source")?;

    let metadata = ErfPackMetadata {
        source:                source.clone(),
        source_sha256:         checksums::sha256_digest(fs::read(&source)?).to_string(),
        file_type:             "MOD ".to_string(),
        file_version:          erf::ErfVersion::V1,
        build_year:            0,
        build_day:             0,
        str_ref:               -1,
        loc_strings:           BTreeMap::new(),
        loc_string_bytes:      BTreeMap::new(),
        oid:                   None,
        resource_list_padding: 0,
        entry_order:           Vec::new(),
        entry_algorithms:      BTreeMap::new(),
        file_sha256s:          snapshot_packable_files(&project, &CancellationToken::new())?,
    };

    assert!(should_copy_original_erf(&metadata, &project)?);
    fs::write(project.join("module.ifo.json"), b"edited source")?;
    assert!(!should_copy_original_erf(&metadata, &project)?);

    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn bif_resolution_matches_each_path_component_case_insensitively() -> TestResult {
    let root = unique_test_dir("bif-case-resolution");
    fs::create_dir_all(root.join("Data"))?;
    fs::write(root.join("Data/Textures.BIF"), b"bif")?;

    let resolved = resolve_existing_key_bif_path(&root.join("chitin.key"), "data/textures.bif")?;
    assert_eq!(resolved, fs::canonicalize(root.join("Data/Textures.BIF"))?);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn bif_resolution_observes_precancellation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = resolve_existing_key_bif_path_with_cancellation(
        Path::new("path-that-must-not-be-read/chitin.key"),
        "data.bif",
        &cancellation,
    )
    .expect_err("cancel BIF resolution");
    assert_eq!(error.kind(), NwpkgErrorKind::Cancelled);
}

#[cfg(target_os = "linux")]
#[test]
fn bif_resolution_rejects_ambiguous_case_matches() -> TestResult {
    let root = unique_test_dir("bif-case-ambiguity");
    fs::create_dir_all(&root)?;
    fs::write(root.join("Alpha.bif"), b"first")?;
    fs::write(root.join("alpha.BIF"), b"second")?;

    let error = resolve_existing_key_bif_path(&root.join("chitin.key"), "Alpha.bif")
        .expect_err("ambiguous BIF spelling must fail");
    assert!(error.to_string().contains("ambiguous case-insensitively"));

    fs::remove_dir_all(root)?;
    Ok(())
}
