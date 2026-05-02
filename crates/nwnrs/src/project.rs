use std::{
    fs,
    io::{self, BufRead, Cursor, Write},
    path::{Path, PathBuf},
};

use nwnrs_nwpkg::{
    ProjectKind, ProjectLayout, write_new_erf_pack_metadata, write_new_key_pack_metadata,
    write_new_resource_pack_metadata, write_project_manifest,
};
use nwnrs_types::prelude::*;

use crate::{
    args::{InitCmd, NewCmd},
    util::{
        Kind, detect_kind, ensure_output_file_ready, ensure_target_dir_ready, infer_erf_type,
        should_skip_top_level_dir, sorted_dir_entries,
    },
};

pub(crate) fn run_init(cmd: InitCmd) -> Result<(), String> {
    let target = cmd.path.unwrap_or_else(|| PathBuf::from("."));
    ensure_init_target_ready(&target)?;
    let kind = resolve_project_kind(cmd.kind)?;
    scaffold_project(&target, &kind)
}

pub(crate) fn run_new(cmd: NewCmd) -> Result<(), String> {
    ensure_target_dir_ready(&cmd.path, false)?;
    let kind = resolve_project_kind(cmd.kind)?;
    scaffold_project(&cmd.path, &kind)
}

fn scaffold_project(target: &Path, kind: &ProjectKind) -> Result<(), String> {
    let project_name = project_name_for_path(target)?;
    let output_kind = project_output_kind(*kind)?;
    match kind.layout() {
        ProjectLayout::Resource => {
            scaffold_resource_project(target, &project_name, *kind, output_kind)
        }
        ProjectLayout::Erf => scaffold_erf_project(target, &project_name, *kind),
        ProjectLayout::Key => scaffold_key_project(target, &project_name, *kind),
    }
}

fn resolve_project_kind(kind: Option<String>) -> Result<ProjectKind, String> {
    match kind {
        Some(kind) => kind.parse(),
        None => prompt_for_project_kind(),
    }
}

fn prompt_for_project_kind() -> Result<ProjectKind, String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    select_project_kind_from(stdin.lock(), stdout.lock())
}

