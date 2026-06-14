use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HdrBackendStatus {
    pub available: bool,
    pub heif_aux_decoder_available: bool,
    pub gain_map_jpeg_writer_available: bool,
    pub heif_tool_path: Option<String>,
    pub vcpkg_path: Option<String>,
    pub detail: String,
    pub next_action: String,
}

pub fn inspect_hdr_backend() -> HdrBackendStatus {
    let heif_tool = find_command(if cfg!(target_os = "windows") {
        "heif-info.exe"
    } else {
        "heif-info"
    });
    let vcpkg = find_vcpkg();

    let heif_dependency_hint_available = heif_tool.is_some() || vcpkg.is_some();
    let heif_aux_decoder_available = crate::native_heif_hdr::heif_aux_decoder_linked();
    let gain_map_jpeg_writer_available = crate::hdr_jpeg_writer::gain_map_jpeg_writer_available();
    let available = heif_aux_decoder_available && gain_map_jpeg_writer_available;

    let detail = if available {
        "Native HEIC auxiliary decode and ISO 21496 / Adaptive HDR JPEG writing are available.".to_string()
    } else if gain_map_jpeg_writer_available && heif_dependency_hint_available {
        "ISO 21496 / Adaptive HDR JPEG writing is linked, and HEIC tooling was detected, but TypeShift is not linked to native HEIC auxiliary-image decoding yet."
            .to_string()
    } else if gain_map_jpeg_writer_available {
        "ISO 21496 / Adaptive HDR JPEG writing is linked, but native HEIC auxiliary-image decoding is not available yet."
            .to_string()
    } else if heif_aux_decoder_available {
        "HEIC tooling was detected, but TypeShift is not linked to an ISO 21496 / Adaptive HDR JPEG writer yet."
            .to_string()
    } else {
        "Native HEIC auxiliary decode and ISO 21496 / Adaptive HDR JPEG writing are not linked in this build."
            .to_string()
    };

    let next_action = if available {
        "Use the HDR JPEG output format to write gain-map JPEG output.".to_string()
    } else if gain_map_jpeg_writer_available && heif_dependency_hint_available {
        "Next implementation step: add a libheif-backed decoder module that extracts primary and auxiliary gain-map pixels, then map Apple HEIC gain-map metadata into ISO 21496 metadata."
            .to_string()
    } else if gain_map_jpeg_writer_available {
        "Next implementation step: bundle libheif with auxiliary-image decode support and map Apple HEIC gain-map metadata into ISO 21496 metadata."
            .to_string()
    } else if heif_aux_decoder_available {
        "Next implementation step: link a gain-map JPEG writer such as ultrajpeg/libultrahdr and map Apple HEIC gain-map metadata into ISO 21496 metadata."
            .to_string()
    } else {
        "Next implementation step: bundle libheif with auxiliary-image decode support, then link a gain-map JPEG writer such as ultrajpeg/libultrahdr."
            .to_string()
    };

    HdrBackendStatus {
        available,
        heif_aux_decoder_available,
        gain_map_jpeg_writer_available,
        heif_tool_path: heif_tool.map(|path| path.to_string_lossy().to_string()),
        vcpkg_path: vcpkg.map(|path| path.to_string_lossy().to_string()),
        detail,
        next_action,
    }
}

fn find_command(name: &str) -> Option<PathBuf> {
    if let Ok(path) = env::var("PATH") {
        for entry in env::split_paths(&path) {
            let candidate = entry.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn find_vcpkg() -> Option<PathBuf> {
    if let Ok(path) = env::var("VCPKG_ROOT") {
        let candidate = PathBuf::from(path).join(if cfg!(target_os = "windows") {
            "vcpkg.exe"
        } else {
            "vcpkg"
        });
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    for candidate in [
        PathBuf::from("C:/vcpkg/vcpkg.exe"),
        PathBuf::from("C:/vcpkg-master/vcpkg.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_backend_status_is_internally_consistent() {
        let status = inspect_hdr_backend();
        assert_eq!(
            status.available,
            status.heif_aux_decoder_available && status.gain_map_jpeg_writer_available
        );
        assert!(!status.detail.is_empty());
        assert!(!status.next_action.is_empty());
    }
}
