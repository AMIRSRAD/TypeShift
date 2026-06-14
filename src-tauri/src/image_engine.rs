use image::{GenericImageView, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    fmt,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[cfg(target_os = "windows")]
#[path = "windows_wic.rs"]
mod windows_wic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageInspection {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub dimensions: Option<Dimensions>,
    pub bit_depth: Option<u8>,
    pub color_profile_name: Option<String>,
    pub color_profile_type: Option<String>,
    pub orientation: String,
    pub has_alpha: Option<bool>,
    pub has_hdr: bool,
    pub has_gain_map: bool,
    pub metadata_summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Png,
    Jpeg,
    AdaptiveHdrJpeg,
    Webp,
    Tiff,
    Bmp,
}

impl OutputFormat {
    fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg | Self::AdaptiveHdrJpeg => "jpg",
            Self::Webp => "webp",
            Self::Tiff => "tiff",
            Self::Bmp => "bmp",
        }
    }

    fn output_tag(&self) -> &'static str {
        match self {
            Self::AdaptiveHdrJpeg => "typeshift-hdr",
            _ => "typeshift",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    VisualMatch,
    SrgbCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPolicy {
    PreserveSafe,
    PreserveAll,
    StripExceptColor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryDataPolicy {
    Discard,
    PreserveIfSupported,
    ExtractSidecars,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DestinationPolicy {
    SameFolder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    AutoNumber,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HdrPolicy {
    ToneMapSdr,
    PreserveHdr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConvertRequest {
    pub input_paths: Vec<String>,
    pub output_format: OutputFormat,
    pub preset: Preset,
    pub metadata_policy: MetadataPolicy,
    pub auxiliary_data_policy: AuxiliaryDataPolicy,
    pub destination_policy: DestinationPolicy,
    pub conflict_policy: ConflictPolicy,
    pub jpeg_quality: u8,
    pub png_bit_depth: u8,
    pub hdr_policy: HdrPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Completed,
    Failed,
    Cancelled,
}

impl ResultStatus {
    pub fn is_terminal_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversionResult {
    pub input_path: String,
    pub output_path: Option<String>,
    pub sidecar_outputs: Vec<ConversionSidecar>,
    pub status: ResultStatus,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
    pub source_profile_summary: Option<String>,
    pub output_profile_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversionSidecar {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversionJob {
    pub job_id: String,
    pub status: JobStatus,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<ConversionResult>,
}

pub type ConversionJobStatus = ConversionJob;

#[derive(Debug)]
pub enum ImageEngineError {
    Io(std::io::Error),
    Decode(image::ImageError),
}

impl fmt::Display for ImageEngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Decode(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for ImageEngineError {}

impl From<std::io::Error> for ImageEngineError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<image::ImageError> for ImageEngineError {
    fn from(value: image::ImageError) -> Self {
        Self::Decode(value)
    }
}

pub fn inspect_image(path: &Path) -> Result<ImageInspection, ImageEngineError> {
    let format = detect_format(path);
    let mut warnings = Vec::new();
    let mut has_hdr = false;
    let mut has_gain_map = false;

    let (dimensions, bit_depth, has_alpha, color_profile_name, color_profile_type) = if is_heif_like(path) {
        let auxiliary_inspection = crate::heif_auxiliary::inspect_auxiliary_items(path);
        has_gain_map = auxiliary_inspection.has_hdr_gain_map;
        has_hdr = has_gain_map;
        warnings.extend(auxiliary_inspection.warnings);

        match inspect_heif(path) {
            Ok(heif) => {
                warnings.extend(heif.warnings);
                (
                    heif.dimensions,
                    heif.bit_depth,
                    Some(heif.has_alpha),
                    heif.color_profile_name,
                    heif.color_profile_type,
                )
            }
            Err(error) => {
                warnings.push(error);
                (None, None, None, None, None)
            }
        }
    } else if let Some(image_format) = image_format_for_path(path) {
        let reader = ImageReader::open(path)?.with_guessed_format()?;
        let decoded = reader.decode()?;
        let dimensions = decoded.dimensions();
        let has_alpha = decoded.color().has_alpha();
        let bit_depth =
            Some((decoded.color().bits_per_pixel() / u16::from(decoded.color().channel_count())) as u8);
        let expected_format = ImageFormat::from_path(path).ok();
        if expected_format != Some(image_format) {
            warnings.push("File extension and detected image format differ.".to_string());
        }
        if is_gif_like(path) {
            warnings.push("GIF inspection uses the first decoded frame; animation preservation is not supported yet.".to_string());
        }
        (
            Some(Dimensions {
                width: dimensions.0,
                height: dimensions.1,
            }),
            bit_depth,
            Some(has_alpha),
            None,
            None,
        )
    } else {
        warnings.push(format!("{format} is not supported in the v1 image pipeline.").to_string());
        (None, None, None, None, None)
    };

    Ok(ImageInspection {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string(),
        format,
        dimensions,
        bit_depth,
        color_profile_name,
        color_profile_type,
        orientation: "Pending decode".to_string(),
        has_alpha,
        has_hdr,
        has_gain_map,
        metadata_summary: "Safe metadata policy selected; GPS will be stripped on conversion.".to_string(),
        warnings,
    })
}

pub fn convert_image(input_path: &Path, request: &ConvertRequest) -> ConversionResult {
    let output_path = safe_output_path(input_path, &request.output_format);
    let mut warnings = Vec::new();
    let mut source_has_gain_map = false;

    if is_heif_like(input_path) {
        let auxiliary_inspection = crate::heif_auxiliary::inspect_auxiliary_items(input_path);
        source_has_gain_map = auxiliary_inspection.has_hdr_gain_map;
        warnings.extend(auxiliary_inspection.warnings);
    }

    warnings.extend(hdr_warnings(input_path, request, source_has_gain_map));
    warnings.extend(auxiliary_data_warnings(input_path, request));
    if let Some(error) = hdr_blocking_error(request, source_has_gain_map) {
        return failed_result(input_path, Some(output_path), warnings, &error);
    }

    if matches!(request.output_format, OutputFormat::AdaptiveHdrJpeg) {
        match convert_adaptive_hdr_jpeg(input_path, &output_path, request, source_has_gain_map) {
            Ok(mut backend_warnings) => {
                warnings.append(&mut backend_warnings);
                let mut metadata_outcome = crate::metadata_writer::copy_metadata_after_conversion(
                    input_path,
                    &output_path,
                    &request.metadata_policy,
                );
                warnings.append(&mut metadata_outcome.warnings);
                return ConversionResult {
                    input_path: input_path.to_string_lossy().to_string(),
                    output_path: Some(output_path.to_string_lossy().to_string()),
                    sidecar_outputs: Vec::new(),
                    status: ResultStatus::Completed,
                    warnings,
                    error_message: None,
                    source_profile_summary: Some("HEIC HDR gain-map source".to_string()),
                    output_profile_summary: Some("ISO 21496 / Adaptive HDR JPEG".to_string()),
                };
            }
            Err(error) => return failed_result(input_path, Some(output_path), warnings, &error),
        }
    }

    if is_heif_like(input_path) {
        match convert_heif(input_path, &output_path, request) {
            Ok(mut backend_warnings) => {
                warnings.append(&mut backend_warnings);
                let mut sidecar_outputs = Vec::new();
                if matches!(request.output_format, OutputFormat::Jpeg) {
                    let mut metadata_outcome = crate::metadata_writer::copy_metadata_after_conversion(
                        input_path,
                        &output_path,
                        &request.metadata_policy,
                    );
                    warnings.append(&mut metadata_outcome.warnings);
                }
                if matches!(request.auxiliary_data_policy, AuxiliaryDataPolicy::ExtractSidecars) {
                    match crate::heif_auxiliary::extract_auxiliary_sidecars(input_path, &output_path) {
                        Ok((sidecars, mut sidecar_warnings)) => {
                            warnings.append(&mut sidecar_warnings);
                            let sidecar_count = sidecars.len();
                            sidecar_outputs = sidecars
                                .into_iter()
                                .map(|sidecar| ConversionSidecar {
                                    path: sidecar.path,
                                    kind: format!("{:?}", sidecar.kind),
                                    bytes: sidecar.bytes,
                                })
                                .collect();
                            if sidecar_count > 0 {
                                warnings.push(format!(
                                    "Extracted {} HEIC sidecar file(s). These preserve raw HEIC auxiliary/metadata payloads for validation; they are not Apple-compatible JPEG Portrait embedding yet.",
                                    sidecar_count
                                ));
                            }
                        }
                        Err(error) => warnings.push(format!("Could not extract HEIC auxiliary sidecars: {error}")),
                    }
                }
                return ConversionResult {
                    input_path: input_path.to_string_lossy().to_string(),
                    output_path: Some(output_path.to_string_lossy().to_string()),
                    sidecar_outputs,
                    status: ResultStatus::Completed,
                    warnings,
                    error_message: None,
                    source_profile_summary: Some("WIC source color context".to_string()),
                    output_profile_summary: Some(match request.preset {
                        Preset::VisualMatch => "WIC visual-match output with source color context".to_string(),
                        Preset::SrgbCompatible => "WIC sRGB-compatible output path".to_string(),
                    }),
                };
            }
            Err(error) => {
                warnings.push("HEIC/HEIF detected.".to_string());
                warnings.push("The final bundled libheif + LittleCMS backend is still pending.".to_string());
                return failed_result(input_path, Some(output_path), warnings, &error);
            }
        }
    }

    if is_gif_like(input_path) {
        warnings.push("Animated GIF conversion currently uses the first decoded frame only.".to_string());
    }

    match convert_supported_raster(input_path, &output_path, request) {
        Ok(profile_summary) => {
            if matches!(request.output_format, OutputFormat::Jpeg) {
                let mut metadata_outcome = crate::metadata_writer::copy_metadata_after_conversion(
                    input_path,
                    &output_path,
                    &request.metadata_policy,
                );
                warnings.append(&mut metadata_outcome.warnings);
            }

            ConversionResult {
                input_path: input_path.to_string_lossy().to_string(),
                output_path: Some(output_path.to_string_lossy().to_string()),
                sidecar_outputs: Vec::new(),
                status: ResultStatus::Completed,
                warnings,
                error_message: None,
                source_profile_summary: profile_summary.clone(),
                output_profile_summary: profile_summary,
            }
        }
        Err(error) => failed_result(input_path, Some(output_path), warnings, &error.to_string()),
    }
}

pub fn create_preview_image(input_path: &Path) -> Result<PathBuf, ImageEngineError> {
    let preview_path = preview_cache_path(input_path)?;
    if preview_path.exists() && fs::metadata(&preview_path).map(|metadata| metadata.len() > 0).unwrap_or(false) {
        return Ok(preview_path);
    }
    if preview_path.exists() {
        let _ = fs::remove_file(&preview_path);
    }

    if is_heif_like(input_path) {
        create_native_heif_preview(input_path, &preview_path).map_err(|error| {
            ImageEngineError::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
        })?;
        return Ok(preview_path);
    }

    let reader = ImageReader::open(input_path)?.with_guessed_format()?;
    let image = reader.decode()?;
    let preview = image.thumbnail(1600, 1600);
    preview.save_with_format(&preview_path, ImageFormat::Png)?;
    Ok(preview_path)
}

fn preview_cache_path(input_path: &Path) -> Result<PathBuf, ImageEngineError> {
    let mut hasher = DefaultHasher::new();
    input_path.hash(&mut hasher);
    if let Ok(metadata) = fs::metadata(input_path) {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                duration.as_nanos().hash(&mut hasher);
            }
        }
    }

    let cache_dir = std::env::temp_dir().join("typeshift-previews");
    fs::create_dir_all(&cache_dir)?;
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("preview")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    Ok(cache_dir.join(format!("{stem}-{:016x}.png", hasher.finish())))
}

fn convert_supported_raster(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertRequest,
) -> Result<Option<String>, ImageEngineError> {
    let reader = ImageReader::open(input_path)?.with_guessed_format()?;
    let image = reader.decode()?;

    match request.output_format {
        OutputFormat::Png => {
            image.save_with_format(output_path, ImageFormat::Png)?;
        }
        OutputFormat::Jpeg => {
            let rgb = image.to_rgb8();
            rgb.save_with_format(output_path, ImageFormat::Jpeg)?;
        }
        OutputFormat::AdaptiveHdrJpeg => {
            return Err(ImageEngineError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Adaptive HDR JPEG requires the HEIC gain-map backend",
            )));
        }
        OutputFormat::Webp => {
            image.save_with_format(output_path, ImageFormat::WebP)?;
        }
        OutputFormat::Tiff => {
            image.save_with_format(output_path, ImageFormat::Tiff)?;
        }
        OutputFormat::Bmp => {
            let rgb = image.to_rgb8();
            rgb.save_with_format(output_path, ImageFormat::Bmp)?;
        }
    }

    Ok(Some(match request.preset {
        Preset::VisualMatch => "Source profile preservation pending ICC backend".to_string(),
        Preset::SrgbCompatible => "sRGB conversion pending ICC backend".to_string(),
    }))
}

fn failed_result(
    input_path: &Path,
    output_path: Option<PathBuf>,
    warnings: Vec<String>,
    message: &str,
) -> ConversionResult {
    ConversionResult {
        input_path: input_path.to_string_lossy().to_string(),
        output_path: output_path.map(|path| path.to_string_lossy().to_string()),
        sidecar_outputs: Vec::new(),
        status: ResultStatus::Failed,
        warnings,
        error_message: Some(message.to_string()),
        source_profile_summary: None,
        output_profile_summary: None,
    }
}

fn auxiliary_data_warnings(input_path: &Path, request: &ConvertRequest) -> Vec<String> {
    if matches!(request.auxiliary_data_policy, AuxiliaryDataPolicy::Discard) {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    if is_heif_like(input_path) {
        if matches!(request.output_format, OutputFormat::AdaptiveHdrJpeg) {
            warnings.push(
                "HDR JPEG embeds the HDR gain map when the native backend can decode it; other iPhone auxiliary assets such as Portrait/depth/Live Photo data are not Apple-compatible JPEG payloads yet."
                    .to_string(),
            );
            if matches!(request.auxiliary_data_policy, AuxiliaryDataPolicy::ExtractSidecars) {
                warnings.push(
                    "Sidecar extraction is enabled. TypeShift will write raw HEIC auxiliary item payloads and a manifest beside the HDR JPEG when extractable items exist."
                        .to_string(),
                );
            }
            return warnings;
        }

        warnings.push(
            "Portrait/depth/gain-map auxiliary data was requested. Normal raster outputs are SDR and cannot apply or re-embed HEIC auxiliary images."
                .to_string(),
        );
        warnings.push(match request.auxiliary_data_policy {
            AuxiliaryDataPolicy::PreserveIfSupported => {
                "Standard metadata can be embedded into JPEG natively, but iPhone Portrait/depth auxiliary structures are not re-embedded into normal PNG/JPEG/WebP/TIFF/BMP raster outputs.".to_string()
            }
            AuxiliaryDataPolicy::ExtractSidecars => {
                "Sidecar extraction is enabled. TypeShift will write raw HEIC auxiliary item payloads and a manifest beside the converted output when the source contains extractable items.".to_string()
            }
            AuxiliaryDataPolicy::Discard => unreachable!("discard policy returned before warnings were built"),
        });
    } else if matches!(request.auxiliary_data_policy, AuxiliaryDataPolicy::ExtractSidecars) {
        warnings.push(
            "Sidecar extraction was requested, but this source is not a HEIC/HEIF file with supported auxiliary image items."
                .to_string(),
        );
    }

    warnings
}

fn hdr_warnings(_input_path: &Path, request: &ConvertRequest, source_has_gain_map: bool) -> Vec<String> {
    if matches!(request.output_format, OutputFormat::AdaptiveHdrJpeg) {
        return vec![
            "HDR JPEG output was selected. TypeShift will use only the native gain-map path for this target; SDR fallback encoders are not allowed for this output.".to_string(),
        ];
    }

    if source_has_gain_map {
        return vec![
            format!(
                "HEIC HDR gain-map data was detected. {} output is a normal SDR raster export; choose HDR JPEG for gain-map HDR output.",
                request.output_format.extension().to_ascii_uppercase()
            ),
        ];
    }

    Vec::new()
}

fn hdr_blocking_error(request: &ConvertRequest, _source_has_gain_map: bool) -> Option<String> {
    if matches!(request.output_format, OutputFormat::AdaptiveHdrJpeg) {
        return None;
    }

    None
}

pub fn safe_output_path(input_path: &Path, output_format: &OutputFormat) -> PathBuf {
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("converted");
    let extension = output_format.extension();
    let tag = output_format.output_tag();
    let first = parent.join(format!("{stem}.{tag}.{extension}"));
    if !first.exists() {
        return first;
    }

    for index in 1.. {
        let candidate = parent.join(format!("{stem}.{tag}-{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded output filename search should always return");
}

fn detect_format(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_uppercase())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}

fn image_format_for_path(path: &Path) -> Option<ImageFormat> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some(ImageFormat::Png),
        Some("jpg") | Some("jpeg") => Some(ImageFormat::Jpeg),
        Some("webp") => Some(ImageFormat::WebP),
        Some("tif") | Some("tiff") => Some(ImageFormat::Tiff),
        Some("bmp") => Some(ImageFormat::Bmp),
        Some("gif") => Some(ImageFormat::Gif),
        Some("ico") => Some(ImageFormat::Ico),
        _ => None,
    }
}

fn is_heif_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("heic" | "heif" | "hif")
    )
}

fn is_gif_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("gif")
    )
}

fn convert_adaptive_hdr_jpeg(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertRequest,
    source_has_gain_map: bool,
) -> Result<Vec<String>, String> {
    if !is_heif_like(input_path) {
        return Err(
            "Adaptive HDR JPEG output currently requires an iPhone HEIC/HEIF source with an HDR gain map."
                .to_string(),
        );
    }

    if !source_has_gain_map {
        return Err(
            "No HEIC HDR gain map was detected in this source, so TypeShift cannot produce true Adaptive HDR JPEG output from it."
                .to_string(),
        );
    }

    let status = crate::hdr_backend::inspect_hdr_backend();
    if !status.available {
        return Err(format!(
            "{} {}",
            status.detail,
            status.next_action
        ));
    }

    crate::native_heif_hdr::convert_heic_to_hdr_jpeg(input_path, output_path, request)
}

#[derive(Debug)]
struct HeifInspection {
    dimensions: Option<Dimensions>,
    bit_depth: Option<u8>,
    has_alpha: bool,
    color_profile_name: Option<String>,
    color_profile_type: Option<String>,
    warnings: Vec<String>,
}

#[cfg(target_os = "windows")]
fn inspect_heif(path: &Path) -> Result<HeifInspection, String> {
    windows_wic::inspect_heif(path).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "windows"))]
