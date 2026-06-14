use super::{ConvertRequest, Dimensions, HeifInspection, OutputFormat, Preset};
use std::{
    fmt,
    os::windows::ffi::OsStrExt,
    path::Path,
};
use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, RPC_E_CHANGED_MODE},
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_ContainerFormatPng,
            GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA,
            IWICBitmapFrameDecode, IWICBitmapSource, IWICColorContext,
            IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
            WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        },
    },
};

#[derive(Debug)]
pub struct WicError {
    message: String,
}

impl WicError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn codec_context(action: &str, error: windows::core::Error) -> Self {
        Self::new(format!(
            "{action} through Windows WIC failed: {error}. Install Microsoft's HEIF Image Extensions and HEVC Video Extensions, or use the future bundled libheif backend."
        ))
    }
}

impl fmt::Display for WicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WicError {}

pub fn inspect_heif(path: &Path) -> Result<HeifInspection, WicError> {
    unsafe {
        ensure_com_initialized()?;
        let factory = create_factory()?;
        let decoder = factory
            .CreateDecoderFromFilename(
                path_to_pcwstr(path).as_pcwstr(),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|error| WicError::codec_context("Opening HEIC", error))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|error| WicError::codec_context("Reading first HEIC frame", error))?;
        let mut width = 0;
        let mut height = 0;
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|error| WicError::codec_context("Reading HEIC dimensions", error))?;
        let color_contexts = color_contexts(&factory, &frame);
        let color_profile_type = if color_contexts.is_empty() {
            None
        } else {
            Some("Windows WIC color context".to_string())
        };
        let mut warnings = vec![
            "HEIC decoded through Windows WIC. This is a local fallback and not the final bundled libheif/LittleCMS backend.".to_string(),
        ];
        if color_contexts.is_empty() {
            warnings.push("No embedded color context was reported by Windows WIC.".to_string());
        }

        Ok(HeifInspection {
            dimensions: Some(Dimensions { width, height }),
            bit_depth: Some(8),
            has_alpha: true,
            color_profile_name: color_profile_type.clone(),
            color_profile_type,
            warnings,
        })
    }
}

