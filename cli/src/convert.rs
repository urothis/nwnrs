use std::{
    ffi::OsStr,
    fs::File,
    io::{Cursor, Write},
    path::Path,
};

use image::{DynamicImage, ImageFormat, RgbaImage};
use nwnrs::prelude::*;
use tracing::info;

use crate::{args::ConvertCmd, util::ensure_output_file_ready};

struct DecodedImage {
    width:  u32,
    height: u32,
    rgba:   Vec<u8>,
}

pub(crate) fn run_convert(cmd: &ConvertCmd) -> Result<(), String> {
    info!("converting file");

    if cmd.list_animations {
        return run_list_animations(cmd);
    }

    let output = require_convert_output(cmd)?;
    ensure_output_file_ready(output, cmd.force)?;

    if is_obj_conversion(output)? {
        return run_convert_obj(cmd);
    }

    ensure_animation_options_unused(cmd)?;

    if is_model_conversion(&cmd.input, output)? {
        return run_convert_model(cmd);
    }

    info!("treating conversion as texture/image conversion");
    let decoded = read_input_image(&cmd.input)?;
    let output_ext = output_extension(output)?;
    match output_ext.as_str() {
        "dds" => write_output_dds(output, &decoded, parse_dds_format(&cmd.dds_format)?)?,
        "tga" => write_output_tga(output, &decoded)?,
        "webp" => write_output_webp(output, &decoded)?,
        other => {
            return Err(format!("unsupported convert output format: {other}"));
        }
    }

    Ok(())
}

fn is_obj_conversion(output: &Path) -> Result<bool, String> {
    Ok(output_extension(output)? == "obj")
}

fn is_model_conversion(input: &Path, output: &Path) -> Result<bool, String> {
    let input_ext = output_extension(input)?;
    let output_ext = output_extension(output)?;
    Ok(input_ext == "mdl" || output_ext == "mdl")
}

fn run_convert_obj(cmd: &ConvertCmd) -> Result<(), String> {
    let output = require_convert_output(cmd)?;
    if output_extension(output)? != "obj" {
        return Err(format!(
            "obj conversion requires output to end in .obj: {}",
            output.display()
        ));
    }

    let input_ext = output_extension(&cmd.input)?;

    match input_ext.as_str() {
        "mdl" => {
            let scene = mdl::NwnScene::from_auto_file(&cmd.input).map_err(|error| {
                format!(
                    "failed to parse {} as scene MDL: {error}",
                    cmd.input.display()
                )
            })?;
            let scene = snapshot_scene_for_export(&scene, cmd)?;
            let mut output_file = File::create(output)
                .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
            mdl::write_scene_obj(&mut output_file, &scene)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))
        }
        "utc" => {
            let root = read_gff_root_from_file(&cmd.input)?;
            let mut resman = build_install_resman(cmd)?;
            let composed =
                mdl::compose_player_creature_from_utc(&mut resman, &root).map_err(|error| {
                    format!(
                        "failed to compose equipped creature from {}: {error}",
                        cmd.input.display()
                    )
                })?;
            let composed = snapshot_composed_scene_for_export(&composed, cmd)?;
            let mut output_file = File::create(output)
                .map_err(|error| format!("failed to create {}: {error}", output.display()))?;
            mdl::write_composed_scene_obj(&mut output_file, &composed)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))
        }
        other => Err(format!(
            "unsupported obj conversion input format: {other}; expected .mdl or .utc"
        )),
    }
}

fn run_convert_model(cmd: &ConvertCmd) -> Result<(), String> {
    let output = require_convert_output(cmd)?;
    let input_ext = output_extension(&cmd.input)?;
    let output_ext = output_extension(output)?;
    if input_ext != "mdl" || output_ext != "mdl" {
        return Err(format!(
            "mdl conversion requires both input and output to end in .mdl: {} -> {}",
            cmd.input.display(),
            output.display()
        ));
    }

    if output_ext == "obj" {
        return Err("mdl conversion to obj should have been handled earlier".to_string());
    }

    let parsed = mdl::ParsedModel::from_file(&cmd.input)
        .map_err(|error| format!("failed to parse {} as MDL: {error}", cmd.input.display()))?;
    let mut output_file = File::create(output)
        .map_err(|error| format!("failed to create {}: {error}", output.display()))?;

    match parsed {
        mdl::ParsedModel::Ascii(model) => {
            let compiled = mdl::compile_ascii_model(&model).map_err(|error| {
                format!(
                    "failed to rebuild compiled MDL from {}: {error}",
                    cmd.input.display()
                )
            })?;
            mdl::write_model(&mut output_file, &compiled)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))
        }
        mdl::ParsedModel::Compiled(model) => {
            let ascii = mdl::lower_binary_model_to_ascii(&model).map_err(|error| {
                format!(
                    "failed to lower compiled MDL {} to ascii: {error}",
                    cmd.input.display()
                )
            })?;
            mdl::write_ascii_model(&mut output_file, &ascii)
                .map_err(|error| format!("failed to write {}: {error}", output.display()))
        }
    }
}

