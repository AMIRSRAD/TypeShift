import { invoke } from "@tauri-apps/api/core";
import type {
  ConversionJob,
  ConversionJobStatus,
  ConvertRequest,
  ConvertVideoRequest,
  HdrBackendStatus,
  ImageInspection,
  MetadataToolStatus,
  VideoBackendStatus,
  VideoConversionJob,
  VideoInspection,
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

export function inspectVideos(paths: string[]): Promise<VideoInspection[]> {
  return invoke<VideoInspection[]>("inspect_videos", { paths });
}

export function previewVideoFrame(path: string): Promise<string> {
  return invoke<string>("preview_video_frame", { path });
}

export function convertVideos(request: ConvertVideoRequest): Promise<VideoConversionJob> {
  return invoke<VideoConversionJob>("convert_videos", { request });
}

export function getVideoBackendStatus(): Promise<VideoBackendStatus> {
  return invoke<VideoBackendStatus>("get_video_backend_status");
}
