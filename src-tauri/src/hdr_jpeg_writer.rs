use ultrajpeg::{
    ColorGamut, ColorTransfer, CompressionEffort, EncodeOptions, GainMapBundle, GainMapMetadata,
    Image, PixelFormat,
};

#[derive(Debug, Clone, PartialEq)]
pub struct GainMapJpegInput {
    pub width: u32,
    pub height: u32,
    pub primary_rgb: Vec<u8>,
    pub gain_map_width: u32,
    pub gain_map_height: u32,
    pub gain_map_gray: Vec<u8>,
    pub primary_quality: u8,
    pub gain_map_quality: u8,
    pub hdr_headroom: f32,
}

pub fn gain_map_jpeg_writer_available() -> bool {
    encode_gain_map_jpeg(GainMapJpegInput {
        width: 1,
        height: 1,
        primary_rgb: vec![255, 255, 255],
        gain_map_width: 1,
        gain_map_height: 1,
        gain_map_gray: vec![128],
        primary_quality: 80,
        gain_map_quality: 80,
        hdr_headroom: 3.0,
    })
    .ok()
    .and_then(|bytes| ultrajpeg::inspect(&bytes).ok())
    .is_some_and(|inspection| inspection.ultra_hdr.is_some())
}

pub fn encode_gain_map_jpeg(input: GainMapJpegInput) -> Result<Vec<u8>, String> {
    validate_input(&input)?;

    let primary = Image::from_data(
        input.width,
        input.height,
        PixelFormat::Rgb8,
        ColorGamut::DisplayP3,
        ColorTransfer::Srgb,
        input.primary_rgb,
    )
    .map_err(|error| format!("Could not build HDR JPEG primary image: {error}"))?;

    let gain_map = Image::from_data(
        input.gain_map_width,
        input.gain_map_height,
        PixelFormat::Gray8,
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        input.gain_map_gray,
    )
    .map_err(|error| format!("Could not build HDR JPEG gain-map image: {error}"))?;

    ultrajpeg::encode(
        &primary,
        &EncodeOptions {
            quality: input.primary_quality,
            gain_map: Some(GainMapBundle {
                image: gain_map,
                metadata: gain_map_metadata(input.hdr_headroom),
                quality: input.gain_map_quality,
                progressive: false,
                compression: CompressionEffort::Balanced,
            }),
            ..EncodeOptions::ultra_hdr_defaults()
        },
    )
    .map_err(|error| format!("Could not encode ISO 21496 / Adaptive HDR JPEG: {error}"))
}

fn validate_input(input: &GainMapJpegInput) -> Result<(), String> {
    if input.width == 0 || input.height == 0 {
        return Err("HDR JPEG primary dimensions must be non-zero.".to_string());
    }
    if input.gain_map_width == 0 || input.gain_map_height == 0 {
        return Err("HDR JPEG gain-map dimensions must be non-zero.".to_string());
    }
    if !(1..=100).contains(&input.primary_quality) {
        return Err("HDR JPEG primary quality must be between 1 and 100.".to_string());
    }
    if !(1..=100).contains(&input.gain_map_quality) {
        return Err("HDR JPEG gain-map quality must be between 1 and 100.".to_string());
    }

    let expected_primary_len = input.width as usize * input.height as usize * 3;
    if input.primary_rgb.len() != expected_primary_len {
        return Err(format!(
            "HDR JPEG primary buffer has {} bytes; expected {expected_primary_len}.",
            input.primary_rgb.len()
        ));
    }

    let expected_gain_map_len = input.gain_map_width as usize * input.gain_map_height as usize;
    if input.gain_map_gray.len() != expected_gain_map_len {
        return Err(format!(
            "HDR JPEG gain-map buffer has {} bytes; expected {expected_gain_map_len}.",
            input.gain_map_gray.len()
        ));
    }

    Ok(())
}

fn gain_map_metadata(hdr_headroom: f32) -> GainMapMetadata {
    let headroom = hdr_headroom.clamp(1.01, 8.0);
    GainMapMetadata {
        min_content_boost: [1.0; 3],
        max_content_boost: [headroom; 3],
        gamma: [1.0; 3],
        offset_sdr: [1.0 / 64.0; 3],
        offset_hdr: [1.0 / 64.0; 3],
        hdr_capacity_min: 1.0,
        hdr_capacity_max: headroom,
        use_base_color_space: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_inspectable_gain_map_jpeg() {
        let jpeg = encode_gain_map_jpeg(GainMapJpegInput {
            width: 2,
            height: 2,
            primary_rgb: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            gain_map_width: 2,
            gain_map_height: 2,
            gain_map_gray: vec![0, 64, 128, 255],
            primary_quality: 92,
            gain_map_quality: 85,
            hdr_headroom: 3.25,
        })
        .unwrap();

        let inspection = ultrajpeg::inspect(&jpeg).unwrap();
        assert!(inspection.ultra_hdr.is_some());
        assert!(inspection.gain_map_jpeg_len.is_some_and(|length| length > 0));
        let metadata = inspection.ultra_hdr.unwrap().gain_map_metadata.unwrap();
        assert!(metadata.hdr_capacity_max > 3.0);
        assert!(metadata.max_content_boost[0] > 3.0);
    }

    #[test]
    fn rejects_mismatched_buffers() {
        let error = encode_gain_map_jpeg(GainMapJpegInput {
            width: 2,
            height: 2,
            primary_rgb: vec![255, 0, 0],
            gain_map_width: 2,
            gain_map_height: 2,
            gain_map_gray: vec![0, 64, 128, 255],
            primary_quality: 92,
            gain_map_quality: 85,
            hdr_headroom: 3.0,
        })
        .unwrap_err();

        assert!(error.contains("primary buffer"));
    }
}
