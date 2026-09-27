use crate::image_engine::{ConflictPolicy, DestinationPolicy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::hash_map::DefaultHasher,
    env,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};

const SUPPORTED_INPUT_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "oga", "opus", "m4a", "aac", "wma", "aiff", "aif", "amr",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInspection {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub container: Option<String>,
    pub duration_seconds: Option<f64>,
    pub codec: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub channels: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub has_cover_art: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioOutputFormat {
    Mp3,
    Wav,
    Flac,
    M4a,
    Ogg,
    Opus,
}

impl AudioOutputFormat {
    fn extension(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::M4a => "m4a",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::M4a => "M4A",
            Self::Ogg => "OGG",
            Self::Opus => "OPUS",
        }
    }

    fn is_lossless(&self) -> bool {
        matches!(self, Self::Wav | Self::Flac)
    }

    fn required_encoder(&self) -> Option<&'static str> {
        match self {
            Self::Mp3 => Some("libmp3lame"),
            Self::M4a => Some("aac"),
            Self::Ogg => Some("libvorbis"),
            Self::Opus => Some("libopus"),
            Self::Wav | Self::Flac => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioQuality {
    High,
    Medium,
    Low,
}

impl AudioQuality {
    fn mp3_bitrate(&self) -> &'static str {
        match self {
            Self::High => "320k",
            Self::Medium => "192k",
            Self::Low => "128k",
        }
    }

    fn aac_bitrate(&self) -> &'static str {
        match self {
            Self::High => "256k",
            Self::Medium => "192k",
            Self::Low => "128k",
        }
    }

    fn opus_bitrate(&self) -> &'static str {
        match self {
            Self::High => "192k",
            Self::Medium => "128k",
            Self::Low => "96k",
        }
    }

    fn vorbis_quality(&self) -> &'static str {
        match self {
            Self::High => "8",
            Self::Medium => "5",
            Self::Low => "2",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::High => "High quality",
            Self::Medium => "Medium quality",
            Self::Low => "Low quality",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConvertAudioRequest {
    pub input_paths: Vec<String>,
    pub output_format: AudioOutputFormat,
    pub quality: AudioQuality,
    pub destination_policy: DestinationPolicy,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioResultStatus {
    Completed,
    Failed,
    Cancelled,
}

impl AudioResultStatus {
    pub fn is_terminal_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioConversionResult {
    pub input_path: String,
    pub output_path: Option<String>,
    pub status: AudioResultStatus,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AudioJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioConversionJob {
    pub job_id: String,
    pub status: AudioJobStatus,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<AudioConversionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioBackendStatus {
    pub available: bool,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub libmp3lame_available: bool,
    pub aac_available: bool,
    pub libvorbis_available: bool,
    pub flac_available: bool,
    pub libopus_available: bool,
    pub detail: String,
    pub next_action: String,
}

pub fn inspect_audio_backend() -> AudioBackendStatus {
    let ffmpeg = find_command(ffmpeg_name());
    let ffprobe = find_command(ffprobe_name());
    let (libmp3lame_available, aac_available, libvorbis_available, flac_available, libopus_available) =
        match ffmpeg.as_deref() {
            Some(path) => scan_encoders(path),
            None => (false, false, false, false, false),
        };
    let available = ffmpeg.is_some() && ffprobe.is_some();

    let detail = if available {
        "FFmpeg and FFprobe were found. Audio inspection and conversion are ready.".to_string()
    } else if ffmpeg.is_some() {
        "FFmpeg was found but FFprobe is missing; audio inspection is limited.".to_string()
    } else {
        "FFmpeg was not found next to the app or on PATH; audio conversion is unavailable."
            .to_string()
    };

    let next_action = if available {
        "Choose an audio file, pick an output format, and convert.".to_string()
    } else {
        "TypeShift ships FFmpeg in its ffmpeg folder; if it is missing there, install FFmpeg (for example via winget install ffmpeg) and restart TypeShift."
            .to_string()
    };

    AudioBackendStatus {
        available,
        ffmpeg_path: ffmpeg.map(|path| path.to_string_lossy().to_string()),
        ffprobe_path: ffprobe.map(|path| path.to_string_lossy().to_string()),
        libmp3lame_available,
        aac_available,
        libvorbis_available,
        flac_available,
        libopus_available,
        detail,
        next_action,
    }
}

pub fn inspect_audio(path: &Path) -> AudioInspection {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let format = detect_format(path);
    let mut warnings = Vec::new();

    let Some(ffprobe) = find_command(ffprobe_name()) else {
        warnings.push(
            "FFprobe was not found; audio details are limited to the file name. Install FFmpeg to inspect audio files."
                .to_string(),
        );
        return AudioInspection {
            path: path.to_string_lossy().to_string(),
            file_name,
            format,
            container: None,
            duration_seconds: None,
            codec: None,
            sample_rate_hz: None,
            channels: None,
            bitrate_kbps: None,
            has_cover_art: false,
            warnings,
        };
    };

    match probe_audio(&ffprobe, path) {
        Ok(probe) => {
            warnings.extend(probe.warnings);
            AudioInspection {
                path: path.to_string_lossy().to_string(),
                file_name,
                format,
                container: probe.container,
                duration_seconds: probe.duration_seconds,
                codec: probe.codec,
                sample_rate_hz: probe.sample_rate_hz,
                channels: probe.channels,
                bitrate_kbps: probe.bitrate_kbps,
                has_cover_art: probe.has_cover_art,
                warnings,
            }
        }
        Err(error) => {
            warnings.push(format!("Could not probe audio details: {error}"));
            AudioInspection {
                path: path.to_string_lossy().to_string(),
                file_name,
                format,
                container: None,
                duration_seconds: None,
                codec: None,
                sample_rate_hz: None,
                channels: None,
                bitrate_kbps: None,
                has_cover_art: false,
                warnings,
            }
        }
    }
}

pub fn convert_audio(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertAudioRequest,
) -> Result<Vec<String>, String> {
    if !is_audio_like(input_path) {
        return Err(format!(
            "{} is not a supported audio input. Supported extensions: {}.",
            input_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("This file"),
            SUPPORTED_INPUT_EXTENSIONS.join(", ")
        ));
    }

    let Some(ffmpeg) = find_command(ffmpeg_name()) else {
        return Err("FFmpeg was not found next to the app or on PATH. Install FFmpeg and restart TypeShift to convert audio files.".to_string());
    };

    let args = build_ffmpeg_args(input_path, output_path, request);
    let output = no_console_command(&ffmpeg)
        .args(&args)
        .output()
        .map_err(|error| format!("Could not start FFmpeg: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first_error_line = stderr
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        let message = if first_error_line.trim().is_empty() {
            "FFmpeg exited with an error but produced no readable message.".to_string()
        } else {
            first_error_line
        };
        return Err(message);
    }

    if !output_path.exists() {
        return Err("FFmpeg reported success but did not write the output file.".to_string());
    }

    let mode_description = if request.output_format.is_lossless() {
        format!(
            "Encoded with FFmpeg as {} (lossless)",
            request.output_format.label()
        )
    } else {
        format!(
            "Encoded with FFmpeg as {} ({})",
            request.output_format.label(),
            request.quality.label()
        )
    };

    let mut warnings = vec![mode_description];
    if request.output_format.is_lossless() {
        warnings.push(
            "Lossless output preserves the decoded audio exactly; the quality preset is ignored."
                .to_string(),
        );
    } else {
        warnings.push(
            "The audio stream was re-encoded with the selected quality settings.".to_string(),
        );
    }
    if request.output_format == AudioOutputFormat::Opus {
        warnings.push(
            "Opus always encodes at 48 kHz; the source sample rate was resampled automatically."
                .to_string(),
        );
    }

    Ok(warnings)
}

pub fn create_audio_preview_waveform(input_path: &Path) -> Result<PathBuf, String> {
    let preview_path = preview_cache_path(input_path)?;
    if preview_path.exists()
        && fs::metadata(&preview_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
    {
        return Ok(preview_path);
    }
    if preview_path.exists() {
        let _ = fs::remove_file(&preview_path);
    }

    let Some(ffmpeg) = find_command(ffmpeg_name()) else {
        return Err("FFmpeg was not found next to the app or on PATH; audio waveform preview is unavailable.".to_string());
    };

    let args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-filter_complex".to_string(),
        "[0:a:0]showwavespic=s=720x240:colors=#55c7b8[w]".to_string(),
        "-map".to_string(),
        "[w]".to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-y".to_string(),
        preview_path.to_string_lossy().to_string(),
    ];

    let output = no_console_command(&ffmpeg)
        .args(&args)
        .output()
        .map_err(|error| format!("Could not start FFmpeg for waveform preview: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if message.trim().is_empty() {
            "FFmpeg could not render a waveform for this audio file.".to_string()
        } else {
            message
        });
    }

    if !preview_path.exists() {
        return Err("FFmpeg did not write a waveform preview.".to_string());
    }
    Ok(preview_path)
}

pub fn safe_output_path(input_path: &Path, output_format: &AudioOutputFormat) -> PathBuf {
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("converted");
    let extension = output_format.extension();
    let first = parent.join(format!("{stem}.typeshift.{extension}"));
    if !first.exists() {
        return first;
    }

    for index in 1.. {
        let candidate = parent.join(format!("{stem}.typeshift-{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded output filename search should always return");
}

pub fn is_audio_like(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .map(|value| SUPPORTED_INPUT_EXTENSIONS.contains(&value.as_str()))
        .unwrap_or(false)
}

fn build_ffmpeg_args(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertAudioRequest,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-map".to_string(),
        "0:a:0".to_string(),
    ];

    match request.output_format {
        AudioOutputFormat::Mp3 => {
            args.extend([
                "-c:a".to_string(),
                "libmp3lame".to_string(),
                "-b:a".to_string(),
                request.quality.mp3_bitrate().to_string(),
            ]);
        }
        AudioOutputFormat::Wav => {
            args.extend(["-c:a".to_string(), "pcm_s16le".to_string()]);
        }
        AudioOutputFormat::Flac => {
            args.extend(["-c:a".to_string(), "flac".to_string()]);
        }
        AudioOutputFormat::M4a => {
            args.extend([
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                request.quality.aac_bitrate().to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
            ]);
        }
        AudioOutputFormat::Ogg => {
            args.extend([
                "-c:a".to_string(),
                "libvorbis".to_string(),
                "-q:a".to_string(),
                request.quality.vorbis_quality().to_string(),
            ]);
        }
        AudioOutputFormat::Opus => {
            args.extend([
                "-c:a".to_string(),
                "libopus".to_string(),
                "-b:a".to_string(),
                request.quality.opus_bitrate().to_string(),
            ]);
        }
    }

    args.extend([
        "-n".to_string(),
        output_path.to_string_lossy().to_string(),
    ]);
    args
}

struct ProbedAudio {
    container: Option<String>,
    duration_seconds: Option<f64>,
    codec: Option<String>,
    sample_rate_hz: Option<u32>,
    channels: Option<u32>,
    bitrate_kbps: Option<u32>,
    has_cover_art: bool,
    warnings: Vec<String>,
}

fn probe_audio(ffprobe: &Path, path: &Path) -> Result<ProbedAudio, String> {
    let output = no_console_command(ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("Could not run FFprobe: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if message.trim().is_empty() {
            "FFprobe could not read this file as audio.".to_string()
        } else {
            message
        });
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Could not parse FFprobe output: {error}"))?;
    parse_probe_value(&value)
}

fn parse_probe_value(value: &Value) -> Result<ProbedAudio, String> {
    let format = &value["format"];
    let streams = value["streams"].as_array().cloned().unwrap_or_default();
    let mut warnings = Vec::new();

    let container = format["format_name"]
        .as_str()
        .map(|name| name.split(',').next().unwrap_or(name).to_string());
    let duration_seconds = format["duration"]
        .as_str()
        .and_then(|duration| duration.parse::<f64>().ok())
        .or_else(|| {
            streams
                .iter()
                .find_map(|stream| stream["duration"].as_str())
                .and_then(|duration| duration.parse::<f64>().ok())
        });

    let audio_stream = streams.iter().find(|stream| stream["codec_type"] == "audio");
    let has_cover_art = streams.iter().any(|stream| {
        stream["codec_type"] == "video"
            && stream["disposition"]["attached_pic"].as_i64() == Some(1)
    });

    let codec = audio_stream
        .and_then(|stream| stream["codec_name"].as_str())
        .map(|codec| codec.to_string());
    let sample_rate_hz = audio_stream
        .and_then(|stream| stream["sample_rate"].as_str())
        .and_then(|rate| rate.parse::<u32>().ok());
    let channels = audio_stream
        .and_then(|stream| stream["channels"].as_u64())
        .map(|channels| channels as u32);
    let bitrate_kbps = audio_stream
        .and_then(|stream| stream["bit_rate"].as_str())
        .and_then(|bitrate| bitrate.parse::<u64>().ok())
        .or_else(|| {
            format["bit_rate"]
                .as_str()
                .and_then(|bitrate| bitrate.parse::<u64>().ok())
        })
        .map(|bits_per_second| (bits_per_second / 1000).min(u32::MAX as u64) as u32);

    if streams.is_empty() {
        warnings.push("FFprobe found no media streams in this file.".to_string());
    }
    if audio_stream.is_none() {
        warnings.push("No audio stream was found; this file may be video-only or silent.".to_string());
    }
    if has_cover_art {
        warnings.push(
            "Embedded cover art was detected. TypeShift converts the audio stream only, so cover art is not carried into the output."
                .to_string(),
        );
    }

    Ok(ProbedAudio {
        container,
        duration_seconds,
        codec,
        sample_rate_hz,
        channels,
        bitrate_kbps,
        has_cover_art,
        warnings,
    })
}

fn scan_encoders(ffmpeg: &Path) -> (bool, bool, bool, bool, bool) {
    let Ok(output) = no_console_command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return (false, false, false, false, false);
    };
    let text = String::from_utf8_lossy(&output.stdout);
    (
        encoder_present(&text, "libmp3lame"),
        encoder_present(&text, "aac"),
        encoder_present(&text, "libvorbis"),
        encoder_present(&text, "flac"),
        encoder_present(&text, "libopus"),
    )
}

fn encoder_present(encoders_text: &str, name: &str) -> bool {
    encoders_text
        .lines()
        .any(|line| line.split_whitespace().any(|token| token == name))
}

/// Build a command that spawns without a visible console window on Windows,
/// so bundled console binaries like FFmpeg never flash a terminal.
fn no_console_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: run the child with no console window.
        command.creation_flags(0x0800_0000);
    }
    command
}

fn find_command(name: &str) -> Option<PathBuf> {
    // Prefer the FFmpeg copy bundled next to the executable, which installers
    // ship inside the `ffmpeg` folder beside the app binary.
    if let Some(path) = bundled_tool_path(name) {
        if path.is_file() {
            return Some(path);
        }
    }

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

/// Resolve where a bundled FFmpeg tool should live next to the running
/// executable: inside the `ffmpeg` folder first, then directly beside the
/// executable as a fallback for manual copies. Debug builds also consult the
/// source-tree `bin/ffmpeg` folder so `tauri dev` finds the bundled tools.
fn bundled_tool_path(name: &str) -> Option<PathBuf> {
    let exe_dir = env::current_exe().ok()?.parent()?.to_path_buf();
    let in_subfolder = exe_dir.join("ffmpeg").join(name);
    if in_subfolder.is_file() {
        return Some(in_subfolder);
    }

    #[cfg(debug_assertions)]
    {
        let dev_folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join("ffmpeg")
            .join(name);
        if dev_folder.is_file() {
            return Some(dev_folder);
        }
    }

    Some(exe_dir.join(name))
}

fn ffmpeg_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn ffprobe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }
}

fn detect_format(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_uppercase())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}

fn preview_cache_path(input_path: &Path) -> Result<PathBuf, String> {
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

    let cache_dir = std::env::temp_dir().join("typeshift-audio-previews");
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create audio preview cache: {error}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(output_format: AudioOutputFormat) -> ConvertAudioRequest {
        ConvertAudioRequest {
            input_paths: vec!["C:/tmp/song.mp3".to_string()],
            output_format,
            quality: AudioQuality::Medium,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
        }
    }

    #[test]
    fn safe_output_path_uses_same_folder_and_typeshift_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("song.flac");

        assert_eq!(
            safe_output_path(&input, &AudioOutputFormat::Mp3),
            dir.path().join("song.typeshift.mp3")
        );
    }

    #[test]
    fn safe_output_path_auto_numbers_existing_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("song.wav");
        fs::write(dir.path().join("song.typeshift.opus"), b"existing").unwrap();
        fs::write(dir.path().join("song.typeshift-1.opus"), b"existing").unwrap();

        assert_eq!(
            safe_output_path(&input, &AudioOutputFormat::Opus),
            dir.path().join("song.typeshift-2.opus")
        );
    }

    #[test]
    fn mp3_args_encode_libmp3lame_with_bitrate_and_never_overwrite() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.wav"),
            Path::new("C:/tmp/song.typeshift.mp3"),
            &request(AudioOutputFormat::Mp3),
        );

        assert!(args.iter().any(|arg| arg == "0:a:0"));
        assert!(args.iter().any(|arg| arg == "libmp3lame"));
        assert!(args.iter().any(|arg| arg == "192k"));
        assert!(args.iter().any(|arg| arg == "-n"));
        assert!(args.iter().any(|arg| arg == "C:/tmp/song.typeshift.mp3"));
        assert!(!args.iter().any(|arg| arg == "+faststart"));
    }

    #[test]
    fn wav_args_use_pcm_and_no_quality_flags() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.mp3"),
            Path::new("C:/tmp/song.typeshift.wav"),
            &request(AudioOutputFormat::Wav),
        );

        assert!(args.iter().any(|arg| arg == "pcm_s16le"));
        assert!(!args.iter().any(|arg| arg == "-b:a"));
        assert!(!args.iter().any(|arg| arg == "-q:a"));
    }

    #[test]
    fn flac_args_use_flac_encoder() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.mp3"),
            Path::new("C:/tmp/song.typeshift.flac"),
            &request(AudioOutputFormat::Flac),
        );

        assert!(args.iter().any(|arg| arg == "flac"));
        assert!(!args.iter().any(|arg| arg == "-b:a"));
    }

    #[test]
    fn m4a_args_use_aac_with_faststart() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.mp3"),
            Path::new("C:/tmp/song.typeshift.m4a"),
            &request(AudioOutputFormat::M4a),
        );

        assert!(args.iter().any(|arg| arg == "aac"));
        assert!(args.iter().any(|arg| arg == "+faststart"));
    }

    #[test]
    fn ogg_args_use_vorbis_quality_scale() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.mp3"),
            Path::new("C:/tmp/song.typeshift.ogg"),
            &request(AudioOutputFormat::Ogg),
        );

        assert!(args.iter().any(|arg| arg == "libvorbis"));
        assert!(args.iter().any(|arg| arg == "-q:a"));
        assert!(args.iter().any(|arg| arg == "5"));
    }

    #[test]
    fn opus_args_use_libopus_bitrate() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/song.mp3"),
            Path::new("C:/tmp/song.typeshift.opus"),
            &request(AudioOutputFormat::Opus),
        );

        assert!(args.iter().any(|arg| arg == "libopus"));
        assert!(args.iter().any(|arg| arg == "128k"));
    }

    #[test]
    fn is_audio_like_matches_common_extensions() {
        assert!(is_audio_like(Path::new("song.mp3")));
        assert!(is_audio_like(Path::new("song.FLAC")));
        assert!(is_audio_like(Path::new("song.m4a")));
        assert!(!is_audio_like(Path::new("clip.mp4")));
        assert!(!is_audio_like(Path::new("photo.png")));
    }

    #[test]
    fn parses_probe_json_with_codec_details_and_cover_art() {
        let value: Value = serde_json::from_str(
            r#"{
                "format": {
                    "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
                    "duration": "12.5",
                    "bit_rate": "256000"
                },
                "streams": [
                    {
                        "codec_type": "video",
                        "codec_name": "mjpeg",
                        "disposition": { "attached_pic": 1 }
                    },
                    {
                        "codec_type": "audio",
                        "codec_name": "aac",
                        "sample_rate": "44100",
                        "channels": 2,
                        "bit_rate": "255000"
                    }
                ]
            }"#,
        )
        .unwrap();

        let probe = parse_probe_value(&value).unwrap();
        assert_eq!(probe.container.as_deref(), Some("mov"));
        assert_eq!(probe.duration_seconds, Some(12.5));
        assert_eq!(probe.codec.as_deref(), Some("aac"));
        assert_eq!(probe.sample_rate_hz, Some(44100));
        assert_eq!(probe.channels, Some(2));
        assert_eq!(probe.bitrate_kbps, Some(255));
        assert!(probe.has_cover_art);
        assert!(probe
            .warnings
            .iter()
            .any(|warning| warning.contains("cover art")));
    }

    #[test]
    fn parses_probe_json_without_audio_stream() {
        let value: Value = serde_json::from_str(
            r#"{
                "format": { "format_name": "wav" },
                "streams": []
            }"#,
        )
        .unwrap();

        let probe = parse_probe_value(&value).unwrap();
        assert_eq!(probe.codec, None);
        assert!(!probe.has_cover_art);
        assert!(probe
            .warnings
            .iter()
            .any(|warning| warning.contains("No audio stream")));
    }

    #[test]
    fn encoder_scan_detects_known_encoders() {
        let sample = "\n A....D libmp3lame           MP3 (LAME)\n \
                      A.....D aac                  AAC (Advanced Audio Coding)\n \
                      A.....D libvorbis            libvorbis\n \
                      A.....D flac                 FLAC (Free Lossless Audio Codec)\n \
                      A.....D libopus              libopus Opus\n";
        assert!(encoder_present(sample, "libmp3lame"));
        assert!(encoder_present(sample, "aac"));
        assert!(encoder_present(sample, "libvorbis"));
        assert!(encoder_present(sample, "flac"));
        assert!(encoder_present(sample, "libopus"));
        assert!(!encoder_present(sample, "libmp3lame-hq"));
    }

    #[test]
    fn lossless_formats_skip_required_encoders() {
        assert_eq!(AudioOutputFormat::Wav.required_encoder(), None);
        assert_eq!(AudioOutputFormat::Flac.required_encoder(), None);
        assert_eq!(
            AudioOutputFormat::Mp3.required_encoder(),
            Some("libmp3lame")
        );
    }

    #[test]
    fn audio_backend_status_is_internally_consistent() {
        let status = inspect_audio_backend();
        assert_eq!(
            status.available,
            status.ffmpeg_path.is_some() && status.ffprobe_path.is_some()
        );
        assert!(!status.detail.is_empty());
        assert!(!status.next_action.is_empty());
    }

    #[test]
    fn bundled_ffmpeg_makes_backend_available() {
        // When the bundled ffmpeg folder ships with the app (source tree in
        // debug builds, beside the executable in release builds), the backend
        // must resolve it without relying on PATH. Skipped when the bundled
        // binaries are absent, e.g. on a fresh clone without the payload.
        let bundled_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join("ffmpeg");
        if !bundled_dir.join(ffmpeg_name()).is_file() {
            return;
        }

        let status = inspect_audio_backend();
        assert!(
            status.available,
            "bundled FFmpeg was not detected: {}",
            status.detail
        );
        assert_eq!(
            status.ffmpeg_path.as_deref(),
            Some(bundled_dir.join(ffmpeg_name()).to_str().unwrap())
        );
    }
}
