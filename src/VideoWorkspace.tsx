import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  FileVideo,
  FolderOpen,
  Loader2,
  Play,
  Shield,
  X,
} from "lucide-react";
import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import {
  convertVideos,
  getVideoBackendStatus,
  inspectVideos,
  previewVideoFrame,
  revealOutput,
} from "./tauri";
import type {
  ConvertVideoRequest,
  VideoBackendStatus,
  VideoConversionJob,
  VideoInspection,
  VideoOutputFormat,
  VideoQuality,
  VideoScalePolicy,
} from "./types";

const supportedVideoExtensions = [
  "mp4",
  "mov",
  "mkv",
  "webm",
  "avi",
  "m4v",
  "mpg",
  "mpeg",
  "wmv",
  "flv",
  "ts",
  "3gp",
];
const supportedVideoPattern = new RegExp(`\\.(${supportedVideoExtensions.join("|")})$`, "i");
const videoOutputFormats: Array<{ value: VideoOutputFormat; label: string }> = [
  { value: "mp4", label: "MP4" },
  { value: "webm", label: "WebM" },
  { value: "mkv", label: "MKV" },
  { value: "gif", label: "GIF" },
];
const videoQualities: Array<{ value: VideoQuality; label: string }> = [
  { value: "high", label: "High" },
  { value: "medium", label: "Medium" },
  { value: "low", label: "Low" },
];
const videoScales: Array<{ value: VideoScalePolicy; label: string }> = [
  { value: "original", label: "Original" },
  { value: "height_1080", label: "1080p" },
  { value: "height_720", label: "720p" },
  { value: "height_480", label: "480p" },
];
type ConversionScope = "selected" | "all";

export interface VideoWorkspaceHandle {
  addFiles: () => void;
}

function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

function uniquePaths(paths: string[]) {
  return Array.from(new Set(paths.filter(Boolean)));
}

function formatDuration(seconds: number | null) {
  if (seconds === null || !Number.isFinite(seconds)) return "Unknown";
  const total = Math.round(seconds);
  if (total < 60) return `${total}s`;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remaining = total % 60;
  if (hours > 0) {
    return `${hours}h ${minutes}m ${remaining}s`;
  }
  return `${minutes}:${String(remaining).padStart(2, "0")}`;
}

function friendlyBackendNotice(backend: VideoBackendStatus | null) {
  if (!backend) return null;
  if (!backend.available) {
    return {
      tone: "warn" as const,
      title: "FFmpeg is required",
      body: `${backend.detail} ${backend.nextAction}`,
    };
  }
  return null;
}

function resultBadges(result: VideoConversionJob["results"][number]) {
  const text = result.warnings.join(" ").toLowerCase();
  const badges: Array<{ tone: "success" | "info" | "warn"; label: string }> = [];

  if (result.status === "completed") {
    badges.push({ tone: "success", label: "Converted" });
  }
  if (text.includes("gif")) {
    badges.push({ tone: "info", label: "GIF" });
  }
  if (text.includes("re-encoded")) {
    badges.push({ tone: "info", label: "Streams re-encoded" });
  }

  return badges.slice(0, 4);
}

