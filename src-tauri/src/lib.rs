mod hdr_backend;
mod hdr_jpeg_writer;
mod heif_auxiliary;
mod image_engine;
mod metadata_writer;
mod native_heif_hdr;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hdr_backend::HdrBackendStatus;
use image_engine::{
    convert_image, create_preview_image, inspect_image, ConversionJob, ConversionJobStatus,
    ConvertRequest, ImageInspection, JobStatus,
};
use metadata_writer::MetadataToolStatus;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::Mutex,
};
use tauri::State;
use uuid::Uuid;

#[derive(Default)]
struct AppState {
    jobs: Mutex<HashMap<String, ConversionJobStatus>>,
    cancellations: Mutex<HashSet<String>>,
}

#[tauri::command]
fn inspect_images(paths: Vec<String>) -> Result<Vec<ImageInspection>, String> {
    paths
        .iter()
        .map(|path| inspect_image(Path::new(path)).map_err(|error| error.to_string()))
        .collect()
}

#[tauri::command]
fn preview_image(path: String) -> Result<String, String> {
    let preview_path = create_preview_image(Path::new(&path)).map_err(|error| error.to_string())?;
    let bytes = fs::read(&preview_path).map_err(|error| {
        format!(
            "Could not read preview image {}: {error}",
            preview_path.display()
        )
    })?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn convert_images(
    request: ConvertRequest,
    state: State<'_, AppState>,
) -> Result<ConversionJob, String> {
    let job_id = Uuid::new_v4().to_string();
    let total = request.input_paths.len();
    let mut job = ConversionJob {
        job_id: job_id.clone(),
        status: JobStatus::Running,
        total,
        completed: 0,
        results: Vec::with_capacity(total),
    };

    {
        let mut jobs = state
            .jobs
            .lock()
            .map_err(|_| "Job state is unavailable".to_string())?;
        jobs.insert(job_id.clone(), job.clone());
    }

    for input_path in &request.input_paths {
        if is_cancelled(&state, &job_id)? {
            job.status = JobStatus::Cancelled;
            break;
        }

        let result = convert_image(Path::new(input_path), &request);
        if result.status.is_terminal_success() {
            job.completed += 1;
        }
        job.results.push(result);

        let mut jobs = state
            .jobs
            .lock()
            .map_err(|_| "Job state is unavailable".to_string())?;
        jobs.insert(job_id.clone(), job.clone());
    }

    if job.status != JobStatus::Cancelled {
        job.status = if job.results.iter().any(|result| result.status.is_failure()) {
            JobStatus::Failed
        } else {
            JobStatus::Completed
        };
    }

    let mut jobs = state
        .jobs
        .lock()
        .map_err(|_| "Job state is unavailable".to_string())?;
    jobs.insert(job_id, job.clone());
    Ok(job)
}

#[tauri::command]
fn get_job_status(
    job_id: String,
    state: State<'_, AppState>,
) -> Result<ConversionJobStatus, String> {
    let jobs = state
        .jobs
        .lock()
        .map_err(|_| "Job state is unavailable".to_string())?;
    jobs.get(&job_id)
        .cloned()
        .ok_or_else(|| format!("Unknown conversion job: {job_id}"))
}

#[tauri::command]
fn cancel_job(job_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut cancellations = state
        .cancellations
        .lock()
        .map_err(|_| "Cancellation state is unavailable".to_string())?;
    cancellations.insert(job_id);
    Ok(())
}

#[tauri::command]
fn reveal_output(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|error| format!("Could not reveal output: {error}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let parent = Path::new(&path)
            .parent()
            .ok_or_else(|| "Output path has no parent folder".to_string())?;
        std::process::Command::new("open")
            .arg(parent)
            .spawn()
            .map_err(|error| format!("Could not reveal output: {error}"))?;
    }

    Ok(())
}

#[tauri::command]
fn get_metadata_tool_status() -> MetadataToolStatus {
    metadata_writer::inspect_metadata_tool()
}

#[tauri::command]
fn get_hdr_backend_status() -> HdrBackendStatus {
    hdr_backend::inspect_hdr_backend()
}

fn is_cancelled(state: &State<'_, AppState>, job_id: &str) -> Result<bool, String> {
    let cancellations = state
        .cancellations
        .lock()
        .map_err(|_| "Cancellation state is unavailable".to_string())?;
    Ok(cancellations.contains(job_id))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            inspect_images,
            preview_image,
            convert_images,
            get_job_status,
            cancel_job,
            reveal_output,
            get_metadata_tool_status,
            get_hdr_backend_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running TypeShift");
}
