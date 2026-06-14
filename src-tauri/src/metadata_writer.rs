use crate::image_engine::MetadataPolicy;
use img_parts::{
    jpeg::{markers, Jpeg, JpegSegment},
    Bytes, ImageEXIF, ImageICC,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
};

const XMP_APP1_PREFIX: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXIF_LE: &[u8] = b"II\x2A\0";
const EXIF_BE: &[u8] = b"MM\0\x2A";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetadataToolStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetadataWriteStatus {
    Applied,
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataWriteOutcome {
    pub status: MetadataWriteStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct SourceMetadata {
    exif: Option<Vec<u8>>,
    xmp: Option<Vec<u8>>,
    icc: Option<Vec<u8>>,
    warnings: Vec<String>,
}

pub fn copy_metadata_after_conversion(
    source_path: &Path,
    output_path: &Path,
    metadata_policy: &MetadataPolicy,
) -> MetadataWriteOutcome {
    match copy_metadata_native(source_path, output_path, metadata_policy) {
        Ok(outcome) => outcome,
        Err(error) => skipped(format!("Native metadata copy was skipped: {error}")),
    }
}

pub fn inspect_metadata_tool() -> MetadataToolStatus {
    MetadataToolStatus {
        available: true,
        path: None,
        version: None,
        detail: "Native JPEG metadata writer is available. TypeShift can copy standard EXIF/XMP/ICC metadata in-process.".to_string(),
    }
}

fn copy_metadata_native(
    source_path: &Path,
    output_path: &Path,
    metadata_policy: &MetadataPolicy,
) -> Result<MetadataWriteOutcome, String> {
    if !is_jpeg_path(output_path) {
        return Ok(skipped("Metadata copy currently targets JPEG outputs only.".to_string()));
    }

    let mut source = read_source_metadata(source_path)?;
    let mut warnings = Vec::new();
    warnings.append(&mut source.warnings);

    let mut output = Jpeg::from_bytes(fs::read(output_path).map_err(|error| {
        format!("Could not read JPEG output {}: {error}", output_path.display())
    })?.into())
    .map_err(|error| format!("Could not parse JPEG output for metadata writing: {error}"))?;

    match metadata_policy {
        MetadataPolicy::StripExceptColor => {
            if let Some(icc) = source.icc {
                output.set_icc_profile(Some(Bytes::from(icc)));
                warnings.push("ICC color profile copied natively.".to_string());
            } else {
                warnings.push("No source ICC profile was available for native metadata copy.".to_string());
            }
        }
        MetadataPolicy::PreserveSafe | MetadataPolicy::PreserveAll => {
            let strip_gps = matches!(metadata_policy, MetadataPolicy::PreserveSafe);

            if let Some(icc) = source.icc {
                output.set_icc_profile(Some(Bytes::from(icc)));
                warnings.push("ICC color profile copied natively.".to_string());
            }

            if let Some(exif) = source.exif.as_mut() {
                sanitize_exif(exif, strip_gps)?;
                output.set_exif(Some(Bytes::from(exif.clone())));
                warnings.push(if strip_gps {
                    "EXIF metadata copied natively with orientation normalized and GPS references stripped.".to_string()
                } else {
                    "EXIF metadata copied natively with orientation normalized.".to_string()
                });
            } else {
                warnings.push("No source EXIF metadata was available for native metadata copy.".to_string());
            }

            if let Some(xmp) = source.xmp.as_deref() {
                match sanitize_xmp(xmp, strip_gps) {
                    XmpSanitizeResult::Keep(cleaned) => {
                        set_xmp(&mut output, cleaned)?;
                        warnings.push(if strip_gps {
                            "XMP metadata copied natively after privacy and orientation checks.".to_string()
                        } else {
                            "XMP metadata copied natively after orientation checks.".to_string()
                        });
                    }
                    XmpSanitizeResult::Dropped(reason) => warnings.push(reason),
                }
            }
        }
    }

    let mut encoded = Vec::new();
    output
        .encoder()
        .write_to(&mut encoded)
        .map_err(|error| format!("Could not encode JPEG metadata output: {error}"))?;
    fs::write(output_path, encoded)
        .map_err(|error| format!("Could not write JPEG metadata output {}: {error}", output_path.display()))?;

    Ok(MetadataWriteOutcome {
        status: MetadataWriteStatus::Applied,
        warnings,
    })
}

fn read_source_metadata(source_path: &Path) -> Result<SourceMetadata, String> {
    if is_jpeg_path(source_path) {
        return read_jpeg_source_metadata(source_path);
    }

    if is_heif_like(source_path) {
        return read_heif_source_metadata(source_path);
    }

    Ok(SourceMetadata {
        warnings: vec![
            "Native metadata copy found no supported metadata extractor for this source format.".to_string(),
        ],
        ..SourceMetadata::default()
    })
}

fn read_jpeg_source_metadata(source_path: &Path) -> Result<SourceMetadata, String> {
    let jpeg = Jpeg::from_bytes(fs::read(source_path).map_err(|error| {
        format!("Could not read JPEG source {}: {error}", source_path.display())
    })?.into())
    .map_err(|error| format!("Could not parse JPEG source metadata: {error}"))?;

    Ok(SourceMetadata {
        exif: jpeg.exif().map(|bytes| bytes.to_vec()),
        xmp: jpeg_xmp(&jpeg).map(|bytes| bytes.to_vec()),
        icc: jpeg.icc_profile().map(|bytes| bytes.to_vec()),
        warnings: Vec::new(),
    })
}

fn read_heif_source_metadata(source_path: &Path) -> Result<SourceMetadata, String> {
    let (payloads, mut warnings) = crate::heif_auxiliary::extract_metadata_payloads(source_path)?;
    let mut metadata = SourceMetadata::default();
    metadata.warnings.append(&mut warnings);

    for payload in payloads {
        if metadata.exif.is_none() && payload.metadata_type == "Exif" {
            if let Some(exif) = normalize_heif_exif_payload(&payload.payload) {
                metadata.exif = Some(exif);
            } else {
                metadata.warnings.push(format!(
                    "HEIC Exif metadata item {} could not be normalized for JPEG embedding.",
                    payload.item_id
                ));
            }
            continue;
        }

        if metadata.xmp.is_none() {
            if let Some(xmp) = extract_xmp_payload(&payload.payload) {
                metadata.xmp = Some(xmp);
            }
        }
    }

    if metadata.exif.is_none() && metadata.xmp.is_none() {
        metadata.warnings.push(
            "No standard HEIC EXIF/XMP metadata payloads were found for native JPEG embedding.".to_string(),
        );
    }

    Ok(metadata)
}

fn jpeg_xmp(jpeg: &Jpeg) -> Option<Bytes> {
    jpeg.segments_by_marker(markers::APP1)
        .find_map(|segment| {
            segment
                .contents()
                .strip_prefix(XMP_APP1_PREFIX)
                .map(|bytes| Bytes::copy_from_slice(bytes))
        })
}

fn set_xmp(jpeg: &mut Jpeg, xmp: Vec<u8>) -> Result<(), String> {
    if XMP_APP1_PREFIX.len() + xmp.len() > u16::MAX as usize - 2 {
        return Err("XMP metadata is too large for a standard JPEG APP1 segment".to_string());
    }

    jpeg.segments_mut().retain(|segment| {
        !(segment.marker() == markers::APP1 && segment.contents().starts_with(XMP_APP1_PREFIX))
    });

    let mut contents = Vec::with_capacity(XMP_APP1_PREFIX.len() + xmp.len());
    contents.extend_from_slice(XMP_APP1_PREFIX);
    contents.extend_from_slice(&xmp);
    let segment = JpegSegment::new_with_contents(markers::APP1, Bytes::from(contents));
    let insert_at = jpeg
        .segments()
        .iter()
        .position(|segment| segment.has_entropy())
        .unwrap_or_else(|| jpeg.segments().len())
        .min(4);
    jpeg.segments_mut().insert(insert_at, segment);
    Ok(())
}

fn normalize_heif_exif_payload(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.starts_with(EXIF_LE) || payload.starts_with(EXIF_BE) {
        return Some(payload.to_vec());
    }

    let search_end = payload.len().min(32);
    for offset in 0..search_end {
        if payload[offset..].starts_with(EXIF_LE) || payload[offset..].starts_with(EXIF_BE) {
            return Some(payload[offset..].to_vec());
        }
    }

    None
}

fn extract_xmp_payload(payload: &[u8]) -> Option<Vec<u8>> {
    for marker in [b"<?xpacket".as_slice(), b"<x:xmpmeta".as_slice(), b"<rdf:RDF".as_slice()] {
        if let Some(start) = find_bytes(payload, marker) {
            return Some(payload[start..].to_vec());
        }
    }
    None
}

enum XmpSanitizeResult {
    Keep(Vec<u8>),
    Dropped(String),
}

fn sanitize_xmp(xmp: &[u8], strip_gps: bool) -> XmpSanitizeResult {
    let text = String::from_utf8_lossy(xmp).to_ascii_lowercase();
    if contains_xmp_orientation(&text) {
        return XmpSanitizeResult::Dropped(
            "XMP metadata was not embedded because it contains orientation fields and converted pixels are already rendered.".to_string(),
        );
    }
    if strip_gps && contains_xmp_location(&text) {
        return XmpSanitizeResult::Dropped(
            "XMP metadata was not embedded because safe metadata mode removes location fields.".to_string(),
        );
    }
    XmpSanitizeResult::Keep(xmp.to_vec())
}

fn contains_xmp_orientation(text: &str) -> bool {
    text.contains("tiff:orientation") || text.contains("exif:orientation")
}

fn contains_xmp_location(text: &str) -> bool {
    [
        "gpslatitude",
        "gpslongitude",
        "gpsaltitude",
        "geotag",
        "locationcreated",
        "locationshown",
        "iptc4xmpcore:location",
        "photoshop:city",
        "photoshop:country",
        "iptcext:gps",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn sanitize_exif(exif: &mut [u8], strip_gps: bool) -> Result<(), String> {
    if exif.len() < 8 {
        return Err("EXIF payload is too short".to_string());
    }

    let endian = Endian::from_exif(exif)?;
    if endian.read_u16(exif, 2)? != 42 {
        return Err("EXIF TIFF header is invalid".to_string());
    }
    let ifd0_offset = endian.read_u32(exif, 4)? as usize;
    sanitize_ifd(exif, endian, ifd0_offset, strip_gps, 0)
}

fn sanitize_ifd(
    exif: &mut [u8],
    endian: Endian,
    ifd_offset: usize,
    strip_gps: bool,
    depth: usize,
) -> Result<(), String> {
    if ifd_offset == 0 || depth > 4 {
        return Ok(());
    }
    if ifd_offset + 2 > exif.len() {
        return Err("EXIF IFD offset is out of range".to_string());
    }

    let entry_count = endian.read_u16(exif, ifd_offset)? as usize;
    let entries_start = ifd_offset + 2;
    let entries_end = entries_start
        .checked_add(entry_count.saturating_mul(12))
        .ok_or_else(|| "EXIF IFD entry table overflowed".to_string())?;
    if entries_end + 4 > exif.len() {
        return Err("EXIF IFD entry table is out of range".to_string());
    }

    let mut child_ifds = Vec::new();
    let mut gps_ifd = None;

    for index in 0..entry_count {
        let entry_offset = entries_start + index * 12;
        let tag = endian.read_u16(exif, entry_offset)?;
        match tag {
            0x0112 => set_orientation_to_normal(exif, endian, entry_offset)?,
            0x8769 | 0x8825 | 0xA005 => {
                let target = endian.read_u32(exif, entry_offset + 8)? as usize;
                if tag == 0x8825 {
                    gps_ifd = Some((entry_offset, target));
                } else {
                    child_ifds.push(target);
                }
            }
            _ => {}
        }
    }

    if strip_gps {
        if let Some((entry_offset, gps_offset)) = gps_ifd {
            zero_ifd_payloads(exif, endian, gps_offset)?;
            endian.write_u32(exif, entry_offset + 4, 1)?;
            endian.write_u32(exif, entry_offset + 8, 0)?;
        }
    } else if let Some((_, gps_offset)) = gps_ifd {
        child_ifds.push(gps_offset);
    }

    let next_ifd = endian.read_u32(exif, entries_end)? as usize;
    if next_ifd != 0 {
        child_ifds.push(next_ifd);
    }

    for child_ifd in child_ifds {
        sanitize_ifd(exif, endian, child_ifd, strip_gps, depth + 1)?;
    }

    Ok(())
}

fn set_orientation_to_normal(exif: &mut [u8], endian: Endian, entry_offset: usize) -> Result<(), String> {
    endian.write_u16(exif, entry_offset + 2, 3)?;
    endian.write_u32(exif, entry_offset + 4, 1)?;
    endian.write_inline_short(exif, entry_offset + 8, 1)
}

fn zero_ifd_payloads(exif: &mut [u8], endian: Endian, ifd_offset: usize) -> Result<(), String> {
    if ifd_offset == 0 || ifd_offset + 2 > exif.len() {
        return Ok(());
    }

    let entry_count = endian.read_u16(exif, ifd_offset)? as usize;
    let entries_start = ifd_offset + 2;
    let entries_end = entries_start
        .checked_add(entry_count.saturating_mul(12))
        .ok_or_else(|| "EXIF GPS IFD entry table overflowed".to_string())?;
    if entries_end + 4 > exif.len() {
        return Ok(());
    }

    for index in 0..entry_count {
        let entry_offset = entries_start + index * 12;
        if let Some((value_offset, value_len)) = exif_value_range(exif, endian, entry_offset)? {
            if value_offset < exif.len() {
                let end = value_offset.saturating_add(value_len).min(exif.len());
                exif[value_offset..end].fill(0);
            }
        }
        exif[entry_offset..entry_offset + 12].fill(0);
    }
    exif[ifd_offset..entries_end + 4].fill(0);
    Ok(())
}

fn exif_value_range(
    exif: &[u8],
    endian: Endian,
    entry_offset: usize,
) -> Result<Option<(usize, usize)>, String> {
    let value_type = endian.read_u16(exif, entry_offset + 2)?;
    let count = endian.read_u32(exif, entry_offset + 4)? as usize;
    let unit: usize = match value_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return Ok(None),
    };
    let length = unit.saturating_mul(count);
    if length <= 4 {
        return Ok(Some((entry_offset + 8, 4)));
    }
    Ok(Some((endian.read_u32(exif, entry_offset + 8)? as usize, length)))
}

#[derive(Debug, Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn from_exif(exif: &[u8]) -> Result<Self, String> {
        if exif.starts_with(EXIF_LE) {
            Ok(Self::Little)
        } else if exif.starts_with(EXIF_BE) {
            Ok(Self::Big)
        } else {
            Err("EXIF payload does not start with a TIFF header".to_string())
        }
    }

    fn read_u16(self, data: &[u8], offset: usize) -> Result<u16, String> {
        if offset + 2 > data.len() {
            return Err("EXIF read exceeded payload length".to_string());
        }
        Ok(match self {
            Self::Little => u16::from_le_bytes([data[offset], data[offset + 1]]),
            Self::Big => u16::from_be_bytes([data[offset], data[offset + 1]]),
        })
    }

    fn read_u32(self, data: &[u8], offset: usize) -> Result<u32, String> {
        if offset + 4 > data.len() {
            return Err("EXIF read exceeded payload length".to_string());
        }
        Ok(match self {
            Self::Little => u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]),
            Self::Big => u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]),
        })
    }

    fn write_u16(self, data: &mut [u8], offset: usize, value: u16) -> Result<(), String> {
        if offset + 2 > data.len() {
            return Err("EXIF write exceeded payload length".to_string());
        }
        let bytes = match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        };
        data[offset..offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    fn write_u32(self, data: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
        if offset + 4 > data.len() {
            return Err("EXIF write exceeded payload length".to_string());
        }
        let bytes = match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        };
        data[offset..offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    fn write_inline_short(self, data: &mut [u8], offset: usize, value: u16) -> Result<(), String> {
        if offset + 4 > data.len() {
            return Err("EXIF write exceeded payload length".to_string());
        }
        data[offset..offset + 4].fill(0);
        self.write_u16(data, offset, value)
    }
}

fn is_jpeg_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg")
    )
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

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn skipped(warning: String) -> MetadataWriteOutcome {
    MetadataWriteOutcome {
        status: MetadataWriteStatus::Skipped,
        warnings: vec![warning],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn native_metadata_status_is_available_without_external_tool() {
        let status = inspect_metadata_tool();
        assert!(status.available);
        assert!(status.detail.contains("Native JPEG metadata writer"));
    }

    #[test]
    fn normalizes_heif_exif_payload_with_offset_prefix() {
        let mut payload = vec![0, 0, 0, 0];
        payload.extend_from_slice(&minimal_exif_with_orientation_and_gps());

        let normalized = normalize_heif_exif_payload(&payload).unwrap();
        assert!(normalized.starts_with(EXIF_LE));
    }

    #[test]
    fn sanitize_exif_sets_orientation_and_removes_gps_pointer() {
        let mut exif = minimal_exif_with_orientation_and_gps();
        sanitize_exif(&mut exif, true).unwrap();
        let endian = Endian::Little;
        let ifd0 = endian.read_u32(&exif, 4).unwrap() as usize;
        let orientation_entry = ifd0 + 2;
        let gps_entry = ifd0 + 2 + 12;

        assert_eq!(endian.read_u16(&exif, orientation_entry).unwrap(), 0x0112);
        assert_eq!(endian.read_u32(&exif, orientation_entry + 8).unwrap() & 0xFFFF, 1);
        assert_eq!(endian.read_u16(&exif, gps_entry).unwrap(), 0x8825);
        assert_eq!(endian.read_u32(&exif, gps_entry + 8).unwrap(), 0);
        assert!(exif[38..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn native_jpeg_copy_embeds_sanitized_exif() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.jpg");
        let output = dir.path().join("output.jpg");
        RgbImage::from_pixel(2, 2, Rgb([120, 40, 80])).save(&source).unwrap();
        RgbImage::from_pixel(2, 2, Rgb([10, 20, 30])).save(&output).unwrap();

        let mut source_jpeg = Jpeg::from_bytes(fs::read(&source).unwrap().into()).unwrap();
        source_jpeg.set_exif(Some(Bytes::from(minimal_exif_with_orientation_and_gps())));
        source_jpeg.encoder().write_to(fs::File::create(&source).unwrap()).unwrap();

        let outcome = copy_metadata_after_conversion(&source, &output, &MetadataPolicy::PreserveSafe);
        assert_eq!(outcome.status, MetadataWriteStatus::Applied);

        let output_jpeg = Jpeg::from_bytes(fs::read(&output).unwrap().into()).unwrap();
        let output_exif = output_jpeg.exif().unwrap();
        let endian = Endian::Little;
        let ifd0 = endian.read_u32(&output_exif, 4).unwrap() as usize;
        assert_eq!(endian.read_u32(&output_exif, ifd0 + 2 + 8).unwrap() & 0xFFFF, 1);
        assert_eq!(endian.read_u32(&output_exif, ifd0 + 2 + 12 + 8).unwrap(), 0);
    }

    fn minimal_exif_with_orientation_and_gps() -> Vec<u8> {
        let mut exif = vec![0; 58];
        exif[0..4].copy_from_slice(EXIF_LE);
        exif[4..8].copy_from_slice(&8u32.to_le_bytes());

        exif[8..10].copy_from_slice(&2u16.to_le_bytes());

        let orientation = 10;
        exif[orientation..orientation + 2].copy_from_slice(&0x0112u16.to_le_bytes());
        exif[orientation + 2..orientation + 4].copy_from_slice(&3u16.to_le_bytes());
        exif[orientation + 4..orientation + 8].copy_from_slice(&1u32.to_le_bytes());
        exif[orientation + 8..orientation + 10].copy_from_slice(&6u16.to_le_bytes());

        let gps = 22;
        exif[gps..gps + 2].copy_from_slice(&0x8825u16.to_le_bytes());
        exif[gps + 2..gps + 4].copy_from_slice(&4u16.to_le_bytes());
        exif[gps + 4..gps + 8].copy_from_slice(&1u32.to_le_bytes());
        exif[gps + 8..gps + 12].copy_from_slice(&38u32.to_le_bytes());
        exif[34..38].copy_from_slice(&0u32.to_le_bytes());

        exif[38..40].copy_from_slice(&1u16.to_le_bytes());
        exif[40..42].copy_from_slice(&1u16.to_le_bytes());
        exif[42..44].copy_from_slice(&2u16.to_le_bytes());
        exif[44..48].copy_from_slice(&2u32.to_le_bytes());
        exif[48..52].copy_from_slice(&54u32.to_le_bytes());
        exif[52..56].copy_from_slice(&0u32.to_le_bytes());
        exif[54] = b'N';
        exif[55] = 0;
        exif
    }
}