function resultSubtitle(result: VideoConversionJob["results"][number]) {
  if (result.status === "failed") {
    return result.errorMessage ?? "Conversion failed";
  }
  if (result.outputPath) {
    return basename(result.outputPath);
  }
  return "Finished";
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-cell">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

const VideoWorkspace = forwardRef<VideoWorkspaceHandle>(function VideoWorkspace(_props, ref) {
  const [paths, setPaths] = useState<string[]>([]);
  const [inspections, setInspections] = useState<VideoInspection[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [outputFormat, setOutputFormat] = useState<VideoOutputFormat>("mp4");
  const [quality, setQuality] = useState<VideoQuality>("medium");
  const [scale, setScale] = useState<VideoScalePolicy>("original");
  const [conversionScope, setConversionScope] = useState<ConversionScope>("selected");
  const [job, setJob] = useState<VideoConversionJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [appError, setAppError] = useState<string | null>(null);
  const [backend, setBackend] = useState<VideoBackendStatus | null>(null);
  const [startupLoading, setStartupLoading] = useState(true);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const selectedInspection = useMemo(
    () => inspections.find((item) => item.path === selectedPath) ?? inspections[0] ?? null,
    [inspections, selectedPath],
  );

  useImperativeHandle(
    ref,
    () => ({
      addFiles: () => {
        void chooseFiles();
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      getVideoBackendStatus()
        .then((status) => {
          if (active) {
            setBackend(status);
          }
        })
        .catch((error) => {
          if (active) {
            setBackend({
              available: false,
              ffmpegPath: null,
              ffprobePath: null,
              libx264Available: false,
              libvpxVp9Available: false,
              libopusAvailable: false,
              aacAvailable: false,
              detail: error instanceof Error ? error.message : String(error),
              nextAction: "Restart TypeShift after installing FFmpeg.",
            });
          }
        })
        .finally(() => {
          if (active) {
            setStartupLoading(false);
          }
        });
    }, 250);

    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (paths.length < 2 && conversionScope === "all") {
      setConversionScope("selected");
    }
  }, [conversionScope, paths.length]);

  useEffect(() => {
    let active = true;
    setPreviewSrc(null);
    setPreviewError(null);

    if (!selectedInspection) {
      return () => {
        active = false;
      };
    }

    previewVideoFrame(selectedInspection.path)
      .then((src) => {
        if (active) {
          setPreviewSrc(src);
        }
      })
      .catch((error) => {
        if (active) {
          setPreviewError(error instanceof Error ? error.message : String(error));
        }
      });

    return () => {
      active = false;
    };
  }, [selectedInspection?.path]);

  async function addPaths(nextPaths: string[]) {
    const merged = uniquePaths([...paths, ...nextPaths]);
    setPaths(merged);
    setSelectedPath((current) => current ?? merged[0] ?? null);
    setAppError(null);

    if (merged.length === 0) {
      setInspections([]);
      return;
    }

    try {
      setInspections(await inspectVideos(merged));
    } catch (error) {
      setAppError(error instanceof Error ? error.message : String(error));
    }
  }

  async function chooseFiles() {
    const selection = await open({
      multiple: true,
      filters: [{ name: "Videos", extensions: supportedVideoExtensions }],
    });
    const selected = Array.isArray(selection) ? selection : selection ? [selection] : [];
    await addPaths(selected);
  }

  function clearQueue() {
    setPaths([]);
    setInspections([]);
    setSelectedPath(null);
    setConversionScope("selected");
    setJob(null);
    setAppError(null);
  }

  async function handleDrop(event: React.DragEvent<HTMLButtonElement>) {
    event.preventDefault();
    const droppedPaths = Array.from(event.dataTransfer.files)
      .map((file) => (file as File & { path?: string }).path ?? "")
      .filter((path) => supportedVideoPattern.test(path));
    if (droppedPaths.length > 0) {
      await addPaths(droppedPaths);
    }
  }

  function removePath(path: string) {
    const remaining = paths.filter((item) => item !== path);
    setPaths(remaining);
    setInspections((items) => items.filter((item) => item.path !== path));
    setSelectedPath((current) => (current === path ? remaining[0] ?? null : current));
  }

  async function runConversion() {
    const targetPaths =
      conversionScope === "all" ? paths : selectedInspection ? [selectedInspection.path] : [];
    if (targetPaths.length === 0 || busy || (backend && !backend.available)) return;
    setBusy(true);
    setAppError(null);
    setJob(null);

    const request: ConvertVideoRequest = {
      inputPaths: targetPaths,
      outputFormat,
      quality,
      scale,
      destinationPolicy: "same_folder",
      conflictPolicy: "auto_number",
    };

    try {
      setJob(await convertVideos(request));
    } catch (error) {
      setAppError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  const completed = job?.results.filter((result) => result.status === "completed").length ?? 0;
  const failed = job?.results.filter((result) => result.status === "failed").length ?? 0;
  const conversionCount = conversionScope === "all" ? paths.length : selectedInspection ? 1 : 0;
  const backendUnavailable = backend !== null && !backend.available;
  const backendNotice = friendlyBackendNotice(backend);
  const encoderMissing =
    backend !== null &&
    backend.available &&
    ((outputFormat === "mp4" || outputFormat === "mkv") && !backend.libx264Available) ||
    (backend !== null &&
      backend.available &&
      outputFormat === "webm" &&
      (!backend.libvpxVp9Available || !backend.libopusAvailable));

  return (
    <>
      <section className="workspace">
        <aside className="file-panel">
          <button
            className="drop-zone"
            type="button"
            onClick={chooseFiles}
            onDragOver={(event) => event.preventDefault()}
            onDrop={handleDrop}
          >
            <FileVideo size={34} />
            <span>Drop videos or select files</span>
          </button>

          <div className="queue-header">
            <span>{paths.length} selected</span>
            {paths.length > 0 && (
              <button className="icon-button" type="button" title="Clear list" onClick={clearQueue}>
                <X size={16} />
              </button>
            )}
          </div>

          <div className="file-list">
            {paths.map((path) => {
              const inspection = inspections.find((item) => item.path === path);
              const active = selectedInspection?.path === path;
              return (
                <div className={active ? "file-row active" : "file-row"} key={path}>
                  <button className="file-select" type="button" onClick={() => setSelectedPath(path)}>
                    <FileVideo size={18} />
                    <span>
                      <strong>{inspection?.fileName ?? basename(path)}</strong>
                      <small>{inspection?.format ?? "Queued"}</small>
                    </span>
                  </button>
                  <button
                    className="remove-file"
                    type="button"
                    title="Remove"
                    onClick={() => removePath(path)}
                  >
                    <X size={14} />
                  </button>
                </div>
              );
            })}
          </div>
        </aside>

        <section className="detail-panel">
          <div className={selectedInspection ? "preview-surface" : "preview-surface empty-state"}>
            {selectedInspection ? (
              <>
                <div className="preview-frame">
                  {previewSrc ? (
                    <img src={previewSrc} alt={selectedInspection.fileName} />
                  ) : previewError ? (
                    <div className="preview-empty error">
                      <AlertTriangle size={30} />
                      <span>Preview unavailable</span>
                    </div>
                  ) : (
                    <div className="preview-empty">
                      <Loader2 className="spin" size={30} />
                      <span>Loading preview</span>
                    </div>
                  )}
                </div>
                <div className="preview-meta">
                  <h2>{selectedInspection.fileName}</h2>
                  <p>{selectedInspection.path}</p>
                  {previewError && <small>{previewError}</small>}
                </div>
              </>
            ) : (
              <>
                <div className="preview-empty large">
                  <FileVideo size={32} />
                  <span>No preview</span>
                </div>
                <div className="preview-meta">
                  <h2>No video selected</h2>
                  <p>Add video files to inspect conversion readiness.</p>
                </div>
              </>
            )}
          </div>

          {selectedInspection && (
            <div className="inspection-grid">
              <Info label="Format" value={selectedInspection.format} />
              <Info
                label="Duration"
                value={formatDuration(selectedInspection.durationSeconds)}
              />
              <Info
                label="Resolution"
                value={
                  selectedInspection.width && selectedInspection.height
                    ? `${selectedInspection.width} x ${selectedInspection.height}`
                    : "Unknown"
                }
              />
              <Info label="Video codec" value={selectedInspection.videoCodec ?? "Unknown"} />
              <Info
                label="Audio"
                value={
                  selectedInspection.hasAudio
                    ? selectedInspection.audioCodec ?? "Track present"
                    : "None"
                }
              />
              <Info
                label="Frame rate"
                value={
                  selectedInspection.fps ? `${selectedInspection.fps.toFixed(1)} fps` : "Unknown"
                }
              />
            </div>
          )}

          {(appError || (backendNotice && !job)) && (
            <div className={appError ? "notice-strip error" : `notice-strip ${backendNotice?.tone}`}>
              <AlertTriangle size={18} />
              <span>
                <strong>{appError ? "Something went wrong" : backendNotice?.title}</strong>
                <small>{appError ?? backendNotice?.body}</small>
              </span>
            </div>
          )}

          {job && (
            <div className="results-panel">
              <div className={failed > 0 ? "results-summary mixed" : "results-summary"}>
                {failed > 0 ? <AlertTriangle size={18} /> : <CheckCircle2 size={18} />}
                <span>
                  {failed > 0 ? `${completed} converted, ${failed} needs attention` : `${completed} converted`}
                </span>
              </div>
              {job.results.map((result) => (
                <div className="result-group" key={result.inputPath}>
                  <div className="result-row">
                    <span>
                      <strong>{basename(result.inputPath)}</strong>
                      <small>{resultSubtitle(result)}</small>
                    </span>
                    {result.outputPath && (
                      <button
                        className="icon-button"
                        type="button"
                        title="Reveal output"
                        onClick={() => revealOutput(result.outputPath!)}
                      >
                        <FolderOpen size={16} />
                      </button>
                    )}
                  </div>
                  {result.status === "completed" && (
                    <div className="result-badges">
                      {resultBadges(result).map((badge) => (
                        <span className={`result-badge ${badge.tone}`} key={badge.label}>
                          {badge.label}
                        </span>
                      ))}
                    </div>
                  )}
                  {result.status === "failed" && result.errorMessage && (
                    <div className="notice-strip error compact">
                      <AlertTriangle size={16} />
                      <span>
                        <strong>Could not convert</strong>
                        <small>{result.errorMessage}</small>
                      </span>
                    </div>
                  )}
                  {result.warnings.length > 0 && (
                    <details className="technical-details">
                      <summary>Technical details</summary>
                      <div className="result-warning-list">
                        {result.warnings.map((warning) => (
                          <div className="result-warning" key={warning}>
                            <span>{warning}</span>
                          </div>
                        ))}
                      </div>
                    </details>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>

        <aside className="settings-panel">
          <div className="settings-header">
            <span>Convert</span>
            <strong>
              {conversionScope === "all"
                ? `${conversionCount} videos`
                : selectedInspection
                  ? selectedInspection.fileName
                  : "No video"}
            </strong>
          </div>

          <section className="setting-section">
            <label>Output format</label>
            <div className="format-grid" aria-label="Output format">
              {videoOutputFormats.map((format) => (
                <button
                  className={outputFormat === format.value ? "active" : ""}
                  type="button"
                  key={format.value}
                  onClick={() => setOutputFormat(format.value)}
                >
                  {format.label}
                </button>
              ))}
            </div>
          </section>

          <section className="setting-section">
            <label>Quality</label>
            <div className="segmented-control block" aria-label="Conversion quality">
              {videoQualities.map((option) => (
                <button
                  className={quality === option.value ? "active" : ""}
                  type="button"
                  key={option.value}
                  onClick={() => setQuality(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </section>

          <section className="setting-section">
            <label>Max height</label>
            <div className="segmented-control block" aria-label="Output resolution">
              {videoScales.map((option) => (
                <button
                  className={scale === option.value ? "active" : ""}
                  type="button"
                  key={option.value}
                  onClick={() => setScale(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </section>

          <section className="setting-section">
            <label>Scope</label>
            <div className="segmented-control block" aria-label="Conversion scope">
              <button
                className={conversionScope === "selected" ? "active" : ""}
                type="button"
                disabled={!selectedInspection || busy}
                onClick={() => setConversionScope("selected")}
              >
                Selected
              </button>
              <button
                className={conversionScope === "all" ? "active" : ""}
                type="button"
                disabled={paths.length < 2 || busy}
                onClick={() => setConversionScope("all")}
              >
                All
              </button>
            </div>
          </section>

          {(backendUnavailable || encoderMissing) && (
            <section className="setting-section">
              {backendUnavailable && (
                <div className="readiness-pill warn">
                  <AlertTriangle size={15} />
                  <span>FFmpeg not found</span>
                </div>
              )}
              {encoderMissing && (
                <div className="readiness-pill warn">
                  <AlertTriangle size={15} />
                  <span>Required encoder missing in this FFmpeg build</span>
                </div>
              )}
            </section>
          )}

          <div className="privacy-note">
            <Shield size={17} />
            <span>Local processing</span>
          </div>
        </aside>
      </section>

      <footer className="action-bar">
        <div className="footer-status">
          <strong>{paths.length} queued</strong>
          <span>
            {conversionCount > 0 ? `${conversionCount} ready to convert` : "Add videos to begin"}
          </span>
        </div>
        <button
          className="primary-button"
          type="button"
          disabled={conversionCount === 0 || busy || backendUnavailable}
          onClick={runConversion}
        >
          {busy ? <Loader2 className="spin" size={18} /> : <Play size={18} />}
          Convert
        </button>
      </footer>
    </>
  );
});

export default VideoWorkspace;