fn select_project_kind_from<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<ProjectKind, String> {
    let default_index = ProjectKind::all()
        .iter()
        .position(|kind| *kind == ProjectKind::DEFAULT)
        .ok_or_else(|| "default project kind is not in the supported kind list".to_string())?;

    loop {
        writeln!(writer, "Select project kind:")
            .map_err(|error| format!("failed to write prompt: {error}"))?;
        for (index, kind) in ProjectKind::all().iter().enumerate() {
            let marker = if index == default_index {
                " (default)"
            } else {
                ""
            };
            writeln!(writer, "{:>2}. {}{}", index + 1, kind, marker)
                .map_err(|error| format!("failed to write prompt: {error}"))?;
        }
        write!(writer, "> ").map_err(|error| format!("failed to write prompt: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("failed to flush prompt: {error}"))?;

        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| format!("failed to read selection: {error}"))?;
        if bytes == 0 {
            return Ok(ProjectKind::DEFAULT);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(ProjectKind::DEFAULT);
        }
        if let Ok(index) = trimmed.parse::<usize>()
            && let Some(kind) = index
                .checked_sub(1)
                .and_then(|offset| ProjectKind::all().get(offset))
        {
            return Ok(*kind);
        }
        if let Ok(kind) = trimmed.parse::<ProjectKind>() {
            return Ok(kind);
        }

        writeln!(
            writer,
            "invalid selection: {trimmed}. Enter a number or one of: {}",
            ProjectKind::all()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .map_err(|error| format!("failed to write prompt: {error}"))?;
    }
}

fn scaffold_resource_project(
    target: &Path,
    project_name: &str,
    kind: ProjectKind,
    output_kind: Kind,
) -> Result<(), String> {
    let file_name = match output_kind {
        Kind::Ncs => "main.nss".to_string(),
        _ => format!("{}.{}", project_name, kind),
    };
    let file_path = target.join(&file_name);
    write_resource_starter(&file_path, project_name, kind.as_str(), output_kind)?;
    if output_kind == Kind::Ncs {
        let langspec_path = target.join("nwscript.nss");
        ensure_output_file_ready(&langspec_path, false)?;
        fs::write(&langspec_path, minimal_langspec())
            .map_err(|error| format!("failed to write {}: {error}", langspec_path.display()))?;
    }
    write_project_manifest(target, kind, project_name, &file_name, false)?;
    let source_kind = if output_kind == Kind::Ncs {
        "nss"
    } else {
        kind.as_str()
    };
    write_new_resource_pack_metadata(target, source_kind, &file_name, false)
}

fn scaffold_erf_project(
    target: &Path,
    project_name: &str,
    kind: ProjectKind,
) -> Result<(), String> {
    let source_root = target.join("src");
    fs::create_dir_all(&source_root)
        .map_err(|error| format!("failed to create {}: {error}", source_root.display()))?;
    write_project_manifest(target, kind, project_name, "src", false)?;
    write_new_erf_pack_metadata(
        target,
        &infer_erf_type(Path::new(&format!("placeholder.{kind}")), None),
        erf::ErfVersion::V1,
        false,
    )
}

fn scaffold_key_project(
    target: &Path,
    project_name: &str,
    kind: ProjectKind,
) -> Result<(), String> {
    let source_root = target.join("data");
    fs::create_dir_all(&source_root)
        .map_err(|error| format!("failed to create {}: {error}", source_root.display()))?;
    write_project_manifest(target, kind, project_name, "data", false)?;
    write_new_key_pack_metadata(target, false)
}

fn write_resource_starter(
    path: &Path,
    project_name: &str,
    manifest_kind: &str,
    output_kind: Kind,
) -> Result<(), String> {
    ensure_output_file_ready(path, false)?;
    match output_kind {
        Kind::Gff => {
            let mut bytes = Cursor::new(Vec::new());
            let root = gff::GffRoot::new(gff_file_type_from_extension(manifest_kind));
            gff::write_gff_root(&mut bytes, &root)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            fs::write(path, bytes.into_inner())
                .map_err(|error| format!("failed to write {}: {error}", path.display()))
        }
        Kind::TwoDa => {
            let mut bytes = Vec::new();
            let mut table = twoda::TwoDa::new();
            table
                .set_columns(vec!["Label".to_string()])
                .map_err(|error| format!("failed to build {}: {error}", path.display()))?;
            table.set_row(0, vec![Some(project_name.to_string())]);
            twoda::write_twoda(&mut bytes, &table, false)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            fs::write(path, bytes)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))
        }
        Kind::Tlk => {
            let mut bytes = Cursor::new(Vec::new());
            let mut tlk = tlk::SingleTlk::new();
            tlk.set_text(0, project_name);
            tlk::write_single_tlk(&mut bytes, &mut tlk)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            fs::write(path, bytes.into_inner())
                .map_err(|error| format!("failed to write {}: {error}", path.display()))
        }
        Kind::Ssf => {
            let mut bytes = Vec::new();
            let ssf = ssf::SsfRoot::new();
            ssf::write_ssf(&mut bytes, &ssf)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            fs::write(path, bytes)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))
        }
        Kind::Model => fs::write(path, format!("newmodel {project_name}\n"))
            .map_err(|error| format!("failed to write {}: {error}", path.display())),
        Kind::Texture => match manifest_kind {
            "tga" => {
                let mut bytes = Vec::new();
                let texture = tga::TgaTexture::encode_rgba8(1, 1, &[255, 255, 255, 255])
                    .map_err(|error| format!("failed to build {}: {error}", path.display()))?;
                tga::write_tga(&mut bytes, &texture)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
                fs::write(path, bytes)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))
            }
            "dds" => {
                let mut bytes = Vec::new();
                let texture =
                    dds::DdsTexture::encode_rgba8(4, 4, dds::DdsFormat::Dxt5, &[255; 4 * 4 * 4])
                        .map_err(|error| format!("failed to build {}: {error}", path.display()))?;
                dds::write_dds(&mut bytes, &texture)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
                fs::write(path, bytes)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))
            }
            "plt" => {
                let mut bytes = Vec::new();
                let texture = plt::PltTexture {
                    file_type:     *b"PLT ",
                    file_version:  *b"V1  ",
                    unused1:       [0, 0, 0, 0],
                    unused2:       [0, 0, 0, 0],
                    width:         1,
                    height:        1,
                    pixels:        vec![plt::PltPixel {
                        value:    255,
                        layer_id: plt::PltLayer::Skin.id(),
                    }],
                    trailing_data: Vec::new(),
                };
                plt::write_plt(&mut bytes, &texture)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
                fs::write(path, bytes)
                    .map_err(|error| format!("failed to write {}: {error}", path.display()))
            }
            _ => Err(format!(
                "unsupported texture scaffold kind: {manifest_kind}"
            )),
        },
        Kind::Ncs => fs::write(path, "int StartingConditional() { return TRUE; }\n")
            .map_err(|error| format!("failed to write {}: {error}", path.display())),
        Kind::Erf | Kind::Key => Err(format!(
            "unsupported resource scaffold kind: {}",
            path.display()
        )),
    }
}

