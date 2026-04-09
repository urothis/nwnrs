use std::{ffi::OsStr, fs::File, path::Path};

use image::{DynamicImage, ImageFormat, RgbaImage};
use nwnrs::prelude::*;
use tracing::info;

use crate::{args::ConvertCmd, util::ensure_output_file_ready};

struct DecodedImage {
    width:  u32,
    height: u32,
    rgba:   Vec<u8>,
}

pub(crate) fn run_convert(cmd: ConvertCmd) -> Result<(), String> {
    info!("converting image");
    ensure_output_file_ready(&cmd.output, cmd.force)?;

    let decoded = read_input_image(&cmd.input)?;
    let output_ext = output_extension(&cmd.output)?;
    match output_ext.as_str() {
        "dds" => write_output_dds(&cmd.output, &decoded, parse_dds_format(&cmd.dds_format)?)?,
        "tga" => write_output_tga(&cmd.output, &decoded)?,
        "webp" => write_output_webp(&cmd.output, &decoded)?,
        other => {
            return Err(format!("unsupported convert output format: {}", other));
        }
    }

    Ok(())
}

fn read_input_image(path: &Path) -> Result<DecodedImage, String> {
    let extension = output_extension(path)?;
    match extension.as_str() {
        "dds" => {
            let dds = dds::read_dds_from_file(path)
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
            let tga = tga::read_tga_from_file(path)
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
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or_else(|| format!("failed to infer format from {}", path.display()))
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
    use nwnrs::prelude::{dds, tga};

    use super::run_convert;
    use crate::args::ConvertCmd;

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

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            input,
            output: output.clone(),
        })
        .expect("convert tga to dds");

        let dds = dds::read_dds_from_file(&output).expect("read converted dds");
        assert_eq!(dds.width, 16);
        assert_eq!(dds.height, 16);
    }

    #[test]
    fn convert_supports_dds_to_webp() {
        let temp_dir = unique_test_dir("convert-dds-to-webp");
        let input = temp_dir.join("ashlw_066.dds");
        let output = temp_dir.join("ashlw_066.webp");
        write_dds_fixture(&input);

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            input,
            output: output.clone(),
        })
        .expect("convert dds to webp");

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

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            input,
            output: output.clone(),
        })
        .expect("convert png to tga");

        let tga = tga::read_tga_from_file(&output).expect("read converted tga");
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

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            input,
            output: output.clone(),
        })
        .expect("convert tga to webp");

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

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt1"),
            input,
            output: output.clone(),
        })
        .expect("convert jpeg to dds");

        let dds = dds::read_dds_from_file(&output).expect("read converted dds");
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

        run_convert(ConvertCmd {
            force: true,
            dds_format: String::from("dxt5"),
            input,
            output: output.clone(),
        })
        .expect("convert jpeg to webp");

        let image = image::open(&output).expect("read converted webp");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 2);
    }
}