pub fn convert_heif(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertRequest,
) -> Result<Vec<String>, WicError> {
    if !matches!(request.output_format, OutputFormat::Png | OutputFormat::Jpeg) {
        return Err(WicError::new(
            "HEIC conversion through the interim Windows WIC backend currently supports PNG and JPEG output only. Convert HEIC to PNG first, then convert that PNG to the other target format.",
        ));
    }

    unsafe {
        ensure_com_initialized()?;
        let factory = create_factory()?;
        let decoder = factory
            .CreateDecoderFromFilename(
                path_to_pcwstr(input_path).as_pcwstr(),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|error| WicError::codec_context("Opening HEIC", error))?;
        let frame = decoder
            .GetFrame(0)
            .map_err(|error| WicError::codec_context("Reading first HEIC frame", error))?;
        let source: IWICBitmapSource = frame
            .cast()
            .map_err(|error| WicError::codec_context("Preparing HEIC bitmap source", error))?;
        let color_contexts = color_contexts(&factory, &frame);
        let converter = factory
            .CreateFormatConverter()
            .map_err(|error| WicError::codec_context("Creating pixel format converter", error))?;
        let target_pixel_format = match request.output_format {
            OutputFormat::Png => GUID_WICPixelFormat32bppBGRA,
            OutputFormat::Jpeg => GUID_WICPixelFormat24bppBGR,
            OutputFormat::AdaptiveHdrJpeg | OutputFormat::Webp | OutputFormat::Tiff | OutputFormat::Bmp => unreachable!(
                "non-PNG/JPEG HEIC output is rejected before WIC encoder selection"
            ),
        };
        converter
            .Initialize(
                &source,
                &target_pixel_format,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|error| WicError::codec_context("Converting HEIC pixels", error))?;
        let converted: IWICBitmapSource = converter
            .cast()
            .map_err(|error| WicError::codec_context("Preparing converted bitmap source", error))?;
        let mut width = 0;
        let mut height = 0;
        converted
            .GetSize(&mut width, &mut height)
            .map_err(|error| WicError::codec_context("Reading converted dimensions", error))?;

        let container = match request.output_format {
            OutputFormat::Png => GUID_ContainerFormatPng,
            OutputFormat::Jpeg => GUID_ContainerFormatJpeg,
            OutputFormat::AdaptiveHdrJpeg | OutputFormat::Webp | OutputFormat::Tiff | OutputFormat::Bmp => unreachable!(
                "non-PNG/JPEG HEIC output is rejected before WIC container selection"
            ),
        };

        let stream = factory
            .CreateStream()
            .map_err(|error| WicError::codec_context("Creating output stream", error))?;
        stream
            .InitializeFromFilename(path_to_pcwstr(output_path).as_pcwstr(), GENERIC_WRITE.0)
            .map_err(|error| WicError::codec_context("Opening output file", error))?;
        let encoder = factory
            .CreateEncoder(&container, std::ptr::null())
            .map_err(|error| WicError::codec_context("Creating output encoder", error))?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .map_err(|error| WicError::codec_context("Initializing output encoder", error))?;

        if !color_contexts.is_empty() {
            let _ = encoder.SetColorContexts(&color_contexts);
        }

        let mut frame_encode = None;
        let mut options = None;
        encoder
            .CreateNewFrame(&mut frame_encode, &mut options)
            .map_err(|error| WicError::codec_context("Creating output frame", error))?;
        let frame_encode =
            frame_encode.ok_or_else(|| WicError::new("Windows WIC did not return an output frame."))?;
        frame_encode
            .Initialize(options.as_ref())
            .map_err(|error| WicError::codec_context("Initializing output frame", error))?;
        frame_encode
            .SetSize(width, height)
            .map_err(|error| WicError::codec_context("Setting output dimensions", error))?;
        let mut pixel_format = target_pixel_format;
        frame_encode
            .SetPixelFormat(&mut pixel_format)
            .map_err(|error| WicError::codec_context("Setting output pixel format", error))?;
        if !color_contexts.is_empty() {
            let _ = frame_encode.SetColorContexts(&color_contexts);
        }
        frame_encode
            .WriteSource(&converted, std::ptr::null())
            .map_err(|error| WicError::codec_context("Writing converted pixels", error))?;
        frame_encode
            .Commit()
            .map_err(|error| WicError::codec_context("Committing output frame", error))?;
        encoder
            .Commit()
            .map_err(|error| WicError::codec_context("Committing output file", error))?;

        let mut warnings = vec![
            "HEIC converted through Windows WIC. This is a local fallback and not the final bundled libheif/LittleCMS backend.".to_string(),
        ];
        if matches!(request.output_format, OutputFormat::Jpeg) {
            warnings.push("JPEG output flattens or drops alpha because JPEG has no alpha channel.".to_string());
        }
        if matches!(request.preset, Preset::SrgbCompatible) {
            warnings.push("sRGB-compatible mode is routed through WIC in this fallback backend; LittleCMS validation is still pending.".to_string());
        }
        if color_contexts.is_empty() {
            warnings.push("No embedded color context was reported by Windows WIC; output may be treated as sRGB by viewers.".to_string());
        }
        Ok(warnings)
    }
}

unsafe fn ensure_com_initialized() -> Result<(), WicError> {
    let result = CoInitializeEx(None, COINIT_MULTITHREADED);
    if result.is_ok() || result == RPC_E_CHANGED_MODE {
        Ok(())
    } else {
        Err(WicError::new(format!("Could not initialize Windows COM: {result:?}")))
    }
}

unsafe fn create_factory() -> Result<IWICImagingFactory, WicError> {
    CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
        .map_err(|error| WicError::codec_context("Creating Windows WIC factory", error))
}

unsafe fn color_contexts(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Vec<Option<IWICColorContext>> {
    let Ok(first_context) = factory.CreateColorContext() else {
        return Vec::new();
    };
    let mut contexts = vec![Some(first_context)];
    let mut actual_count = 0;
    if frame
        .GetColorContexts(&mut contexts, &mut actual_count)
        .is_ok()
        && actual_count > 0
    {
        contexts.truncate(actual_count as usize);
        contexts
    } else {
        Vec::new()
    }
}

struct WidePath {
    buffer: Vec<u16>,
}

impl WidePath {
    fn as_pcwstr(&self) -> PCWSTR {
        PCWSTR(self.buffer.as_ptr())
    }
}

fn path_to_pcwstr(path: &Path) -> WidePath {
    let mut buffer: Vec<u16> = path.as_os_str().encode_wide().collect();
    buffer.push(0);
    WidePath { buffer }
}