fn inspect_heif(_path: &Path) -> Result<HeifInspection, String> {
    Err("HEIC inspection is currently available only on Windows builds.".to_string())
}

#[cfg(target_os = "windows")]
fn convert_heif(input_path: &Path, output_path: &Path, request: &ConvertRequest) -> Result<Vec<String>, String> {
    windows_wic::convert_heif(input_path, output_path, request).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "windows"))]
fn convert_heif(_input_path: &Path, _output_path: &Path, _request: &ConvertRequest) -> Result<Vec<String>, String> {
    Err("HEIC conversion is currently available only on Windows builds.".to_string())
}

#[cfg(feature = "native-heif")]
fn create_native_heif_preview(input_path: &Path, output_path: &Path) -> Result<(), String> {
    crate::native_heif_hdr::write_heic_preview_png(input_path, output_path)
}

#[cfg(not(feature = "native-heif"))]
fn create_native_heif_preview(input_path: &Path, output_path: &Path) -> Result<(), String> {
    let request = ConvertRequest {
        input_paths: vec![input_path.to_string_lossy().to_string()],
        output_format: OutputFormat::Png,
        preset: Preset::VisualMatch,
        metadata_policy: MetadataPolicy::PreserveSafe,
        auxiliary_data_policy: AuxiliaryDataPolicy::Discard,
        destination_policy: DestinationPolicy::SameFolder,
        conflict_policy: ConflictPolicy::AutoNumber,
        jpeg_quality: 92,
        png_bit_depth: 8,
        hdr_policy: HdrPolicy::ToneMapSdr,
    };
    convert_heif(input_path, output_path, &request).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn safe_output_path_uses_same_folder_and_typeshift_suffix() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("photo.heic");

        assert_eq!(
            safe_output_path(&input, &OutputFormat::Png),
            dir.path().join("photo.typeshift.png")
        );
    }

    #[test]
    fn safe_output_path_auto_numbers_existing_outputs() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("photo.heic");
        fs::write(dir.path().join("photo.typeshift.jpg"), b"existing").unwrap();
        fs::write(dir.path().join("photo.typeshift-1.jpg"), b"existing").unwrap();

        assert_eq!(
            safe_output_path(&input, &OutputFormat::Jpeg),
            dir.path().join("photo.typeshift-2.jpg")
        );
    }

    #[test]
    fn adaptive_hdr_jpeg_uses_distinct_output_name() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("photo.heic");

        assert_eq!(
            safe_output_path(&input, &OutputFormat::AdaptiveHdrJpeg),
            dir.path().join("photo.typeshift-hdr.jpg")
        );
    }

    #[test]
    fn heic_inspection_reports_backend_warning() {
        let inspection = inspect_image(Path::new("sample.heic")).unwrap();

        assert_eq!(inspection.format, "HEIC");
        assert!(inspection
            .warnings
            .iter()
            .any(|warning| warning.contains("Windows WIC") || warning.contains("HEIC")));
    }

    #[test]
    fn image_format_for_path_supports_common_raster_inputs() {
        assert_eq!(image_format_for_path(Path::new("photo.png")), Some(ImageFormat::Png));
        assert_eq!(image_format_for_path(Path::new("photo.jpeg")), Some(ImageFormat::Jpeg));
        assert_eq!(image_format_for_path(Path::new("photo.webp")), Some(ImageFormat::WebP));
        assert_eq!(image_format_for_path(Path::new("photo.tiff")), Some(ImageFormat::Tiff));
        assert_eq!(image_format_for_path(Path::new("photo.bmp")), Some(ImageFormat::Bmp));
        assert_eq!(image_format_for_path(Path::new("photo.gif")), Some(ImageFormat::Gif));
        assert_eq!(image_format_for_path(Path::new("photo.ico")), Some(ImageFormat::Ico));
    }

    #[test]
    fn converts_png_fixture_to_supported_raster_outputs() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("source.png");
        let mut fixture = RgbaImage::new(2, 2);
        fixture.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        fixture.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        fixture.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
        fixture.put_pixel(1, 1, Rgba([255, 255, 255, 255]));
        fixture.save(&input).unwrap();

        for output_format in [
            OutputFormat::Png,
            OutputFormat::Jpeg,
            OutputFormat::Webp,
            OutputFormat::Tiff,
            OutputFormat::Bmp,
        ] {
            let request = ConvertRequest {
                input_paths: vec![input.to_string_lossy().to_string()],
                output_format,
                preset: Preset::VisualMatch,
                metadata_policy: MetadataPolicy::PreserveSafe,
                auxiliary_data_policy: AuxiliaryDataPolicy::Discard,
                destination_policy: DestinationPolicy::SameFolder,
                conflict_policy: ConflictPolicy::AutoNumber,
                jpeg_quality: 92,
                png_bit_depth: 16,
                hdr_policy: HdrPolicy::ToneMapSdr,
            };

            let result = convert_image(&input, &request);
            assert_eq!(result.status, ResultStatus::Completed);
            assert!(Path::new(result.output_path.as_ref().unwrap()).exists());
        }
    }

    #[test]
    fn auxiliary_data_warning_is_limited_to_heif_or_sidecar_requests() {
        let preserve_request = ConvertRequest {
            input_paths: vec!["photo.png".to_string()],
            output_format: OutputFormat::Png,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::PreserveIfSupported,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::ToneMapSdr,
        };
        assert!(auxiliary_data_warnings(Path::new("photo.png"), &preserve_request).is_empty());
        assert!(!auxiliary_data_warnings(Path::new("photo.heic"), &preserve_request).is_empty());

        let mut sidecar_request = preserve_request.clone();
        sidecar_request.auxiliary_data_policy = AuxiliaryDataPolicy::ExtractSidecars;
        assert!(!auxiliary_data_warnings(Path::new("photo.png"), &sidecar_request).is_empty());
    }

    #[test]
    fn hdr_gain_map_warns_for_sdr_outputs() {
        let request = ConvertRequest {
            input_paths: vec!["photo.heic".to_string()],
            output_format: OutputFormat::Png,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::Discard,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::ToneMapSdr,
        };

        assert!(hdr_warnings(Path::new("photo.heic"), &request, false).is_empty());
        assert!(hdr_warnings(Path::new("photo.heic"), &request, true)
            .iter()
            .any(|warning| warning.contains("normal SDR raster export")));
    }

    #[test]
    fn legacy_hdr_policy_no_longer_blocks_raster_conversion() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("source.png");
        RgbaImage::new(1, 1).save(&input).unwrap();
        let request = ConvertRequest {
            input_paths: vec![input.to_string_lossy().to_string()],
            output_format: OutputFormat::Png,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::Discard,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::PreserveHdr,
        };

        let result = convert_image(&input, &request);
        assert_eq!(result.status, ResultStatus::Completed);
    }

    #[test]
    fn detected_gain_map_does_not_block_sdr_output() {
        let request = ConvertRequest {
            input_paths: vec!["photo.heic".to_string()],
            output_format: OutputFormat::Jpeg,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::PreserveIfSupported,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::ToneMapSdr,
        };

        assert!(hdr_blocking_error(&request, true).is_none());
        assert!(hdr_blocking_error(&request, false).is_none());
    }

    #[test]
    fn adaptive_hdr_jpeg_uses_native_hdr_path_errors() {
        let request = ConvertRequest {
            input_paths: vec!["photo.heic".to_string()],
            output_format: OutputFormat::AdaptiveHdrJpeg,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::PreserveIfSupported,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::PreserveHdr,
        };

        assert!(hdr_blocking_error(&request, true).is_none());
        assert!(convert_adaptive_hdr_jpeg(Path::new("photo.heic"), Path::new("photo.typeshift-hdr.jpg"), &request, false)
            .is_err_and(|message| message.contains("No HEIC HDR gain map")));
    }

    #[test]
    #[ignore = "requires local ../sample.HEIC and writes a converted file beside it"]
    fn sample_heic_hdr_conversion_smoke() {
        let input = Path::new("../sample.HEIC");
        if !input.exists() {
            return;
        }

        let request = ConvertRequest {
            input_paths: vec![input.to_string_lossy().to_string()],
            output_format: OutputFormat::AdaptiveHdrJpeg,
            preset: Preset::VisualMatch,
            metadata_policy: MetadataPolicy::PreserveSafe,
            auxiliary_data_policy: AuxiliaryDataPolicy::PreserveIfSupported,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
            jpeg_quality: 92,
            png_bit_depth: 16,
            hdr_policy: HdrPolicy::PreserveHdr,
        };

        let result = convert_image(input, &request);
        println!("sample HEIC result: {result:#?}");
        assert_eq!(result.status, ResultStatus::Completed);
        let output_path = result.output_path.as_ref().unwrap();
        let output = std::fs::read(output_path).unwrap();
        let inspection = ultrajpeg::inspect(&output).unwrap();
        let metadata = inspection
            .ultra_hdr
            .and_then(|ultra_hdr| ultra_hdr.gain_map_metadata)
            .unwrap();
        assert!(metadata.hdr_capacity_max > 3.0);
        assert!(metadata.max_content_boost[0] > 3.0);
    }
}
