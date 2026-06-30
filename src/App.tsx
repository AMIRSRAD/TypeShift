import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  FileAudio,
  FileImage,
  FileVideo,
  FolderOpen,
  ImagePlus,
  Info as InfoIcon,
  Loader2,
  Play,
  Shield,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { convertImages, getHdrBackendStatus, getMetadataToolStatus, inspectImages, previewImage, revealOutput } from "./tauri";
import type {
  ConversionJob,
  ConvertRequest,
  HdrBackendStatus,
  ImageInspection,
  AuxiliaryDataPolicy,
  MetadataToolStatus,
  MetadataPolicy,
  OutputFormat,
  Preset,
} from "./types";

const defaultRequest = (paths: string[]): ConvertRequest => ({
  inputPaths: paths,
  outputFormat: "png",
  preset: "visual_match",
  metadataPolicy: "preserve_safe",
  auxiliaryDataPolicy: "preserve_if_supported",
  destinationPolicy: "same_folder",
  conflictPolicy: "auto_number",
  jpegQuality: 92,
  pngBitDepth: 16,
  hdrPolicy: "tone_map_sdr",
});

const supportedInputExtensions = ["heic", "heif", "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "gif", "ico"];
const supportedInputPattern = new RegExp(`\\.(${supportedInputExtensions.join("|")})$`, "i");
const outputFormats: Array<{ value: OutputFormat; label: string }> = [
  { value: "png", label: "PNG" },
  { value: "jpeg", label: "JPEG" },
  { value: "adaptive_hdr_jpeg", label: "HDR JPEG" },
  { value: "webp", label: "WebP" },
  { value: "tiff", label: "TIFF" },
  { value: "bmp", label: "BMP" },
];
type ConversionScope = "selected" | "all";
type MediaMode = "image" | "video" | "audio";

function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function sidecarLabel(kind: string) {
  return kind
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/_/g, " ")
    .toLowerCase();
}

function uniquePaths(paths: string[]) {
  return Array.from(new Set(paths.filter(Boolean)));
}

function friendlyInspectionNotice(inspection: ImageInspection | null) {
  if (!inspection) return null;
  if (inspection.hasGainMap) {
    return {
      tone: "info",
      title: "HDR photo detected",
      body: "Choose HDR JPEG to keep highlight detail. Other formats create a normal SDR copy.",
    };
  }
  if (inspection.warnings.some((warning) => warning.toLowerCase().includes("gif"))) {
    return {
      tone: "info",
      title: "First frame only",
      body: "Animated GIF conversion currently uses the first frame.",
    };
  }
  return null;
}

function resultBadges(result: ConversionJob["results"][number]) {
  const text = result.warnings.join(" ").toLowerCase();
  const badges: Array<{ tone: "success" | "info" | "warn"; label: string }> = [];

  if (result.status === "completed") {
    badges.push({ tone: "success", label: "Converted" });
  }
  if (text.includes("hdr jpeg was encoded") || text.includes("hdr gain-map headroom")) {
    badges.push({ tone: "info", label: "HDR highlights kept" });
  } else if (text.includes("normal sdr raster") || text.includes("sdr")) {
    badges.push({ tone: "info", label: "SDR copy" });
  }
  if (text.includes("metadata copied")) {
    badges.push({ tone: "success", label: "Metadata kept" });
  }
  if (text.includes("gps/location metadata stripped")) {
    badges.push({ tone: "success", label: "Location removed" });
  }
  if (text.includes("portrait") || text.includes("live photo")) {
    badges.push({ tone: "warn", label: "Portrait data limited" });
  }

  return badges.slice(0, 4);
}

function resultSubtitle(result: ConversionJob["results"][number]) {
  if (result.status === "failed") {
    return result.errorMessage ?? "Conversion failed";
  }
  if (result.outputPath) {
    return basename(result.outputPath);
  }
  return "Finished";
}

