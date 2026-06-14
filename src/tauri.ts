import { invoke } from "@tauri-apps/api/core";
import type {
  ConversionJob,
  ConversionJobStatus,
  ConvertRequest,
  HdrBackendStatus,
  ImageInspection,
  MetadataToolStatus,
} from "./types";

export function inspectImages(paths: string[]): Promise<ImageInspection[]> {
  return invoke<ImageInspection[]>("inspect_images", { paths });
}

export function previewImage(path: string): Promise<string> {
  return invoke<string>("preview_image", { path });
}

export function convertImages(request: ConvertRequest): Promise<ConversionJob> {
  return invoke<ConversionJob>("convert_images", { request });
}

export function getJobStatus(jobId: string): Promise<ConversionJobStatus> {
  return invoke<ConversionJobStatus>("get_job_status", { jobId });
}

export function cancelJob(jobId: string): Promise<void> {
  return invoke<void>("cancel_job", { jobId });
}

export function revealOutput(path: string): Promise<void> {
  return invoke<void>("reveal_output", { path });
}

export function getMetadataToolStatus(): Promise<MetadataToolStatus> {
  return invoke<MetadataToolStatus>("get_metadata_tool_status");
}

export function getHdrBackendStatus(): Promise<HdrBackendStatus> {
  return invoke<HdrBackendStatus>("get_hdr_backend_status");
}
