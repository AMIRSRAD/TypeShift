use crate::image_engine::{ConflictPolicy, DestinationPolicy};
use serde::{Deserialize, Serialize};
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
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "mpg", "mpeg", "wmv", "flv", "ts", "3gp",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoInspection {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub container: Option<String>,
    pub duration_seconds: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub fps: Option<f64>,
    pub has_audio: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoOutputFormat {
    Mp4,
    Webm,
    Mkv,
    Gif,
}

impl VideoOutputFormat {
    fn extension(&self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Mkv => "mkv",
            Self::Gif => "gif",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Webm => "WebM",
            Self::Mkv => "MKV",
            Self::Gif => "GIF",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoQuality {
    High,
    Medium,
    Low,
}

impl VideoQuality {
    fn crf_h264(&self) -> &'static str {
        match self {
            Self::High => "18",
            Self::Medium => "23",
            Self::Low => "28",
        }
    }

    fn crf_vp9(&self) -> &'static str {
        match self {
            Self::High => "32",
            Self::Medium => "36",
            Self::Low => "40",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoScalePolicy {
    Original,
    Height1080,
    Height720,
    Height480,
}

impl VideoScalePolicy {
    fn scale_filter(&self) -> Option<String> {
        match self {
            Self::Original => None,
            Self::Height1080 => Some("scale=-2:1080".to_string()),
            Self::Height720 => Some("scale=-2:720".to_string()),
            Self::Height480 => Some("scale=-2:480".to_string()),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Height1080 => "1080p",
            Self::Height720 => "720p",
            Self::Height480 => "480p",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConvertVideoRequest {
    pub input_paths: Vec<String>,
    pub output_format: VideoOutputFormat,
    pub quality: VideoQuality,
    pub scale: VideoScalePolicy,
    pub destination_policy: DestinationPolicy,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoResultStatus {
    Completed,
    Failed,
    Cancelled,
}

impl VideoResultStatus {
    pub fn is_terminal_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoConversionResult {
    pub input_path: String,
    pub output_path: Option<String>,
    pub status: VideoResultStatus,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VideoJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoConversionJob {
    pub job_id: String,
    pub status: VideoJobStatus,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<VideoConversionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoBackendStatus {
    pub available: bool,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub libx264_available: bool,
    pub libvpx_vp9_available: bool,
    pub libopus_available: bool,
    pub aac_available: bool,
    pub detail: String,
    pub next_action: String,
}

pub fn inspect_video_backend() -> VideoBackendStatus {
    let ffmpeg = find_command(ffmpeg_name());
    let ffprobe = find_command(ffprobe_name());
    let (libx264_available, libvpx_vp9_available, libopus_available, aac_available) =
        match ffmpeg.as_deref() {
            Some(path) => scan_encoders(path),
            None => (false, false, false, false),
        };
    let available = ffmpeg.is_some() && ffprobe.is_some();

    let detail = if available {
        "FFmpeg and FFprobe were found. Video inspection and conversion are ready."
            .to_string()
    } else if ffmpeg.is_some() {
        "FFmpeg was found but FFprobe is missing; video inspection is limited.".to_string()
    } else {
        "FFmpeg was not found next to the app or on PATH; video conversion is unavailable."
            .to_string()
    };

    let next_action = if available {
        "Choose a video, pick an output format, and convert.".to_string()
    } else {
        "TypeShift ships FFmpeg in its ffmpeg folder; if it is missing there, install FFmpeg (for example via winget install ffmpeg) and restart TypeShift."
            .to_string()
    };

    VideoBackendStatus {
        available,
        ffmpeg_path: ffmpeg.map(|path| path.to_string_lossy().to_string()),
        ffprobe_path: ffprobe.map(|path| path.to_string_lossy().to_string()),
        libx264_available,
        libvpx_vp9_available,
        libopus_available,
        aac_available,
        detail,
        next_action,
    }
}

pub fn inspect_video(path: &Path) -> VideoInspection {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let format = detect_format(path);
    let mut warnings = Vec::new();

    let Some(ffprobe) = find_command(ffprobe_name()) else {
        warnings.push(
            "FFprobe was not found; video details are limited to the file name. Install FFmpeg to inspect videos."
                .to_string(),
        );
        return VideoInspection {
            path: path.to_string_lossy().to_string(),
            file_name,
            format,
            container: None,
            duration_seconds: None,
            width: None,
            height: None,
            video_codec: None,
            audio_codec: None,
            fps: None,
            has_audio: false,
            warnings,
        };
    };

    match probe_video(&ffprobe, path) {
        Ok(probe) => {
            warnings.extend(probe.warnings);
            VideoInspection {
                path: path.to_string_lossy().to_string(),
                file_name,
                format,
                container: probe.container,
                duration_seconds: probe.duration_seconds,
                width: probe.width,
                height: probe.height,
                video_codec: probe.video_codec,
                audio_codec: probe.audio_codec,
                fps: probe.fps,
                has_audio: probe.has_audio,
                warnings,
            }
        }
        Err(error) => {
            warnings.push(format!("Could not probe video details: {error}"));
            VideoInspection {
                path: path.to_string_lossy().to_string(),
                file_name,
                format,
                container: None,
                duration_seconds: None,
                width: None,
                height: None,
                video_codec: None,
                audio_codec: None,
                fps: None,
                has_audio: false,
                warnings,
            }
        }
    }
}

pub fn convert_video(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertVideoRequest,
) -> Result<Vec<String>, String> {
    if !is_video_like(input_path) {
        return Err(format!(
            "{} is not a supported video input. Supported extensions: {}.",
            input_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("This file"),
            SUPPORTED_INPUT_EXTENSIONS.join(", ")
        ));
    }

    let Some(ffmpeg) = find_command(ffmpeg_name()) else {
        return Err("FFmpeg was not found next to the app or on PATH. Install FFmpeg and restart TypeShift to convert videos.".to_string());
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

    Ok(vec![
        format!(
            "Encoded with FFmpeg as {} ({}, {})",
            request.output_format.label(),
            request.quality.label(),
            request.scale.label()
        ),
        if request.output_format == VideoOutputFormat::Gif {
            "GIF output has no audio track and uses a reduced frame rate.".to_string()
        } else {
            "Video and audio streams were re-encoded with the selected quality settings."
                .to_string()
        },
    ])
}

pub fn create_video_preview_frame(input_path: &Path) -> Result<PathBuf, String> {
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
        return Err("FFmpeg was not found next to the app or on PATH; video preview is unavailable.".to_string());
    };

    let seek = preview_seek_seconds(input_path).unwrap_or(0.0);
    let args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-ss".to_string(),
        format!("{seek:.2}"),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-vf".to_string(),
        "scale='min(640,iw)':-2".to_string(),
        "-y".to_string(),
        preview_path.to_string_lossy().to_string(),
    ];

    let output = no_console_command(&ffmpeg)
        .args(&args)
        .output()
        .map_err(|error| format!("Could not start FFmpeg for preview: {error}"))?;

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
            "FFmpeg could not extract a preview frame.".to_string()
        } else {
            message
        });
    }

    if !preview_path.exists() {
        return Err("FFmpeg did not write a preview frame.".to_string());
    }
    Ok(preview_path)
}

pub fn safe_output_path(input_path: &Path, output_format: &VideoOutputFormat) -> PathBuf {
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

pub fn is_video_like(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .map(|value| SUPPORTED_INPUT_EXTENSIONS.contains(&value.as_str()))
        .unwrap_or(false)
}

fn build_ffmpeg_args(
    input_path: &Path,
    output_path: &Path,
    request: &ConvertVideoRequest,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
    ];

    if request.output_format == VideoOutputFormat::Gif {
        let mut filter = "fps=15".to_string();
        if let Some(scale) = request.scale.scale_filter() {
            filter.push(',');
            filter.push_str(&scale);
        }
        filter.push_str(",format=rgb24");
        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-vf".to_string(),
            filter,
            "-loop".to_string(),
            "0".to_string(),
            "-an".to_string(),
        ]);
    } else {
        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a?".to_string(),
        ]);

        match request.output_format {
            VideoOutputFormat::Mp4 | VideoOutputFormat::Mkv => {
                args.extend([
                    "-c:v".to_string(),
                    "libx264".to_string(),
                    "-preset".to_string(),
                    "medium".to_string(),
                    "-crf".to_string(),
                    request.quality.crf_h264().to_string(),
                    "-pix_fmt".to_string(),
                    "yuv420p".to_string(),
                    "-c:a".to_string(),
                    "aac".to_string(),
                    "-b:a".to_string(),
                    "128k".to_string(),
                ]);
                if request.output_format == VideoOutputFormat::Mp4 {
                    args.extend(["-movflags".to_string(), "+faststart".to_string()]);
                }
            }
            VideoOutputFormat::Webm => {
                args.extend([
                    "-c:v".to_string(),
                    "libvpx-vp9".to_string(),
                    "-crf".to_string(),
                    request.quality.crf_vp9().to_string(),
                    "-b:v".to_string(),
                    "0".to_string(),
                    "-c:a".to_string(),
                    "libopus".to_string(),
                    "-b:a".to_string(),
                    "96k".to_string(),
                ]);
            }
            VideoOutputFormat::Gif => unreachable!("GIF branch handled above"),
        }

        if let Some(scale) = request.scale.scale_filter() {
            args.extend(["-vf".to_string(), scale]);
        }
    }

    args.extend([
        "-n".to_string(),
        output_path.to_string_lossy().to_string(),
    ]);
    args
}

struct ProbedVideo {
    container: Option<String>,
    duration_seconds: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    fps: Option<f64>,
    has_audio: bool,
    warnings: Vec<String>,
}

fn probe_video(ffprobe: &Path, path: &Path) -> Result<ProbedVideo, String> {
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
            "FFprobe could not read this file as a video.".to_string()
        } else {
            message
        });
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Could not parse FFprobe output: {error}"))?;

    let format = &value["format"];
    let streams = value["streams"].as_array().cloned().unwrap_or_default();
    let mut warnings = Vec::new();

    let container = format["format_name"]
        .as_str()
        .map(|name| name.split(',').next().unwrap_or(name).to_string());
    let duration_seconds = format["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .or_else(|| {
            streams
                .iter()
                .find_map(|stream| stream["duration"].as_str())
                .and_then(|value| value.parse::<f64>().ok())
        });

    let video_stream = streams.iter().find(|stream| stream["codec_type"] == "video");
    let audio_stream = streams.iter().find(|stream| stream["codec_type"] == "audio");

    let width = video_stream
        .and_then(|stream| stream["width"].as_u64())
        .map(|value| value as u32);
    let height = video_stream
        .and_then(|stream| stream["height"].as_u64())
        .map(|value| value as u32);
    let video_codec = video_stream
        .and_then(|stream| stream["codec_name"].as_str())
        .map(|value| value.to_string());
    let audio_codec = audio_stream
        .and_then(|stream| stream["codec_name"].as_str())
        .map(|value| value.to_string());
    let fps = video_stream.and_then(|stream| parse_frame_rate(stream["r_frame_rate"].as_str()));

    if streams.is_empty() {
        warnings.push("FFprobe found no media streams in this file.".to_string());
    }
    if video_stream.is_none() {
        warnings.push("No video stream was found; this file may be audio-only.".to_string());
    }

    Ok(ProbedVideo {
        container,
        duration_seconds,
        width,
        height,
        video_codec,
        audio_codec,
        fps,
        has_audio: audio_stream.is_some(),
        warnings,
    })
}