export default function App() {
  const [activeMedia, setActiveMedia] = useState<MediaMode>("image");
  const [paths, setPaths] = useState<string[]>([]);
  const [inspections, setInspections] = useState<ImageInspection[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [outputFormat, setOutputFormat] = useState<OutputFormat>("png");
  const [preset, setPreset] = useState<Preset>("visual_match");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [jpegQuality, setJpegQuality] = useState(92);
  const [pngBitDepth, setPngBitDepth] = useState<8 | 16>(16);
  const [metadataPolicy, setMetadataPolicy] = useState<MetadataPolicy>("preserve_safe");
  const [auxiliaryDataPolicy, setAuxiliaryDataPolicy] = useState<AuxiliaryDataPolicy>("preserve_if_supported");
  const [conversionScope, setConversionScope] = useState<ConversionScope>("selected");
  const [job, setJob] = useState<ConversionJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [appError, setAppError] = useState<string | null>(null);
  const [metadataTool, setMetadataTool] = useState<MetadataToolStatus | null>(null);
  const [hdrBackend, setHdrBackend] = useState<HdrBackendStatus | null>(null);
  const [startupLoading, setStartupLoading] = useState(true);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);

  const selectedInspection = useMemo(
    () => inspections.find((item) => item.path === selectedPath) ?? inspections[0] ?? null,
    [inspections, selectedPath],
  );

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      Promise.allSettled([getMetadataToolStatus(), getHdrBackendStatus()])
        .then(([metadataResult, hdrResult]) => {
          if (!active) return;
          if (metadataResult.status === "fulfilled") {
            setMetadataTool(metadataResult.value);
          } else {
            setMetadataTool({
              available: false,
              path: null,
              version: null,
              detail: metadataResult.reason instanceof Error ? metadataResult.reason.message : String(metadataResult.reason),
            });
          }
          if (hdrResult.status === "fulfilled") {
            setHdrBackend(hdrResult.value);
          } else {
            setHdrBackend({
              available: false,
              heifAuxDecoderAvailable: false,
              gainMapJpegWriterAvailable: false,
              heifToolPath: null,
              vcpkgPath: null,
              detail: hdrResult.reason instanceof Error ? hdrResult.reason.message : String(hdrResult.reason),
              nextAction: "Restart TypeShift after installing the native HDR backend.",
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

    previewImage(selectedInspection.path)
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
      setInspections(await inspectImages(merged));
    } catch (error) {
      setAppError(error instanceof Error ? error.message : String(error));
    }
  }

  async function chooseFiles() {
    const selection = await open({
      multiple: true,
      filters: [{ name: "Images", extensions: supportedInputExtensions }],
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
      .filter((path) => supportedInputPattern.test(path));
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
    const targetPaths = conversionScope === "all"
      ? paths
      : selectedInspection
        ? [selectedInspection.path]
        : [];
    if (targetPaths.length === 0 || busy) return;
    setBusy(true);
    setAppError(null);
    setJob(null);

    const request = defaultRequest(targetPaths);
    request.outputFormat = outputFormat;
    request.preset = preset;
    request.jpegQuality = jpegQuality;
    request.pngBitDepth = pngBitDepth;
    request.metadataPolicy = metadataPolicy;
    request.auxiliaryDataPolicy = auxiliaryDataPolicy;
    request.hdrPolicy = outputFormat === "adaptive_hdr_jpeg" ? "preserve_hdr" : "tone_map_sdr";

    try {
      setJob(await convertImages(request));
    } catch (error) {
      setAppError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  const completed = job?.results.filter((result) => result.status === "completed").length ?? 0;
  const failed = job?.results.filter((result) => result.status === "failed").length ?? 0;
  const selectedNotice = friendlyInspectionNotice(selectedInspection);
  const metadataRelevant = outputFormat === "jpeg" || outputFormat === "adaptive_hdr_jpeg";
  const metadataMissing = metadataTool && !metadataTool.available && metadataRelevant;
  const hdrMissing = hdrBackend && !hdrBackend.available && outputFormat === "adaptive_hdr_jpeg";
  const conversionCount = conversionScope === "all" ? paths.length : selectedInspection ? 1 : 0;
  const canUseImageConverter = activeMedia === "image";

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-block">
          <h1>TypeShift</h1>
          <p>Local image conversion with color-aware defaults.</p>
        </div>
        <nav className="media-tabs" aria-label="Converter type">
          <button
            className={activeMedia === "image" ? "active" : ""}
            type="button"
            onClick={() => setActiveMedia("image")}
          >
            <FileImage size={16} />
            Image
          </button>
          <button
            className={activeMedia === "video" ? "active" : ""}
            type="button"
            onClick={() => setActiveMedia("video")}
          >
            <FileVideo size={16} />
            Video
          </button>
          <button
            className={activeMedia === "audio" ? "active" : ""}
            type="button"
            onClick={() => setActiveMedia("audio")}
          >
            <FileAudio size={16} />
            Audio
          </button>
        </nav>
        <div className="topbar-actions">
          {startupLoading && (
            <div className="startup-pill">
              <Loader2 className="spin" size={15} />
              <span>Starting</span>
            </div>
          )}
          <button className="secondary-button compact" type="button" onClick={() => setAboutOpen(true)}>
            <InfoIcon size={18} />
            Info
          </button>
          <button className="secondary-button" type="button" onClick={chooseFiles} disabled={!canUseImageConverter}>
            <ImagePlus size={18} />
            {activeMedia === "image" ? "Add images" : activeMedia === "video" ? "Add videos" : "Add audio"}
          </button>
        </div>
      </header>

      {activeMedia === "image" ? (
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
            <FileImage size={34} />
            <span>Drop images or select files</span>
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
                <div
                  className={active ? "file-row active" : "file-row"}
                  key={path}
                >
                  <button className="file-select" type="button" onClick={() => setSelectedPath(path)}>
                    <FileImage size={18} />
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
                  <FileImage size={32} />
                  <span>No preview</span>
                </div>
                <div className="preview-meta">
                  <h2>No image selected</h2>
                  <p>Add image files to inspect conversion readiness.</p>
                </div>
              </>
            )}
          </div>

          {selectedInspection && (
            <div className="inspection-grid">
              <Info label="Format" value={selectedInspection.format} />
              <Info
                label="Dimensions"
                value={
                  selectedInspection.dimensions
                    ? `${selectedInspection.dimensions.width} x ${selectedInspection.dimensions.height}`
                    : "Pending native decode"
                }
              />
              <Info label="Color" value={selectedInspection.colorProfileName ?? "Profile pending"} />
              <Info label="Bit depth" value={selectedInspection.bitDepth ? `${selectedInspection.bitDepth}-bit` : "Pending"} />
              <Info
                label="HDR"
                value={selectedInspection.hasGainMap ? "Gain map detected" : selectedInspection.hasHdr ? "HDR detected" : "Not detected"}
              />
              <Info label="Orientation" value={selectedInspection.orientation} />
              <Info label="Metadata" value={selectedInspection.metadataSummary} />
            </div>
          )}

          {(appError || (selectedNotice && !job)) && (
            <div className={appError ? "notice-strip error" : `notice-strip ${selectedNotice?.tone}`}>
              {appError ? <AlertTriangle size={18} /> : <InfoIcon size={18} />}
              <span>
                <strong>{appError ? "Something went wrong" : selectedNotice?.title}</strong>
                <small>{appError ?? selectedNotice?.body}</small>
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
                      <button className="icon-button" type="button" title="Reveal output" onClick={() => revealOutput(result.outputPath!)}>
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
                  {result.sidecarOutputs.length > 0 && (
                    <div className="sidecar-list">
                      {result.sidecarOutputs.map((sidecar) => (
                        <div className="sidecar-row" key={sidecar.path}>
                          <span>
                            <strong>{basename(sidecar.path)}</strong>
                            <small>{sidecarLabel(sidecar.kind)} - {formatBytes(sidecar.bytes)}</small>
                          </span>
                          <button className="icon-button" type="button" title="Reveal sidecar" onClick={() => revealOutput(sidecar.path)}>
                            <FolderOpen size={15} />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>

        <aside className="settings-panel">
          <div className="settings-header">
            <span>Convert</span>
            <strong>{conversionScope === "all" ? `${conversionCount} images` : selectedInspection ? selectedInspection.fileName : "No image"}</strong>
          </div>

          <section className="setting-section">
            <label>Output format</label>
            <div className="format-grid" aria-label="Output format">
              {outputFormats.map((format) => (
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
            <label>Preset</label>
            <div className="segmented-control block" aria-label="Conversion preset">
              <button className={preset === "visual_match" ? "active" : ""} type="button" onClick={() => setPreset("visual_match")}>
                Visual Match
              </button>
              <button className={preset === "srgb_compatible" ? "active" : ""} type="button" onClick={() => setPreset("srgb_compatible")}>
                sRGB Compatible
              </button>
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

          {(metadataMissing || hdrMissing) && (
            <section className="setting-section">
              {metadataMissing && (
                <div className="readiness-pill warn">
                  <AlertTriangle size={15} />
                  <span>Metadata backend unavailable</span>
                </div>
              )}
              {hdrMissing && (
                <div className="readiness-pill warn">
                  <AlertTriangle size={15} />
                  <span>HDR JPEG unavailable</span>
                </div>
              )}
            </section>
          )}

          <section className="setting-section">
            <button className="advanced-button full" type="button" onClick={() => setAdvancedOpen((value) => !value)}>
              <SlidersHorizontal size={17} />
              Advanced
              {advancedOpen ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
            </button>
          </section>

          {advancedOpen && (
            <section className="advanced-stack">
              <label>
                <span>JPEG quality</span>
                <input
                  type="range"
                  min="70"
                  max="100"
                  value={jpegQuality}
                  onChange={(event) => setJpegQuality(Number(event.currentTarget.value))}
                />
                <strong>{jpegQuality}</strong>
              </label>
              <label>
                <span>PNG depth</span>
                <select value={pngBitDepth} onChange={(event) => setPngBitDepth(Number(event.currentTarget.value) as 8 | 16)}>
                  <option value={16}>16-bit when possible</option>
                  <option value={8}>8-bit</option>
                </select>
              </label>
              <label>
                <span>Metadata</span>
                <select value={metadataPolicy} onChange={(event) => setMetadataPolicy(event.currentTarget.value as MetadataPolicy)}>
                  <option value="preserve_safe">Preserve safe metadata</option>
                  <option value="preserve_all">Preserve all metadata</option>
                  <option value="strip_except_color">Strip except color</option>
                </select>
              </label>
              <label>
                <span>Portrait data</span>
                <select
                  value={auxiliaryDataPolicy}
                  onChange={(event) => setAuxiliaryDataPolicy(event.currentTarget.value as AuxiliaryDataPolicy)}
                >
                  <option value="preserve_if_supported">Keep when supported</option>
                  <option value="extract_sidecars">Extract sidecars when supported</option>
                  <option value="discard">Discard converted-only data</option>
                </select>
              </label>
              {outputFormat === "adaptive_hdr_jpeg" && (
                <div className="advanced-warning">
                  <AlertTriangle size={16} />
                  <span>HDR JPEG uses the native gain-map path. Normal raster exports are SDR.</span>
                </div>
              )}
              {auxiliaryDataPolicy === "extract_sidecars" && (
                <div className="advanced-warning">
                  <AlertTriangle size={16} />
                  <span>Sidecar extraction writes raw HEIC auxiliary payloads and a manifest beside the output.</span>
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
          <span>{conversionCount > 0 ? `${conversionCount} ready to convert` : "Add images to begin"}</span>
        </div>
        <button className="primary-button" type="button" disabled={conversionCount === 0 || busy} onClick={runConversion}>
          {busy ? <Loader2 className="spin" size={18} /> : <Play size={18} />}
          Convert
        </button>
      </footer>
        </>
      ) : (
        <>
          <MediaPlaceholder mode={activeMedia} />
          <footer className="action-bar">
            <div className="footer-status">
              <strong>0 queued</strong>
              <span>{activeMedia === "video" ? "Video conversion workspace is reserved" : "Audio conversion workspace is reserved"}</span>
            </div>
            <button className="primary-button" type="button" disabled>
              <Play size={18} />
              Convert
            </button>
          </footer>
        </>
      )}

      {aboutOpen && (
        <div className="modal-backdrop" role="presentation" onClick={() => setAboutOpen(false)}>
          <section
            className="about-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="about-title"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="about-header">
              <div>
                <span>About this app</span>
                <h2 id="about-title">TypeShift</h2>
              </div>
              <button className="icon-button" type="button" title="Close" onClick={() => setAboutOpen(false)}>
                <X size={16} />
              </button>
            </div>
            <div className="about-grid">
              <Info label="Creator" value="Amirsalar Saberi rad" />
              <Info label="Website" value="amirsrad.ir" />
              <Info label="License" value="Proprietary - all rights reserved" />
            </div>
            <p>
              TypeShift application code, branding, UI design, and original project assets are owned by
              Amirsalar Saberi rad.
            </p>
            <p>
              Third-party dependencies and bundled libraries remain under their own respective licenses.
            </p>
          </section>
        </div>
      )}
    </main>
  );
}

function MediaPlaceholder({ mode }: { mode: Exclude<MediaMode, "image"> }) {
  const Icon = mode === "video" ? FileVideo : FileAudio;
  const title = mode === "video" ? "Video converter" : "Audio converter";
  return (
    <section className="future-workspace">
      <div className="future-panel">
        <Icon size={42} />
        <h2>{title}</h2>
        <p>This tab is reserved for the next converter module.</p>
      </div>
    </section>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-cell">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