fn project_output_kind(kind: ProjectKind) -> Result<Kind, String> {
    detect_kind(Path::new(&format!("placeholder.{kind}")))
        .ok_or_else(|| format!("unsupported project kind: {kind}"))
}

fn ensure_init_target_ready(path: &Path) -> Result<(), String> {
    if path.exists() {
        if !path.is_dir() {
            return Err(format!("target is not a directory: {}", path.display()));
        }
        for entry in sorted_dir_entries(path)? {
            if should_skip_top_level_dir(&entry.path) {
                continue;
            }
            return Err(format!(
                "target directory is not empty; aborting for your own safety: {}",
                path.display()
            ));
        }
        return Ok(());
    }

    fs::create_dir_all(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))
}

fn project_name_for_path(path: &Path) -> Result<String, String> {
    if let Some(name) = path.file_name().and_then(|value| value.to_str())
        && !name.is_empty()
        && name != "."
    {
        return Ok(sanitize_project_name(name));
    }
    let cwd = std::env::current_dir().map_err(|error| format!("failed to read cwd: {error}"))?;
    if let Some(name) = cwd.file_name().and_then(|value| value.to_str())
        && !name.is_empty()
    {
        return Ok(sanitize_project_name(name));
    }
    Ok("nwproject".to_string())
}

fn sanitize_project_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "nwproject".to_string()
    } else {
        sanitized
    }
}

fn gff_file_type_from_extension(extension: &str) -> String {
    let mut file_type = extension.to_ascii_uppercase();
    file_type.truncate(4);
    while file_type.len() < 4 {
        file_type.push(' ');
    }
    file_type
}

fn minimal_langspec() -> &'static str {
    r#"#define ENGINE_NUM_STRUCTURES 0

int TRUE = 1;
int FALSE = 0;
"#
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use nwnrs_nwpkg::{read_erf_pack_metadata, read_resource_pack_metadata};

    use super::*;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-{prefix}-{nanos}"))
    }

    #[test]
    fn new_scaffolds_resource_project_with_manifest_and_lock() {
        let target = unique_test_dir("new-utc-project");
        let expected_name = project_name_for_path(&target).expect("derive project name");
        run_new(NewCmd {
            kind: Some("utc".to_string()),
            path: target.clone(),
        })
        .expect("scaffold utc project");

        assert!(target.join("nwproject.toml").is_file());
        assert!(target.join("nwproject.lock").is_file());
        assert!(target.join(format!("{expected_name}.utc")).is_file());

        let metadata = read_resource_pack_metadata(&target)
            .expect("read lock")
            .expect("resource lock present");
        assert_eq!(metadata.source_kind, "utc");
        assert_eq!(metadata.file_name, format!("{expected_name}.utc"));

        let _ = fs::remove_dir_all(target);
    }

    #[test]
    fn new_scaffolds_ncs_project_with_langspec() {
        let target = unique_test_dir("new-ncs-project");
        run_new(NewCmd {
            kind: Some("ncs".to_string()),
            path: target.clone(),
        })
        .expect("scaffold ncs project");

        assert!(target.join("main.nss").is_file());
        assert!(target.join("nwscript.nss").is_file());
        let metadata = read_resource_pack_metadata(&target)
            .expect("read lock")
            .expect("resource lock present");
        assert_eq!(metadata.source_kind, "nss");
        assert_eq!(metadata.file_name, "main.nss");

        let _ = fs::remove_dir_all(target);
    }

    #[test]
    fn init_scaffolds_archive_project_with_empty_lock() {
        let target = unique_test_dir("init-mod-project");
        fs::create_dir_all(&target).expect("create target dir");
        run_init(InitCmd {
            kind: Some("mod".to_string()),
            path: Some(target.clone()),
        })
        .expect("scaffold mod project");

        assert!(target.join("src").is_dir());
        let metadata = read_erf_pack_metadata(&target)
            .expect("read lock")
            .expect("erf lock present");
        assert_eq!(metadata.file_type, "MOD ");

        let _ = fs::remove_dir_all(target);
    }

    #[test]
    fn project_kind_picker_defaults_to_erf_on_empty_input() {
        let selected =
            select_project_kind_from("\n".as_bytes(), Vec::new()).expect("select default kind");
        assert_eq!(selected, ProjectKind::Erf);
    }

    #[test]
    fn project_kind_picker_accepts_numeric_selection() {
        let selected =
            select_project_kind_from("1\n".as_bytes(), Vec::new()).expect("select first kind");
        assert_eq!(selected, ProjectKind::TwoDa);
    }

    #[test]
    fn project_kind_picker_accepts_named_selection() {
        let selected =
            select_project_kind_from("UTC\n".as_bytes(), Vec::new()).expect("select named kind");
        assert_eq!(selected, ProjectKind::Utc);
    }
}
