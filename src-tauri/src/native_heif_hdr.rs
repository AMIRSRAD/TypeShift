#[cfg(feature = "native-heif")]
use std::{fs, path::Path};

#[cfg(feature = "native-heif")]
use image::{ImageFormat, RgbImage};

#[cfg(feature = "native-heif")]
use libheif_rs::{
    AuxiliaryImagesFilter, ColorSpace, DecodingOptions, HeifContext, ImageHandle, LibHeif,
    RgbChroma,
};

#[cfg(feature = "native-heif")]
use crate::{
    hdr_jpeg_writer::{encode_gain_map_jpeg, GainMapJpegInput},
    image_engine::ConvertRequest,
};

#[cfg(feature = "native-heif")]
pub fn heif_aux_decoder_linked() -> bool {
    true
}

#[cfg(not(feature = "native-heif"))]
pub fn heif_aux_decoder_linked() -> bool {
    false
}

#[cfg(feature = "native-heif")]
pub fn convert_heic_to_hdr_jpeg(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertRequest,
) -> Result<Vec<String>, String> {
    let lib_heif = LibHeif::new_checked()
        .map_err(|error| format!("Could not initialize native libheif backend: {error}"))?;
    let input_path_string = input_path.to_string_lossy();
    let context = HeifContext::read_from_file(&input_path_string)
        .map_err(|error| format!("Could not read HEIC with native libheif backend: {error}"))?;
    let primary_handle = context
        .primary_image_handle()
        .map_err(|error| format!("Could not read primary HEIC image handle: {error}"))?;
    let gain_map_handle = find_hdr_gain_map_handle(&primary_handle)?;

    let primary_rgb = decode_primary_rgb(&lib_heif, &primary_handle)?;
    let mut gain_map_gray = decode_gain_map_gray(&lib_heif, &gain_map_handle)?;
    let gain_map_tone = normalize_apple_gain_map(&mut gain_map_gray.data);
    let hdr_headroom = apple_hdr_gain_map_headroom(input_path).unwrap_or(3.0);
    let jpeg = encode_gain_map_jpeg(GainMapJpegInput {
        width: primary_rgb.width,
        height: primary_rgb.height,
        primary_rgb: primary_rgb.data,
        gain_map_width: gain_map_gray.width,
        gain_map_height: gain_map_gray.height,
        gain_map_gray: gain_map_gray.data,
        primary_quality: request.jpeg_quality.clamp(1, 100),
        gain_map_quality: request.jpeg_quality.clamp(1, 100).saturating_sub(5).max(70),
        hdr_headroom,
    })?;

    fs::write(output_path, jpeg)
        .map_err(|error| format!("Could not write HDR JPEG {}: {error}", output_path.display()))?;

    Ok(vec![
        "HEIC primary image and HDR gain-map auxiliary image were decoded through native libheif."
            .to_string(),
        format!(
            "Apple HDR gain-map baseline was normalized from p75={} to p995={} so HDR boost targets highlights instead of the whole frame.",
            gain_map_tone.neutral_floor, gain_map_tone.highlight_ceiling
        ),
        "HDR JPEG was encoded with ISO 21496 / Ultra HDR gain-map metadata.".to_string(),
        format!("HDR gain-map headroom was set to {hdr_headroom:.3}x."),
        "Apple-specific gain-map metadata mapping is still conservative; validate output in iOS Photos before treating it as final parity."
            .to_string(),
    ])
}

#[cfg(feature = "native-heif")]
pub fn write_heic_preview_png(input_path: &Path, output_path: &Path) -> Result<(), String> {
    let lib_heif = LibHeif::new_checked()
        .map_err(|error| format!("Could not initialize native libheif preview backend: {error}"))?;
    let input_path_string = input_path.to_string_lossy();
    let context = HeifContext::read_from_file(&input_path_string)
        .map_err(|error| format!("Could not read HEIC preview with native libheif backend: {error}"))?;
    let primary_handle = context
        .primary_image_handle()
        .map_err(|error| format!("Could not read HEIC preview primary image handle: {error}"))?;
    let primary_rgb = decode_primary_rgb(&lib_heif, &primary_handle)?;
    let preview = RgbImage::from_raw(primary_rgb.width, primary_rgb.height, primary_rgb.data)
        .ok_or_else(|| "Decoded HEIC preview pixel buffer did not match image dimensions.".to_string())?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create HEIC preview cache folder: {error}"))?;
    }

    preview
        .save_with_format(output_path, ImageFormat::Png)
        .map_err(|error| format!("Could not write HEIC preview PNG: {error}"))
}

