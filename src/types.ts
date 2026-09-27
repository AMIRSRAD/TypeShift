export type OutputFormat = "png" | "jpeg" | "adaptive_hdr_jpeg" | "webp" | "tiff" | "bmp";
export type Preset = "visual_match" | "srgb_compatible";
export type MetadataPolicy = "preserve_safe" | "preserve_all" | "strip_except_color";
export type AuxiliaryDataPolicy = "discard" | "preserve_if_supported" | "extract_sidecars";
export type HdrPolicy = "tone_map_sdr" | "preserve_hdr";
export type DestinationPolicy = "same_folder";
export type ConflictPolicy = "auto_number";
export type JobStatus = "queued" | "running" | "completed" | "failed" | "cancelled";
export type ResultStatus = "completed" | "failed" | "cancelled";

export interface Dimensions {
  width: number;
  height: number;
}

export interface ImageInspection {
  path: string;
  fileName: string;
  format: string;
  dimensions: Dimensions | null;
  bitDepth: number | null;
  colorProfileName: string | null;
  colorProfileType: string | null;
  orientation: string;
  hasAlpha: boolean | null;
  hasHdr: boolean;
  hasGainMap: boolean;
  metadataSummary: string;
  warnings: string[];
}

export interface ConvertRequest {
  inputPaths: string[];
  outputFormat: OutputFormat;
  preset: Preset;
  metadataPolicy: MetadataPolicy;
  auxiliaryDataPolicy: AuxiliaryDataPolicy;
  destinationPolicy: DestinationPolicy;
  conflictPolicy: ConflictPolicy;
  jpegQuality: number;
  pngBitDepth: 8 | 16;
  hdrPolicy: HdrPolicy;
}

export interface ConversionResult {
  inputPath: string;
  outputPath: string | null;
  sidecarOutputs: ConversionSidecar[];
  status: ResultStatus;
  warnings: string[];
  errorMessage: string | null;
  sourceProfileSummary: string | null;
  outputProfileSummary: string | null;
}

export interface ConversionSidecar {
  path: string;
  kind: string;
  bytes: number;
}

export interface MetadataToolStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  detail: string;
}

export interface HdrBackendStatus {
  available: boolean;
  heifAuxDecoderAvailable: boolean;
  gainMapJpegWriterAvailable: boolean;
  heifToolPath: string | null;
  vcpkgPath: string | null;
  detail: string;
  nextAction: string;
}

export interface ConversionJob {
  jobId: string;
  status: JobStatus;
  total: number;
  completed: number;
  results: ConversionResult[];
}

export type ConversionJobStatus = ConversionJob;

export type VideoOutputFormat = "mp4" | "webm" | "mkv" | "gif";
export type VideoQuality = "high" | "medium" | "low";
export type VideoScalePolicy = "original" | "height_1080" | "height_720" | "height_480";

export interface VideoInspection {
  path: string;
  fileName: string;
  format: string;
  container: string | null;
  durationSeconds: number | null;
  width: number | null;
  height: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  fps: number | null;
  hasAudio: boolean;
  warnings: string[];
}

export interface ConvertVideoRequest {
  inputPaths: string[];
  outputFormat: VideoOutputFormat;
  quality: VideoQuality;
  scale: VideoScalePolicy;
  destinationPolicy: DestinationPolicy;
  conflictPolicy: ConflictPolicy;
}

export interface VideoConversionResult {
  inputPath: string;
  outputPath: string | null;
  status: ResultStatus;
  warnings: string[];
  errorMessage: string | null;
}

export interface VideoConversionJob {
  jobId: string;
  status: JobStatus;
  total: number;
  completed: number;
  results: VideoConversionResult[];
}

export interface VideoBackendStatus {
  available: boolean;
  ffmpegPath: string | null;
  ffprobePath: string | null;
  libx264Available: boolean;
  libvpxVp9Available: boolean;
  libopusAvailable: boolean;
  aacAvailable: boolean;
  detail: string;
  nextAction: string;
}
