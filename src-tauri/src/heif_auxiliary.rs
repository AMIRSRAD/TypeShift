use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeifAuxiliaryInspection {
    pub backend: String,
    pub has_hdr_gain_map: bool,
    pub hdr_indicators: Vec<String>,
    pub auxiliary_items: Vec<HeifAuxiliaryItem>,
    pub metadata_items: Vec<HeifMetadataItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeifAuxiliaryItem {
    pub item_id: u32,
    pub item_type: String,
    pub role: HeifAuxiliaryRole,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_depth: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeifAuxiliaryRole {
    Depth,
    Disparity,
    PortraitEffectsMatte,
    SemanticMatte,
    GainMap,
    Alpha,
    Thumbnail,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeifMetadataItem {
    pub item_id: u32,
    pub metadata_type: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeifMetadataPayload {
    pub item_id: u32,
    pub metadata_type: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeifSidecar {
    pub item_id: Option<u32>,
    pub kind: HeifSidecarKind,
    pub role: Option<HeifAuxiliaryRole>,
    pub metadata_type: Option<String>,
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeifSidecarKind {
    AuxiliaryItem,
    MetadataItem,
    Manifest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuxiliaryManifest {
    source_path: String,
    output_path: String,
    backend: String,
    has_hdr_gain_map: bool,
    hdr_indicators: Vec<String>,
    auxiliary_items: Vec<HeifAuxiliaryItem>,
    metadata_items: Vec<HeifMetadataItem>,
    sidecars: Vec<HeifSidecar>,
    warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct ParsedHeif {
    items: HashMap<u32, ItemInfo>,
    locations: HashMap<u32, ItemLocation>,
    properties: HashMap<u16, ItemProperty>,
    associations: HashMap<u32, Vec<u16>>,
    references: Vec<ItemReference>,
}

#[derive(Debug, Default)]
struct ItemInfo {
    item_type: String,
}

#[derive(Debug, Default)]
struct ItemLocation {
    extents: Vec<ItemExtent>,
}

#[derive(Debug)]
struct ItemExtent {
    offset: u64,
    length: u64,
}

#[derive(Debug)]
enum ItemProperty {
    AuxType(String),
    Dimensions(u32, u32),
    PixelBits(Vec<u8>),
}

#[derive(Debug)]
struct ItemReference {
    reference_type: String,
    to_item_ids: Vec<u32>,
}

#[derive(Debug)]
struct BoxHeader {
    box_type: String,
    data_start: usize,
    data_end: usize,
}

pub fn inspect_auxiliary_items(path: &Path) -> HeifAuxiliaryInspection {
    match fs::read(path) {
        Ok(data) => {
            let hdr_indicators = hdr_gain_map_indicators(&data);
            match parse_bytes(&data) {
                Ok(parsed) => inspection_from_parsed(
                    "heif-container-parser",
                    parsed,
                    hdr_indicators,
                    Vec::new(),
                ),
                Err(error) => HeifAuxiliaryInspection {
                    backend: "heif-container-parser".to_string(),
                    has_hdr_gain_map: !hdr_indicators.is_empty(),
                    hdr_indicators,
                    auxiliary_items: Vec::new(),
                    metadata_items: Vec::new(),
                    warnings: vec![format!(
                        "Could not inspect HEIC auxiliary items: {error}. TypeShift will still use HEIC metadata string indicators for HDR safety checks."
                    )],
                },
            }
        }
        Err(error) => HeifAuxiliaryInspection {
            backend: "heif-container-parser".to_string(),
            has_hdr_gain_map: false,
            hdr_indicators: Vec::new(),
            auxiliary_items: Vec::new(),
            metadata_items: Vec::new(),
            warnings: vec![format!(
                "Could not read HEIC auxiliary items: {error}. TypeShift will still try the rendered-image decoder."
            )],
        },
    }
}

pub fn extract_auxiliary_sidecars(
    source_path: &Path,
    output_path: &Path,
) -> Result<(Vec<HeifSidecar>, Vec<String>), String> {
    let data = fs::read(source_path).map_err(|error| format!("Could not read HEIC source: {error}"))?;
    let parsed = parse_bytes(&data)?;
    let inspection = inspection_from_parsed(
        "heif-container-parser",
        parsed,
        hdr_gain_map_indicators(&data),
        Vec::new(),
    );
    let parsed = parse_bytes(&data)?;
    let mut warnings = inspection.warnings.clone();

    if inspection.auxiliary_items.is_empty() && inspection.metadata_items.is_empty() {
        warnings.push("No HEIC auxiliary or metadata items were found to extract.".to_string());
        return Ok((Vec::new(), warnings));
    }

    let mut sidecars = Vec::new();
    let mut used_paths = HashSet::new();
    for item in &inspection.auxiliary_items {
        let Some(payload) = item_payload(&data, &parsed, item.item_id, &mut warnings)? else {
            warnings.push(format!("Auxiliary item {} has no file-backed location.", item.item_id));
            continue;
        };

        if payload.is_empty() {
            continue;
        }

        let sidecar_path = unique_auxiliary_sidecar_path(output_path, item, &mut used_paths);
        fs::write(&sidecar_path, &payload)
            .map_err(|error| format!("Could not write auxiliary sidecar {}: {error}", sidecar_path.display()))?;
        sidecars.push(HeifSidecar {
            item_id: Some(item.item_id),
            kind: HeifSidecarKind::AuxiliaryItem,
            role: Some(item.role.clone()),
            metadata_type: None,
            path: sidecar_path.to_string_lossy().to_string(),
            bytes: payload.len() as u64,
        });
    }

    for item in &inspection.metadata_items {
        let Some(payload) = item_payload(&data, &parsed, item.item_id, &mut warnings)? else {
            warnings.push(format!("Metadata item {} has no file-backed location.", item.item_id));
            continue;
        };

        if payload.is_empty() {
            continue;
        }

        let sidecar_path = unique_metadata_sidecar_path(output_path, item, &mut used_paths);
        fs::write(&sidecar_path, &payload)
            .map_err(|error| format!("Could not write metadata sidecar {}: {error}", sidecar_path.display()))?;
        sidecars.push(HeifSidecar {
            item_id: Some(item.item_id),
            kind: HeifSidecarKind::MetadataItem,
            role: None,
            metadata_type: Some(item.metadata_type.clone()),
            path: sidecar_path.to_string_lossy().to_string(),
            bytes: payload.len() as u64,
        });
    }

    let manifest_path = output_path.with_extension("aux-manifest.json");
    let manifest = AuxiliaryManifest {
        source_path: source_path.to_string_lossy().to_string(),
        output_path: output_path.to_string_lossy().to_string(),
        backend: inspection.backend,
        has_hdr_gain_map: inspection.has_hdr_gain_map,
        hdr_indicators: inspection.hdr_indicators,
        auxiliary_items: inspection.auxiliary_items,
        metadata_items: inspection.metadata_items,
        sidecars: sidecars.clone(),
        warnings: warnings.clone(),
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Could not serialize auxiliary manifest: {error}"))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|error| format!("Could not write auxiliary manifest {}: {error}", manifest_path.display()))?;
    let manifest_bytes = fs::metadata(&manifest_path).map(|metadata| metadata.len()).unwrap_or_default();
    sidecars.push(HeifSidecar {
        item_id: None,
        kind: HeifSidecarKind::Manifest,
        role: None,
        metadata_type: None,
        path: manifest_path.to_string_lossy().to_string(),
        bytes: manifest_bytes,
    });
    warnings.push(format!("Auxiliary manifest written to {}.", manifest_path.display()));

    Ok((sidecars, warnings))
}

pub fn extract_metadata_payloads(source_path: &Path) -> Result<(Vec<HeifMetadataPayload>, Vec<String>), String> {
    let data = fs::read(source_path).map_err(|error| format!("Could not read HEIC source: {error}"))?;
    let parsed = parse_bytes(&data)?;
    let inspection = inspection_from_parsed(
        "heif-container-parser",
        parse_bytes(&data)?,
        hdr_gain_map_indicators(&data),
        Vec::new(),
    );
    let mut warnings = inspection.warnings;
    let mut payloads = Vec::new();

    for item in inspection.metadata_items {
        let Some(payload) = item_payload(&data, &parsed, item.item_id, &mut warnings)? else {
            warnings.push(format!("Metadata item {} has no file-backed location.", item.item_id));
            continue;
        };
        if payload.is_empty() {
            continue;
        }
        payloads.push(HeifMetadataPayload {
            item_id: item.item_id,
            metadata_type: item.metadata_type,
            payload,
        });
    }

    Ok((payloads, warnings))
}

fn parse_bytes(data: &[u8]) -> Result<ParsedHeif, String> {
    let mut parsed = ParsedHeif::default();
    let boxes = parse_boxes(data, 0, data.len())?;
    let Some(meta) = boxes.into_iter().find(|header| header.box_type == "meta") else {
        return Err("HEIC file has no meta box".to_string());
    };
    if meta.data_start + 4 > meta.data_end {
        return Err("HEIC meta box is truncated".to_string());
    }

    for child in parse_boxes(data, meta.data_start + 4, meta.data_end)? {
        match child.box_type.as_str() {
            "iinf" => parse_iinf(data, &child, &mut parsed)?,
            "iloc" => parse_iloc(data, &child, &mut parsed)?,
            "iref" => parse_iref(data, &child, &mut parsed)?,
            "iprp" => parse_iprp(data, &child, &mut parsed)?,
            _ => {}
        }
    }

    Ok(parsed)
}

fn inspection_from_parsed(
    backend: &str,
    parsed: ParsedHeif,
    hdr_indicators: Vec<String>,
    mut warnings: Vec<String>,
) -> HeifAuxiliaryInspection {
    let referenced_auxiliary_ids = parsed
        .references
        .iter()
        .filter(|reference| reference.reference_type == "auxl" || reference.reference_type == "thmb")
        .flat_map(|reference| reference.to_item_ids.iter().copied())
        .collect::<HashSet<_>>();

    let mut auxiliary_items = Vec::new();
    for (item_id, item_info) in &parsed.items {
        let properties = item_properties(*item_id, &parsed);
        let aux_type = properties.iter().find_map(|property| match property {
            ItemProperty::AuxType(value) => Some(value.as_str()),
            _ => None,
        });
        let role = role_for_item(&item_info.item_type, aux_type, referenced_auxiliary_ids.contains(item_id));
        if matches!(role, HeifAuxiliaryRole::Unknown) && aux_type.is_none() {
            continue;
        }

        let dimensions = properties.iter().find_map(|property| match property {
            ItemProperty::Dimensions(width, height) => Some((*width, *height)),
            _ => None,
        });
        let bit_depth = properties.iter().find_map(|property| match property {
            ItemProperty::PixelBits(bits) => bits.iter().copied().max(),
            _ => None,
        });

        auxiliary_items.push(HeifAuxiliaryItem {
            item_id: *item_id,
            item_type: item_info.item_type.clone(),
            role,
            width: dimensions.map(|value| value.0),
            height: dimensions.map(|value| value.1),
            bit_depth,
        });
    }

    auxiliary_items.sort_by_key(|item| item.item_id);

    let mut metadata_items = parsed
        .items
        .iter()
        .filter(|(_, info)| is_metadata_type(&info.item_type))
        .map(|(item_id, info)| HeifMetadataItem {
            item_id: *item_id,
            metadata_type: info.item_type.clone(),
        })
        .collect::<Vec<_>>();
    metadata_items.sort_by_key(|item| item.item_id);

    let role_detected_gain_map = auxiliary_items
        .iter()
        .any(|item| matches!(item.role, HeifAuxiliaryRole::GainMap));
    let has_hdr_gain_map = role_detected_gain_map || !hdr_indicators.is_empty();

    if role_detected_gain_map {
        warnings.push("HEIC HDR gain-map auxiliary data was detected.".to_string());
    }
    if !hdr_indicators.is_empty() {
        warnings.push(format!(
            "HEIC HDR/gain-map metadata indicators were detected: {}.",
            hdr_indicators.join(", ")
        ));
    }
    if auxiliary_items.iter().any(|item| {
        matches!(
            item.role,
            HeifAuxiliaryRole::Depth
                | HeifAuxiliaryRole::Disparity
                | HeifAuxiliaryRole::PortraitEffectsMatte
                | HeifAuxiliaryRole::SemanticMatte
        )
    }) {
        warnings.push("HEIC Portrait/depth auxiliary data was detected.".to_string());
    }

    HeifAuxiliaryInspection {
        backend: backend.to_string(),
        has_hdr_gain_map,
        hdr_indicators,
        auxiliary_items,
        metadata_items,
        warnings,
    }
}

fn parse_iinf(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let (version, _, mut offset) = read_full_box(data, header)?;
    let entry_count = if version == 0 {
        u32::from(read_u16(data, &mut offset, header.data_end)?)
    } else {
        read_u32(data, &mut offset, header.data_end)?
    };

    for _ in 0..entry_count {
        let entry = read_box(data, offset, header.data_end)?;
        if entry.box_type == "infe" {
            parse_infe(data, &entry, parsed)?;
        }
        offset = entry.data_end;
    }

    Ok(())
}

fn parse_infe(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let (version, _, mut offset) = read_full_box(data, header)?;
    if version < 2 {
        return Ok(());
    }

    let item_id = if version == 2 {
        u32::from(read_u16(data, &mut offset, header.data_end)?)
    } else {
        read_u32(data, &mut offset, header.data_end)?
    };
    let _protection_index = read_u16(data, &mut offset, header.data_end)?;
    let item_type = read_fourcc(data, &mut offset, header.data_end)?;
    parsed.items.insert(item_id, ItemInfo { item_type });
    Ok(())
}

fn parse_iloc(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let (version, _, mut offset) = read_full_box(data, header)?;
    let sizes = read_u16(data, &mut offset, header.data_end)?;
    let offset_size = ((sizes >> 12) & 0x0f) as usize;
    let length_size = ((sizes >> 8) & 0x0f) as usize;
    let base_offset_size = ((sizes >> 4) & 0x0f) as usize;
    let index_size = if version == 1 || version == 2 {
        (sizes & 0x0f) as usize
    } else {
        0
    };
    let item_count = if version < 2 {
        u32::from(read_u16(data, &mut offset, header.data_end)?)
    } else {
        read_u32(data, &mut offset, header.data_end)?
    };

    for _ in 0..item_count {
        let item_id = if version < 2 {
            u32::from(read_u16(data, &mut offset, header.data_end)?)
        } else {
            read_u32(data, &mut offset, header.data_end)?
        };
        let construction_method = if version == 1 || version == 2 {
            read_u16(data, &mut offset, header.data_end)? & 0x000f
        } else {
            0
        };
        let data_reference_index = read_u16(data, &mut offset, header.data_end)?;
        let base_offset = read_variable(data, &mut offset, base_offset_size, header.data_end)?;
        let extent_count = read_u16(data, &mut offset, header.data_end)?;
        let mut extents = Vec::new();

        for _ in 0..extent_count {
            if index_size > 0 {
                let _extent_index = read_variable(data, &mut offset, index_size, header.data_end)?;
            }
            let extent_offset = read_variable(data, &mut offset, offset_size, header.data_end)?;
            let extent_length = read_variable(data, &mut offset, length_size, header.data_end)?;
            if construction_method == 0 && data_reference_index == 0 && extent_length > 0 {
                extents.push(ItemExtent {
                    offset: base_offset + extent_offset,
                    length: extent_length,
                });
            }
        }

        parsed.locations.insert(item_id, ItemLocation { extents });
    }

    Ok(())
}

fn parse_iref(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let (version, _, mut offset) = read_full_box(data, header)?;
    while offset < header.data_end {
        let reference_box = read_box(data, offset, header.data_end)?;
        let mut child_offset = reference_box.data_start;
        let _from_item_id = if version == 0 {
            u32::from(read_u16(data, &mut child_offset, reference_box.data_end)?)
        } else {
            read_u32(data, &mut child_offset, reference_box.data_end)?
        };
        let reference_count = read_u16(data, &mut child_offset, reference_box.data_end)?;
        let mut to_item_ids = Vec::new();
        for _ in 0..reference_count {
            to_item_ids.push(if version == 0 {
                u32::from(read_u16(data, &mut child_offset, reference_box.data_end)?)
            } else {
                read_u32(data, &mut child_offset, reference_box.data_end)?
            });
        }
        parsed.references.push(ItemReference {
            reference_type: reference_box.box_type,
            to_item_ids,
        });
        offset = reference_box.data_end;
    }

    Ok(())
}

fn parse_iprp(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    for child in parse_boxes(data, header.data_start, header.data_end)? {
        match child.box_type.as_str() {
            "ipco" => parse_ipco(data, &child, parsed)?,
            "ipma" => parse_ipma(data, &child, parsed)?,
            _ => {}
        }
    }

    Ok(())
}

fn parse_ipco(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let mut index = 1_u16;
    for child in parse_boxes(data, header.data_start, header.data_end)? {
        match child.box_type.as_str() {
            "auxC" => {
                let aux_type = read_c_string(data, child.data_start, child.data_end);
                parsed.properties.insert(index, ItemProperty::AuxType(aux_type));
            }
            "ispe" => {
                let (_, _, mut offset) = read_full_box(data, &child)?;
                let width = read_u32(data, &mut offset, child.data_end)?;
                let height = read_u32(data, &mut offset, child.data_end)?;
                parsed.properties.insert(index, ItemProperty::Dimensions(width, height));
            }
            "pixi" => {
                let (_, _, mut offset) = read_full_box(data, &child)?;
                let channel_count = read_u8(data, &mut offset, child.data_end)?;
                let mut bits = Vec::new();
                for _ in 0..channel_count {
                    bits.push(read_u8(data, &mut offset, child.data_end)?);
                }
                parsed.properties.insert(index, ItemProperty::PixelBits(bits));
            }
            _ => {}
        }
        index = index.saturating_add(1);
    }

    Ok(())
}

fn parse_ipma(data: &[u8], header: &BoxHeader, parsed: &mut ParsedHeif) -> Result<(), String> {
    let (version, flags, mut offset) = read_full_box(data, header)?;
    let entry_count = read_u32(data, &mut offset, header.data_end)?;
    let large_property_index = flags & 1 == 1;

    for _ in 0..entry_count {
        let item_id = if version < 1 {
            u32::from(read_u16(data, &mut offset, header.data_end)?)
        } else {
            read_u32(data, &mut offset, header.data_end)?
        };
        let association_count = read_u8(data, &mut offset, header.data_end)?;
        let mut property_indices = Vec::new();
        for _ in 0..association_count {
            let raw = if large_property_index {
                read_u16(data, &mut offset, header.data_end)?
            } else {
                u16::from(read_u8(data, &mut offset, header.data_end)?)
            };
            let property_index = if large_property_index { raw & 0x7fff } else { raw & 0x007f };
            if property_index > 0 {
                property_indices.push(property_index);
            }
        }
        parsed.associations.insert(item_id, property_indices);
    }

    Ok(())
}

fn parse_boxes(data: &[u8], start: usize, end: usize) -> Result<Vec<BoxHeader>, String> {
    let mut boxes = Vec::new();
    let mut offset = start;
    while offset + 8 <= end {
        let header = read_box(data, offset, end)?;
        if header.data_end <= offset {
            return Err("HEIC box parser made no progress".to_string());
        }
        offset = header.data_end;
        boxes.push(header);
    }

    Ok(boxes)
}

fn read_box(data: &[u8], start: usize, parent_end: usize) -> Result<BoxHeader, String> {
    let mut offset = start;
    let size32 = read_u32(data, &mut offset, parent_end)?;
    let box_type = read_fourcc(data, &mut offset, parent_end)?;
    let size = if size32 == 1 {
        read_u64(data, &mut offset, parent_end)?
    } else if size32 == 0 {
        (parent_end - start) as u64
    } else {
        u64::from(size32)
    };
    let end = start
        .checked_add(usize::try_from(size).map_err(|_| "HEIC box is too large".to_string())?)
        .ok_or_else(|| "HEIC box size overflows addressable memory".to_string())?;
    if end > parent_end || end < offset {
        return Err(format!("{box_type} box is truncated"));
    }

    Ok(BoxHeader {
        box_type,
        data_start: offset,
        data_end: end,
    })
}

fn read_full_box(data: &[u8], header: &BoxHeader) -> Result<(u8, u32, usize), String> {
    let mut offset = header.data_start;
    let version = read_u8(data, &mut offset, header.data_end)?;
    let flag_a = u32::from(read_u8(data, &mut offset, header.data_end)?);
    let flag_b = u32::from(read_u8(data, &mut offset, header.data_end)?);
    let flag_c = u32::from(read_u8(data, &mut offset, header.data_end)?);
    Ok((version, (flag_a << 16) | (flag_b << 8) | flag_c, offset))
}

fn item_properties(item_id: u32, parsed: &ParsedHeif) -> Vec<&ItemProperty> {
    parsed
        .associations
        .get(&item_id)
        .into_iter()
        .flat_map(|indices| indices.iter())
        .filter_map(|index| parsed.properties.get(index))
        .collect()
}

fn role_for_item(item_type: &str, aux_type: Option<&str>, is_auxiliary_reference: bool) -> HeifAuxiliaryRole {
    let combined = format!("{} {}", item_type.to_ascii_lowercase(), aux_type.unwrap_or_default().to_ascii_lowercase());
    if combined.contains("gain") || combined.contains("hdr") {
        HeifAuxiliaryRole::GainMap
    } else if combined.contains("portrait") {
        HeifAuxiliaryRole::PortraitEffectsMatte
    } else if combined.contains("semantic") || combined.contains("matte") {
        HeifAuxiliaryRole::SemanticMatte
    } else if combined.contains("disparity") {
        HeifAuxiliaryRole::Disparity
    } else if combined.contains("depth") || item_type == "dpth" {
        HeifAuxiliaryRole::Depth
    } else if combined.contains("alpha") || combined.contains("auxid:1") {
        HeifAuxiliaryRole::Alpha
    } else if item_type == "thmb" {
        HeifAuxiliaryRole::Thumbnail
    } else if is_auxiliary_reference {
        HeifAuxiliaryRole::Unknown
    } else {
        HeifAuxiliaryRole::Unknown
    }
}

fn is_metadata_type(item_type: &str) -> bool {
    matches!(item_type, "Exif" | "mime" | "xml " | "uri ")
}

fn hdr_gain_map_indicators(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data).to_ascii_lowercase();
    let patterns = [
        ("HDRGainMapVersion", "hdrgainmapversion"),
        ("HDRGainMap", "hdrgainmap"),
        ("gainmap", "gainmap"),
        ("Apple HDR gain-map auxiliary type", "apple:photo:2020:aux:hdrgainmap"),
        ("Apple QuickTime HDR metadata", "com.apple.quicktime.hdr"),
        ("ISO 21496 gain-map metadata", "urn:iso:std:iso:ts:21496"),
        ("ISO 21496 short marker", "iso:ts:21496"),
    ];

    let mut indicators = Vec::new();
    for (label, pattern) in patterns {
        if text.contains(pattern) && !indicators.iter().any(|value| value == label) {
            indicators.push(label.to_string());
        }
    }

    indicators
}

fn item_payload(
    data: &[u8],
    parsed: &ParsedHeif,
    item_id: u32,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, String> {
    let Some(location) = parsed.locations.get(&item_id) else {
        return Ok(None);
    };

    let mut payload = Vec::new();
    for extent in &location.extents {
        let start = usize::try_from(extent.offset).map_err(|_| "HEIC item offset is too large".to_string())?;
        let length = usize::try_from(extent.length).map_err(|_| "HEIC item length is too large".to_string())?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| "HEIC item extent overflows addressable memory".to_string())?;
        if end > data.len() {
            warnings.push(format!("HEIC item {item_id} points outside the file."));
            return Ok(Some(Vec::new()));
        }
        payload.extend_from_slice(&data[start..end]);
    }

    Ok(Some(payload))
}

fn unique_auxiliary_sidecar_path(
    output_path: &Path,
    item: &HeifAuxiliaryItem,
    used_paths: &mut HashSet<PathBuf>,
) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("converted");
    let role = role_slug(&item.role);
    let item_type = item.item_type.trim().to_ascii_lowercase();
    for index in 0.. {
        let suffix = if index == 0 {
            format!("{stem}.{role}-{}.{}.heif-item", item.item_id, item_type)
        } else {
            format!("{stem}.{role}-{}-{}.{}.heif-item", item.item_id, index, item_type)
        };
        let candidate = parent.join(suffix);
        if !candidate.exists() && used_paths.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded sidecar filename search should always return");
}

fn unique_metadata_sidecar_path(
    output_path: &Path,
    item: &HeifMetadataItem,
    used_paths: &mut HashSet<PathBuf>,
) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("converted");
    let metadata_type = item.metadata_type.trim().to_ascii_lowercase();
    let extension = metadata_extension(&item.metadata_type);
    for index in 0.. {
        let suffix = if index == 0 {
            format!("{stem}.metadata-{}.{}.{}", item.item_id, metadata_type, extension)
        } else {
            format!("{stem}.metadata-{}-{}.{}.{}", item.item_id, index, metadata_type, extension)
        };
        let candidate = parent.join(suffix);
        if !candidate.exists() && used_paths.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded metadata sidecar filename search should always return");
}

fn metadata_extension(metadata_type: &str) -> &'static str {
    match metadata_type {
        "Exif" => "exif",
        "xml " => "xml",
        "mime" => "bin",
        "uri " => "uri",
        _ => "metadata",
    }
}

fn role_slug(role: &HeifAuxiliaryRole) -> &'static str {
    match role {
        HeifAuxiliaryRole::Depth => "depth",
        HeifAuxiliaryRole::Disparity => "disparity",
        HeifAuxiliaryRole::PortraitEffectsMatte => "portrait-matte",
        HeifAuxiliaryRole::SemanticMatte => "semantic-matte",
        HeifAuxiliaryRole::GainMap => "gain-map",
        HeifAuxiliaryRole::Alpha => "alpha",
        HeifAuxiliaryRole::Thumbnail => "thumbnail",
        HeifAuxiliaryRole::Unknown => "auxiliary",
    }
}

fn read_c_string(data: &[u8], start: usize, end: usize) -> String {
    let value_end = data[start..end]
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| start + index)
        .unwrap_or(end);
    String::from_utf8_lossy(&data[start..value_end]).to_string()
}

fn read_u8(data: &[u8], offset: &mut usize, end: usize) -> Result<u8, String> {
    if *offset + 1 > end || *offset + 1 > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let value = data[*offset];
    *offset += 1;
    Ok(value)
}

fn read_u16(data: &[u8], offset: &mut usize, end: usize) -> Result<u16, String> {
    if *offset + 2 > end || *offset + 2 > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let value = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    Ok(value)
}

fn read_u32(data: &[u8], offset: &mut usize, end: usize) -> Result<u32, String> {
    if *offset + 4 > end || *offset + 4 > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let value = u32::from_be_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(value)
}

fn read_u64(data: &[u8], offset: &mut usize, end: usize) -> Result<u64, String> {
    if *offset + 8 > end || *offset + 8 > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let value = u64::from_be_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
        data[*offset + 4],
        data[*offset + 5],
        data[*offset + 6],
        data[*offset + 7],
    ]);
    *offset += 8;
    Ok(value)
}

fn read_variable(data: &[u8], offset: &mut usize, size: usize, end: usize) -> Result<u64, String> {
    if size == 0 {
        return Ok(0);
    }
    if size > 8 {
        return Err("Unsupported HEIC variable integer size".to_string());
    }
    if *offset + size > end || *offset + size > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let mut value = 0_u64;
    for _ in 0..size {
        value = (value << 8) | u64::from(data[*offset]);
        *offset += 1;
    }
    Ok(value)
}

fn read_fourcc(data: &[u8], offset: &mut usize, end: usize) -> Result<String, String> {
    if *offset + 4 > end || *offset + 4 > data.len() {
        return Err("Unexpected end of HEIC data".to_string());
    }
    let value = String::from_utf8_lossy(&data[*offset..*offset + 4]).to_string();
    *offset += 4;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_detection_finds_apple_gain_maps_and_portrait_mattes() {
        assert_eq!(
            role_for_item("hvc1", Some("urn:com:apple:photo:2020:aux:hdrgainmap"), true),
            HeifAuxiliaryRole::GainMap
        );
        assert_eq!(
            role_for_item("hvc1", Some("urn:com:apple:photo:2018:aux:portrait-effects-matte"), true),
            HeifAuxiliaryRole::PortraitEffectsMatte
        );
        assert_eq!(
            role_for_item("dpth", Some("urn:mpeg:hevc:2015:auxid:2"), true),
            HeifAuxiliaryRole::Depth
        );
    }

    #[test]
    fn sidecar_names_are_deterministic() {
        let mut used = HashSet::new();
        let item = HeifAuxiliaryItem {
            item_id: 7,
            item_type: "hvc1".to_string(),
            role: HeifAuxiliaryRole::GainMap,
            width: Some(10),
            height: Some(10),
            bit_depth: Some(10),
        };

        assert_eq!(
            unique_auxiliary_sidecar_path(Path::new("C:/tmp/photo.typeshift.jpg"), &item, &mut used),
            PathBuf::from("C:/tmp/photo.typeshift.gain-map-7.hvc1.heif-item")
        );
    }

    #[test]
    fn metadata_sidecar_names_are_deterministic() {
        let mut used = HashSet::new();
        let item = HeifMetadataItem {
            item_id: 3,
            metadata_type: "Exif".to_string(),
        };

        assert_eq!(
            unique_metadata_sidecar_path(Path::new("C:/tmp/photo.typeshift.jpg"), &item, &mut used),
            PathBuf::from("C:/tmp/photo.typeshift.metadata-3.exif.exif")
        );
    }

    #[test]
    fn hdr_gain_map_indicators_find_apple_and_iso_markers() {
        let indicators = hdr_gain_map_indicators(
            b"....HDRGainMapVersion....urn:iso:std:iso:ts:21496:-1....",
        );
        assert!(indicators.iter().any(|value| value == "HDRGainMapVersion"));
        assert!(indicators
            .iter()
            .any(|value| value == "ISO 21496 gain-map metadata"));

        let apple = hdr_gain_map_indicators(b"urn:com:apple:photo:2020:aux:hdrgainmap");
        assert!(apple
            .iter()
            .any(|value| value == "Apple HDR gain-map auxiliary type"));
    }
}