fn read_input_image(path: &Path) -> Result<DecodedImage, String> {
    let extension = output_extension(path)?;
    match extension.as_str() {
        "dds" => {
            let dds = dds::DdsTexture::from_file(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let rgba = dds
                .decode_rgba8()
                .map_err(|error| format!("failed to decode {}: {error}", path.display()))?;
            Ok(DecodedImage {
                width: dds.width,
                height: dds.height,
                rgba,
            })
        }
        "tga" => {
            let tga = tga::TgaTexture::from_file(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let rgba = tga
                .decode_rgba8()
                .map_err(|error| format!("failed to decode {}: {error}", path.display()))?;
            Ok(DecodedImage {
                width: u32::from(tga.width),
                height: u32::from(tga.height),
                rgba,
            })
        }
        _ => {
            let image = image::open(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?
                .to_rgba8();
            Ok(DecodedImage {
                width:  image.width(),
                height: image.height(),
                rgba:   image.into_raw(),
            })
        }
    }
}

fn write_output_dds(
    path: &Path,
    decoded: &DecodedImage,
    format: dds::DdsFormat,
) -> Result<(), String> {
    let dds = dds::DdsTexture::encode_rgba8(decoded.width, decoded.height, format, &decoded.rgba)
        .map_err(|error| format!("failed to encode dds for {}: {error}", path.display()))?;
    let mut file = File::create(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    dds::write_dds(&mut file, &dds)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn write_output_tga(path: &Path, decoded: &DecodedImage) -> Result<(), String> {
    let width = u16::try_from(decoded.width).map_err(|error| {
        format!(
            "image width does not fit TGA limits for {}: {error}",
            path.display()
        )
    })?;
    let height = u16::try_from(decoded.height).map_err(|error| {
        format!(
            "image height does not fit TGA limits for {}: {error}",
            path.display()
        )
    })?;
    let tga = tga::TgaTexture::encode_rgba8(width, height, &decoded.rgba)
        .map_err(|error| format!("failed to encode tga for {}: {error}", path.display()))?;
    let mut file = File::create(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    tga::write_tga(&mut file, &tga)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn write_output_webp(path: &Path, decoded: &DecodedImage) -> Result<(), String> {
    let image = RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba.clone())
        .ok_or_else(|| "decoded image buffer length does not match dimensions".to_string())?;
    let dynamic = DynamicImage::ImageRgba8(image);
    let mut file = File::create(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    dynamic
        .write_to(&mut file, ImageFormat::WebP)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn parse_dds_format(value: &str) -> Result<dds::DdsFormat, String> {
    match value.to_ascii_lowercase().as_str() {
        "dxt1" => Ok(dds::DdsFormat::Dxt1),
        "dxt5" => Ok(dds::DdsFormat::Dxt5),
        _ => Err(format!("unsupported dds format: {value}")),
    }
}

fn output_extension(path: &Path) -> Result<String, String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("failed to infer format from {}", path.display()))
}

fn read_gff_root_from_file(path: &Path) -> Result<gff::GffRoot, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    gff::read_gff_root(&mut Cursor::new(bytes))
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn build_install_resman(cmd: &ConvertCmd) -> Result<resman::ResMan, String> {
    let root = if let Some(root) = &cmd.root {
        root.clone()
    } else {
        install::find_nwnrs_root("")
            .map_err(|error| format!("failed to autodetect install root: {error}"))?
    };
    let user = if let Some(user) = &cmd.user {
        user.clone()
    } else {
        install::find_user_root("")
            .map_err(|error| format!("failed to autodetect user root: {error}"))?
    };
    install::new_default_resman(
        &root,
        &user,
        &cmd.language,
        0,
        true,
        cmd.load_ovr,
        &[],
        &[],
        &[],
        &[],
    )
    .map_err(|error| install_context_error(&root, &user, error.to_string()))
}

fn install_context_error(root: &Path, user: &Path, message: String) -> String {
    format!(
        "failed to build install resource manager (root={}, user={}): {message}",
        root.display(),
        user.display()
    )
}

fn require_convert_output(cmd: &ConvertCmd) -> Result<&Path, String> {
    cmd.output
        .as_deref()
        .ok_or_else(|| "convert requires OUTPUT unless --list-animations is used".to_string())
}

fn ensure_animation_options_unused(cmd: &ConvertCmd) -> Result<(), String> {
    if cmd.animation.is_some() || cmd.time.is_some() {
        return Err(
            "animation options are only supported for obj export; use OUTPUT ending in .obj"
                .to_string(),
        );
    }
    Ok(())
}

fn run_list_animations(cmd: &ConvertCmd) -> Result<(), String> {
    let input_ext = output_extension(&cmd.input)?;
    let animations = match input_ext.as_str() {
        "mdl" => {
            let scene = mdl::NwnScene::from_auto_file(&cmd.input).map_err(|error| {
                format!(
                    "failed to parse {} as scene MDL: {error}",
                    cmd.input.display()
                )
            })?;
            mdl::scene_animation_names(&scene)
        }
        "utc" => {
            let root = read_gff_root_from_file(&cmd.input)?;
            let mut resman = build_install_resman(cmd)?;
            let composed =
                mdl::compose_player_creature_from_utc(&mut resman, &root).map_err(|error| {
                    format!(
                        "failed to compose equipped creature from {}: {error}",
                        cmd.input.display()
                    )
                })?;
            mdl::composed_scene_animation_names(&composed)
        }
        other => {
            return Err(format!(
                "animation listing is only supported for .mdl or .utc input, got {other}"
            ));
        }
    };

    let mut stdout = std::io::stdout().lock();
    if animations.is_empty() {
        writeln!(stdout, "none").map_err(|error| format!("failed to write stdout: {error}"))?;
    } else {
        for animation in animations {
            writeln!(stdout, "{animation}")
                .map_err(|error| format!("failed to write stdout: {error}"))?;
        }
    }
    Ok(())
}

fn snapshot_scene_for_export(
    scene: &mdl::NwnScene,
    cmd: &ConvertCmd,
) -> Result<mdl::NwnScene, String> {
    match (&cmd.animation, cmd.time) {
        (Some(name), time) => mdl::sample_scene_animation(scene, name, time.unwrap_or(0.0))
            .map_err(|error| error.to_string()),
        (None, Some(time)) => {
            mdl::sample_scene_default_animation(scene, time).map_err(|error| error.to_string())
        }
        (None, None) => Ok(scene.clone()),
    }
}

fn snapshot_composed_scene_for_export(
    scene: &mdl::NwnComposedScene,
    cmd: &ConvertCmd,
) -> Result<mdl::NwnComposedScene, String> {
    match (&cmd.animation, cmd.time) {
        (Some(name), time) => {
            mdl::sample_composed_scene_animation(scene, name, time.unwrap_or(0.0))
                .map_err(|error| error.to_string())
        }
        (None, Some(time)) => mdl::sample_composed_scene_default_animation(scene, time)
            .map_err(|error| error.to_string()),
        (None, None) => Ok(scene.clone()),
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
    use nwnrs::{
        prelude::{dds, mdl, tga},
        resman::CachePolicy,
    };
    use nwnrs_test_support::{
        demand_resource, materialize_bytes_to_temp_file, materialize_resource_to_temp_file,
        require_game_resource, skip_if_game_resources_unavailable,
    };

    use super::run_convert;
    use crate::args::ConvertCmd;

    fn base_convert_cmd(input: PathBuf, output: PathBuf) -> ConvertCmd {
        ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            root: None,
            user: None,
            language: String::from("english"),
            load_ovr: false,
            animation: None,
            time: None,
            list_animations: false,
            input,
            output: Some(output),
        }
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("nwnrs-cli-{label}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_png_fixture(path: &Path) {
        let image = RgbaImage::from_raw(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
            ],
        )
        .expect("construct png fixture");
        DynamicImage::ImageRgba8(image)
            .save_with_format(path, ImageFormat::Png)
            .expect("write png fixture");
    }

    fn write_jpeg_fixture(path: &Path) {
        let image = RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
            .expect("construct jpeg fixture");
        DynamicImage::ImageRgb8(image)
            .save_with_format(path, ImageFormat::Jpeg)
            .expect("write jpeg fixture");
    }

    fn write_tga_fixture(path: &Path) {
        let tga = tga::TgaTexture::encode_rgba8(16, 16, &vec![255; 16_usize * 16 * 4])
            .expect("encode tga fixture");
        let mut file = fs::File::create(path).expect("create tga fixture");
        tga::write_tga(&mut file, &tga).expect("write tga fixture");
    }

    fn write_dds_fixture(path: &Path) {
        let dds = dds::DdsTexture::encode_rgba8(
            16,
            16,
            dds::DdsFormat::Dxt5,
            &vec![255; 16_usize * 16 * 4],
        )
        .expect("encode dds fixture");
        let mut file = fs::File::create(path).expect("create dds fixture");
        dds::write_dds(&mut file, &dds).expect("write dds fixture");
    }

    #[test]
    fn convert_supports_tga_to_dds() {
        let temp_dir = unique_test_dir("convert-tga-to-dds");
        let input = temp_dir.join("amp01_g06.tga");
        let output = temp_dir.join("amp01_g06.dds");
        write_tga_fixture(&input);

        run_convert(&base_convert_cmd(input, output.clone())).expect("convert tga to dds");

        let dds = dds::DdsTexture::from_file(&output).expect("read converted dds");
        assert_eq!(dds.width, 16);
        assert_eq!(dds.height, 16);
    }

    #[test]
    fn convert_supports_dds_to_webp() {
        let temp_dir = unique_test_dir("convert-dds-to-webp");
        let input = temp_dir.join("ashlw_066.dds");
        let output = temp_dir.join("ashlw_066.webp");
        write_dds_fixture(&input);

        run_convert(&base_convert_cmd(input, output.clone())).expect("convert dds to webp");

        let image = image::open(&output).expect("read converted webp");
        assert_eq!(image.width(), 16);
        assert_eq!(image.height(), 16);
    }

    #[test]
    fn convert_supports_png_to_tga() {
        let temp_dir = unique_test_dir("convert-png-to-tga");
        let input = temp_dir.join("input.png");
        let output = temp_dir.join("output.tga");
        write_png_fixture(&input);

        run_convert(&base_convert_cmd(input, output.clone())).expect("convert png to tga");

        let tga = tga::TgaTexture::from_file(&output).expect("read converted tga");
        assert_eq!(tga.width, 2);
        assert_eq!(tga.height, 2);
        assert_eq!(tga.pixel_depth, 32);
    }

    #[test]
    fn convert_supports_tga_to_webp() {
        let temp_dir = unique_test_dir("convert-tga-to-webp");
        let input = temp_dir.join("amp01_g06.tga");
        let output = temp_dir.join("amp01_g06.webp");
        write_tga_fixture(&input);

        run_convert(&base_convert_cmd(input, output.clone())).expect("convert tga to webp");

        let image = image::open(&output).expect("read converted webp");
        assert_eq!(image.width(), 16);
        assert_eq!(image.height(), 16);
    }

    #[test]
    fn convert_supports_jpeg_to_dds() {
        let temp_dir = unique_test_dir("convert-jpeg-to-dds");
        let input = temp_dir.join("input.jpg");
        let output = temp_dir.join("output.dds");
        write_jpeg_fixture(&input);

        let mut cmd = base_convert_cmd(input, output.clone());
        cmd.dds_format = String::from("dxt1");
        run_convert(&cmd).expect("convert jpeg to dds");

        let dds = dds::DdsTexture::from_file(&output).expect("read converted dds");
        assert_eq!(dds.width, 2);
        assert_eq!(dds.height, 2);
        assert_eq!(dds.format, dds::DdsFormat::Dxt1);
    }

    #[test]
    fn convert_supports_jpeg_to_webp() {
        let temp_dir = unique_test_dir("convert-jpeg-to-webp");
        let input = temp_dir.join("input.jpg");
        let output = temp_dir.join("output.webp");
        write_jpeg_fixture(&input);

        run_convert(&base_convert_cmd(input, output.clone())).expect("convert jpeg to webp");

        let image = image::open(&output).expect("read converted webp");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 2);
    }

    #[test]
    fn convert_supports_compiled_mdl_to_ascii() -> Result<(), Box<dyn Error>> {
        let input = match compiled_mdl_fixture_path() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let output = unique_test_dir("convert-compiled-mdl-to-ascii").join("a_ba2_ascii.mdl");

        run_convert(&base_convert_cmd(input, output.clone()))
            .expect("convert compiled mdl to ascii");

        let ascii = mdl::AsciiModel::from_file(&output).expect("read canonical ascii mdl");
        assert_eq!(ascii.geometry_name, "a_ba2");
        assert!(ascii.to_text().contains("# nwnrs-compiled-source begin"));
        Ok(())
    }

    #[test]
    fn convert_supports_ascii_mdl_to_compiled() -> Result<(), Box<dyn Error>> {
        let input = match canonical_ascii_mdl_fixture_path() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let output = unique_test_dir("convert-ascii-mdl-to-compiled").join("a_ba2_compiled.mdl");

        run_convert(&base_convert_cmd(input, output.clone()))
            .expect("convert ascii mdl to compiled");

        let compiled = mdl::BinaryModel::from_file(&output).expect("read compiled mdl");
        assert_eq!(compiled.name, "a_ba2");
        assert_eq!(compiled.animations.len(), 20);
        Ok(())
    }

    fn compiled_mdl_fixture_path() -> Result<PathBuf, Box<dyn Error>> {
        require_game_resource(materialize_resource_to_temp_file(
            "a_ba2",
            mdl::MODEL_RES_TYPE,
        ))
    }

    fn canonical_ascii_mdl_fixture_path() -> Result<PathBuf, Box<dyn Error>> {
        let res = require_game_resource(demand_resource("a_ba2", mdl::MODEL_RES_TYPE))?;
        let binary = mdl::BinaryModel::from_res(&res, CachePolicy::Use)?;
        let ascii = mdl::lower_binary_model_to_ascii(&binary)?;
        Ok(materialize_bytes_to_temp_file(
            &ascii.to_text().into_bytes(),
            "a_ba2_ascii.mdl",
        )?)
    }

    #[test]
    fn convert_rejects_unknown_animation_name_and_preserves_output_path()
    -> Result<(), Box<dyn Error>> {
        let input = match compiled_mdl_fixture_path() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let scene = mdl::NwnScene::from_auto_file(&input)?;
        let available = mdl::scene_animation_names(&scene);
        assert!(!available.is_empty(), "fixture should expose animations");

        let output = unique_test_dir("convert-invalid-animation").join("a_ba2.obj");
        let mut cmd = base_convert_cmd(input, output.clone());
        cmd.animation = Some("definitely-not-real".to_string());
        let error = run_convert(&cmd).expect_err("invalid animation should fail");

        assert!(error.contains("available animations:"), "{error}");
        let expected = available
            .first()
            .unwrap_or_else(|| panic!("fixture should expose at least one animation"));
        assert!(error.contains(expected), "{error}");
        assert!(!output.exists(), "bad animation should not create output");
        Ok(())
    }

    #[test]
    fn snapshot_without_explicit_animation_reports_available_names() {
        let scene = scene_with_named_animations(&["walk", "run"]);
        let cmd = ConvertCmd {
            animation: None,
            time: Some(0.0),
            ..base_convert_cmd(PathBuf::from("input.mdl"), PathBuf::from("output.obj"))
        };

        let error = super::snapshot_scene_for_export(&scene, &cmd)
            .expect_err("time without explicit/default animation should fail");

        assert!(error.contains("available animations: walk, run"), "{error}");
    }

    fn scene_with_named_animations(names: &[&str]) -> mdl::NwnScene {
        mdl::NwnScene {
            name:              "demo".to_string(),
            supermodel:        None,
            classification:    None,
            animation_scale:   None,
            coordinate_system: mdl::NwnCoordinateSystem::AuroraSource,
            nodes:             Vec::new(),
            meshes:            Vec::new(),
            materials:         Vec::new(),
            animations:        names
                .iter()
                .map(|name| mdl::NwnAnimation {
                    name:            (*name).to_string(),
                    model_name:      "demo".to_string(),
                    length:          1.0,
                    transition_time: 0.0,
                    root_name:       None,
                    root_node:       None,
                    events:          Vec::new(),
                    node_tracks:     Vec::new(),
                })
                .collect(),
            diagnostics:       Vec::new(),
        }
    }
}