#[cfg(not(feature = "native-heif"))]
pub fn write_heic_preview_png(
    _input_path: &std::path::Path,
    _output_path: &std::path::Path,
) -> Result<(), String> {
    Err("Native libheif preview backend is not linked in this build.".to_string())
}

#[cfg(feature = "native-heif")]
#[derive(Debug, Clone, Copy)]
struct GainMapToneStats {
    neutral_floor: u8,
    highlight_ceiling: u8,
}

#[cfg(not(feature = "native-heif"))]
pub fn convert_heic_to_hdr_jpeg(
    _input_path: &std::path::Path,
    _output_path: &std::path::Path,
    _request: &crate::image_engine::ConvertRequest,
) -> Result<Vec<String>, String> {
    Err("Native libheif auxiliary-image decoder is not linked in this build. Build with `cargo build --release --features native-heif` to enable HEIC primary/gain-map decoding.".to_string())
}

#[cfg(feature = "native-heif")]
struct DecodedBuffer {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[cfg(feature = "native-heif")]
fn find_hdr_gain_map_handle(primary_handle: &ImageHandle) -> Result<ImageHandle, String> {
    let filter = AuxiliaryImagesFilter::new();
    let auxiliary_images = primary_handle.auxiliary_images(filter);
    for handle in auxiliary_images {
        let auxiliary_type = handle.auxiliary_type().unwrap_or_default();
        if is_hdr_gain_map_auxiliary_type(&auxiliary_type) {
            return Ok(handle);
        }
    }

    Err("No decodable HEIC HDR gain-map auxiliary image handle was found.".to_string())
}

#[cfg(feature = "native-heif")]
fn decode_primary_rgb(lib_heif: &LibHeif, handle: &ImageHandle) -> Result<DecodedBuffer, String> {
    let mut options = DecodingOptions::new()
        .ok_or_else(|| "Could not allocate libheif decoding options.".to_string())?;
    options.set_convert_hdr_to_8bit(true);
    let image = lib_heif
        .decode(handle, ColorSpace::Rgb(RgbChroma::Rgb), Some(options))
        .map_err(|error| format!("Could not decode HEIC primary RGB pixels: {error}"))?;
    let plane = image
        .planes()
        .interleaved
        .ok_or_else(|| "Decoded HEIC primary image did not expose an interleaved RGB plane.".to_string())?;

    copy_interleaved_rows(&plane.data, plane.width, plane.height, plane.stride, 3)
}

#[cfg(feature = "native-heif")]
fn decode_gain_map_gray(lib_heif: &LibHeif, handle: &ImageHandle) -> Result<DecodedBuffer, String> {
    let mut options = DecodingOptions::new()
        .ok_or_else(|| "Could not allocate libheif decoding options.".to_string())?;
    options.set_convert_hdr_to_8bit(true);
    let image = lib_heif
        .decode(handle, ColorSpace::Monochrome, Some(options))
        .map_err(|error| format!("Could not decode HEIC HDR gain-map pixels: {error}"))?;
    let planes = image.planes();
    if let Some(plane) = planes.y {
        return copy_interleaved_rows(&plane.data, plane.width, plane.height, plane.stride, 1);
    }
    if let Some(plane) = planes.interleaved {
        return copy_interleaved_rows(&plane.data, plane.width, plane.height, plane.stride, 1);
    }

    Err("Decoded HEIC gain map did not expose a grayscale plane.".to_string())
}

#[cfg(feature = "native-heif")]
fn copy_interleaved_rows(
    bytes: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    bytes_per_pixel: usize,
) -> Result<DecodedBuffer, String> {
    let row_len = width as usize * bytes_per_pixel;
    if width == 0 || height == 0 {
        return Err("Decoded HEIC plane dimensions were zero.".to_string());
    }
    if stride < row_len {
        return Err(format!(
            "Decoded HEIC plane stride {stride} is smaller than expected row length {row_len}."
        ));
    }
    let expected_total = stride
        .checked_mul(height as usize)
        .ok_or_else(|| "Decoded HEIC plane size overflowed.".to_string())?;
    if bytes.len() < expected_total {
        return Err(format!(
            "Decoded HEIC plane has {} bytes; expected at least {expected_total}.",
            bytes.len()
        ));
    }

    let mut data = Vec::with_capacity(row_len * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        data.extend_from_slice(&bytes[start..start + row_len]);
    }

    Ok(DecodedBuffer {
        width,
        height,
        data,
    })
}

#[cfg(feature = "native-heif")]
fn apple_hdr_gain_map_headroom(path: &Path) -> Option<f32> {
    let data = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&data);
    let tag = "HDRGainMapHeadroom";
    let index = text.find(tag)?;
    let after_tag = &text[index + tag.len()..];
    let value_start = after_tag.find('>')? + 1;
    let value_text = after_tag[value_start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '+' | '-'))
        .collect::<String>();
    let value = value_text.parse::<f32>().ok()?;
    value.is_finite().then_some(value).filter(|value| *value > 1.0)
}