fn parse_frame_rate(value: Option<&str>) -> Option<f64> {
    let value = value?;
    if let Ok(parsed) = value.parse::<f64>() {
        return Some(parsed);
    }
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    if denominator == 0.0 {
        return None;
    }
    Some(numerator / denominator)
}

fn scan_encoders(ffmpeg: &Path) -> (bool, bool, bool, bool) {
    let Ok(output) = no_console_command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return (false, false, false, false);
    };
    let text = String::from_utf8_lossy(&output.stdout);
    (
        encoder_present(&text, "libx264"),
        encoder_present(&text, "libvpx-vp9"),
        encoder_present(&text, "libopus"),
        encoder_present(&text, "aac"),
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

    let cache_dir = std::env::temp_dir().join("typeshift-video-previews");
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create video preview cache: {error}"))?;
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

fn preview_seek_seconds(input_path: &Path) -> Option<f64> {
    let ffprobe = find_command(ffprobe_name())?;
    let probe = probe_video(&ffprobe, input_path).ok()?;
    let duration = probe.duration_seconds?;
    if duration <= 0.0 {
        return Some(0.0);
    }
    Some(duration.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(output_format: VideoOutputFormat) -> ConvertVideoRequest {
        ConvertVideoRequest {
            input_paths: vec!["C:/tmp/video.mp4".to_string()],
            output_format,
            quality: VideoQuality::Medium,
            scale: VideoScalePolicy::Original,
            destination_policy: DestinationPolicy::SameFolder,
            conflict_policy: ConflictPolicy::AutoNumber,
        }
    }

    #[test]
    fn safe_output_path_uses_same_folder_and_typeshift_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("movie.mp4");

        assert_eq!(
            safe_output_path(&input, &VideoOutputFormat::Mp4),
            dir.path().join("movie.typeshift.mp4")
        );
    }

    #[test]
    fn safe_output_path_auto_numbers_existing_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("movie.mp4");
        fs::write(dir.path().join("movie.typeshift.gif"), b"existing").unwrap();
        fs::write(dir.path().join("movie.typeshift-1.gif"), b"existing").unwrap();

        assert_eq!(
            safe_output_path(&input, &VideoOutputFormat::Gif),
            dir.path().join("movie.typeshift-2.gif")
        );
    }

    #[test]
    fn mp4_args_encode_h264_aac_with_faststart() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/video.mp4"),
            Path::new("C:/tmp/video.typeshift.mp4"),
            &request(VideoOutputFormat::Mp4),
        );

        assert!(args.iter().any(|arg| arg == "libx264"));
        assert!(args.iter().any(|arg| arg == "aac"));
        assert!(args.iter().any(|arg| arg == "yuv420p"));
        assert!(args.iter().any(|arg| arg == "+faststart"));
        assert!(args.iter().any(|arg| arg == "-n"));
        assert!(args.iter().any(|arg| arg == "C:/tmp/video.typeshift.mp4"));
    }

    #[test]
    fn webm_args_encode_vp9_opus_and_never_overwrite() {
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/video.mov"),
            Path::new("C:/tmp/video.typeshift.webm"),
            &request(VideoOutputFormat::Webm),
        );

        assert!(args.iter().any(|arg| arg == "libvpx-vp9"));
        assert!(args.iter().any(|arg| arg == "libopus"));
        assert!(args.iter().any(|arg| arg == "0"));
        assert!(!args.iter().any(|arg| arg == "+faststart"));
    }

    #[test]
    fn gif_args_use_filter_chain_and_drop_audio() {
        let mut gif_request = request(VideoOutputFormat::Gif);
        gif_request.scale = VideoScalePolicy::Height480;
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/video.mp4"),
            Path::new("C:/tmp/video.typeshift.gif"),
            &gif_request,
        );

        assert!(args.iter().any(|arg| arg == "fps=15,scale=-2:480,format=rgb24"));
        assert!(args.iter().any(|arg| arg == "-an"));
        assert!(!args.iter().any(|arg| arg == "libx264"));
    }

    #[test]
    fn scale_filter_is_applied_for_scaled_outputs() {
        let mut scaled = request(VideoOutputFormat::Mkv);
        scaled.scale = VideoScalePolicy::Height720;
        let args = build_ffmpeg_args(
            Path::new("C:/tmp/video.mp4"),
            Path::new("C:/tmp/video.typeshift.mkv"),
            &scaled,
        );

        assert!(args.iter().any(|arg| arg == "scale=-2:720"));
    }

    #[test]
    fn is_video_like_matches_common_extensions() {
        assert!(is_video_like(Path::new("clip.mp4")));
        assert!(is_video_like(Path::new("clip.MOV")));
        assert!(is_video_like(Path::new("clip.webm")));
        assert!(!is_video_like(Path::new("photo.heic")));
        assert!(!is_video_like(Path::new("photo.png")));
    }

    #[test]
    fn parses_rational_and_float_frame_rates() {
        assert_eq!(parse_frame_rate(Some("30000/1001")), Some(30000.0 / 1001.0));
        assert_eq!(parse_frame_rate(Some("25")), Some(25.0));
        assert_eq!(parse_frame_rate(Some("0/0")), None);
        assert_eq!(parse_frame_rate(None), None);
    }

    #[test]
    fn encoder_scan_detects_known_encoders() {
        let sample = "\n V....D libx264              H.264 / AVC / MPEG-4 AVC part 10\n \
                      V....D libvpx-vp9          VP9 (Experimental)\n \
                      A..... aac                 AAC (Advanced Audio Coding)\n \
                      A..... libopus             libopus Opus\n";
        assert!(encoder_present(sample, "libx264"));
        assert!(encoder_present(sample, "libvpx-vp9"));
        assert!(encoder_present(sample, "aac"));
        assert!(encoder_present(sample, "libopus"));
        assert!(!encoder_present(sample, "libx265"));
    }

    #[test]
    fn video_backend_status_is_internally_consistent() {
        let status = inspect_video_backend();
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

        let status = inspect_video_backend();
        assert!(status.available, "bundled FFmpeg was not detected: {}", status.detail);
        assert_eq!(
            status.ffmpeg_path.as_deref(),
            Some(bundled_dir.join(ffmpeg_name()).to_str().unwrap())
        );
    }
}
