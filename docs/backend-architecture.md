# Backend Architecture

The backend is Rust code under `src-tauri/src/`. It exposes Tauri commands to
the frontend and owns all filesystem, image decoding, conversion, metadata, and
platform integration behavior.

## Command Layer

Commands are registered in `lib.rs`.

```rust
inspect_images(paths)
preview_image(path)
convert_images(request)
get_job_status(job_id)
cancel_job(job_id)
reveal_output(path)
get_metadata_tool_status()
get_hdr_backend_status()
```

The command layer should remain thin. It should validate command-level errors,
manage job state, and delegate real work to backend modules.

## App State

`AppState` stores:

- `jobs`: latest known `ConversionJobStatus` by job id,
- `cancellations`: job ids requested for cancellation.

Conversions currently run synchronously inside the command loop. The job model
already exists so future long-running audio/video conversions can move to a
background worker without changing frontend contracts too much.

## Image Engine

`image_engine.rs` is the orchestration layer for image conversion.

Responsibilities:

- inspect supported image files,
- choose output paths,
- decide the backend path,
- create preview images,
- convert normal raster formats,
- route HEIC/HEIF files,
- run metadata writing,
- produce user-facing warnings and result objects.

The output naming policy writes beside the source file and uses a safe suffix:

```text
photo.typeshift.png
photo.typeshift-1.png
photo.typeshift-hdr.jpg
photo.typeshift-hdr-1.jpg
```

## HEIC Paths

HEIC/HEIF handling is split across multiple modules:

- `windows_wic.rs`: Windows Imaging Component fallback.
- `native_heif_hdr.rs`: native `libheif` path for preview and HDR JPEG.
- `heif_auxiliary.rs`: HEIC item graph parser and sidecar extraction.

The WIC path is useful but machine-dependent. The native path is the intended
direction for professional conversion behavior.

## HDR Backend

`HDR JPEG` is handled separately from normal JPEG.

Key rules:

- HDR JPEG requires a HEIC source with a gain map.
- The app refuses SDR fallback for HDR JPEG output.
- Normal image outputs remain SDR compatibility exports.

`hdr_backend.rs` reports whether the current build can decode HEIC auxiliary
gain-map images and write gain-map JPEG output.

## Metadata Writer

`metadata_writer.rs` is an in-process JPEG metadata writer. It uses JPEG segment
editing instead of launching ExifTool.

It can write:

- EXIF APP1,
- XMP APP1,
- ICC APP2.

It also:

- normalizes orientation after pixels are rendered,
- strips readable GPS/location metadata in safe mode,
- keeps location metadata in preserve-all mode.

It does not yet recreate Apple Photos Portrait or Live Photo behavior. Those
features require Apple-compatible private metadata and auxiliary image
embedding.

## Error And Warning Strategy

Backend messages should be split into:

- blocking errors: conversion cannot produce the requested output,
- warnings: conversion succeeded but with a limitation,
- technical details: useful to developers or advanced users.

Examples:

- Missing HDR gain map for HDR JPEG is a blocking error.
- Converting HDR HEIC to PNG is a warning because the output is SDR.
- Sidecar extraction details belong in technical details.

Do not silently fall back when the selected output format has a specific
promise, especially `HDR JPEG`.

## Adding Backend Modules

For future audio/video modules:

1. Create a new Rust module, for example `video_engine.rs` or
   `audio_engine.rs`.
2. Define domain-specific request/result types.
3. Add Tauri commands in `lib.rs`.
4. Add typed frontend wrappers in `src/tauri.ts`.
5. Keep cancellation and progress semantics explicit.
6. Add tests for naming, unsupported codecs, and deterministic outputs.

Avoid growing `image_engine.rs` with non-image logic.