#[cfg(feature = "native-heif")]
fn normalize_apple_gain_map(data: &mut [u8]) -> GainMapToneStats {
    if data.is_empty() {
        return GainMapToneStats {
            neutral_floor: 0,
            highlight_ceiling: 255,
        };
    }

    let mut sorted = data.to_vec();
    sorted.sort_unstable();
    let floor = percentile_u8(&sorted, 0.75);
    let mut ceiling = percentile_u8(&sorted, 0.995);
    if ceiling <= floor {
        ceiling = sorted[sorted.len() - 1].max(floor.saturating_add(1));
    }
    let range = (ceiling.saturating_sub(floor)).max(1) as f32;

    for value in data {
        if *value <= floor {
            *value = 0;
            continue;
        }

        let normalized = ((*value - floor) as f32 / range).clamp(0.0, 1.0);
        let highlight_weight = normalized.powf(1.2);
        *value = (highlight_weight * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    GainMapToneStats {
        neutral_floor: floor,
        highlight_ceiling: ceiling,
    }
}

#[cfg(feature = "native-heif")]
fn percentile_u8(sorted: &[u8], percentile: f32) -> u8 {
    let index = ((sorted.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    sorted[index]
}

#[cfg(feature = "native-heif")]
fn is_hdr_gain_map_auxiliary_type(auxiliary_type: &str) -> bool {
    let normalized = auxiliary_type.to_ascii_lowercase();
    normalized.contains("hdrgainmap")
        || normalized.contains("hdr.gain")
        || normalized.contains("gainmap")
        || normalized.contains("gain-map")
        || normalized.contains("iso:ts:21496")
        || normalized.contains("urn:iso:std:iso:ts:21496")
}

#[cfg(all(test, feature = "native-heif"))]
mod tests {
    use super::*;

    #[test]
    fn detects_hdr_gain_map_auxiliary_types() {
        assert!(is_hdr_gain_map_auxiliary_type(
            "urn:com:apple:photo:2020:aux:hdrgainmap"
        ));
        assert!(is_hdr_gain_map_auxiliary_type("urn:iso:std:iso:ts:21496"));
        assert!(!is_hdr_gain_map_auxiliary_type(
            "urn:com:apple:photo:2020:aux:semanticskymatte"
        ));
    }

    #[test]
    fn copies_strided_rows() {
        let decoded = copy_interleaved_rows(&[1, 2, 3, 9, 4, 5, 6, 9], 1, 2, 4, 3).unwrap();
        assert_eq!(decoded.data, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn parses_apple_hdr_gain_map_headroom_from_xmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.heic");
        fs::write(
            &path,
            br#"<HDRGainMap:HDRGainMapHeadroom>3.192761</HDRGainMap:HDRGainMapHeadroom>"#,
        )
        .unwrap();

        let value = apple_hdr_gain_map_headroom(&path).unwrap();
        assert!((value - 3.192761).abs() < 0.0001);
    }

    #[test]
    fn normalizes_raised_apple_gain_map_baseline() {
        let mut data = vec![69, 75, 82, 86, 95, 118, 146, 164, 199, 255, 255];
        let stats = normalize_apple_gain_map(&mut data);

        assert!(stats.neutral_floor >= 146);
        assert_eq!(data[0], 0);
        assert_eq!(data[5], 0);
        assert_eq!(data[6], 0);
        assert_eq!(*data.last().unwrap(), 255);
    }
}
