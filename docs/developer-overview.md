# TypeShift Developer Overview

This document explains how TypeShift is put together for developers who need to
modify or extend the app.

## Architecture

TypeShift is a local desktop app built with:

- Tauri v2 for the Windows desktop shell and Rust command bridge.
- React, TypeScript, and Vite for the UI.
- Rust for image inspection, conversion, metadata handling, and native backend
  readiness checks.

The app is intentionally local-only. The frontend never uploads files. It passes
local file paths to Tauri commands, and the Rust backend reads and writes files
on the same machine.

## Main Runtime Flow

```text
User selects or drops files
  -> React stores the queue
  -> inspect_images(paths) reads source metadata
  -> preview_image(path) creates a temporary PNG preview
  -> user chooses output settings
  -> convert_images(request) runs conversion jobs
  -> React renders results, warnings, sidecars, and reveal buttons
```

The current production converter is the `Image` tab. `Video` and `Audio` tabs
are UI placeholders reserved for future modules.

## Important Directories

```text
src/
  App.tsx          Main React app and image converter UI
  styles.css       Application styling and layout rules
  tauri.ts         Typed frontend wrappers around Tauri commands
  types.ts         Shared frontend TypeScript contracts

src-tauri/src/
  lib.rs           Tauri command registration and app state
  image_engine.rs  Image inspection, output naming, conversion orchestration
  windows_wic.rs   Windows WIC fallback for HEIC inspection/conversion
  native_heif_hdr.rs
                   Native HEIC preview and HDR JPEG conversion path
  heif_auxiliary.rs
                   HEIC item graph parser and auxiliary/metadata extraction
  hdr_jpeg_writer.rs
                   Gain-map JPEG writer wrapper
  hdr_backend.rs   Runtime readiness checks for HDR conversion
  metadata_writer.rs
                   Native JPEG EXIF/XMP/ICC writer

docs/
  image-pipeline.md
  native-dependencies.md
  windows-libheif-auxiliary-backend.md
```

The desktop shell also registers Tauri plugins for dialogs and system-browser
opening. The Info panel uses `@tauri-apps/plugin-opener` to open
`https://amirsrad.ir` outside the WebView.

## Frontend State Model

The React app keeps image workflow state in `App.tsx`:

- `activeMedia`: selected top-level tab, currently `image`, `video`, or `audio`.
- `paths`: selected local file paths.
- `inspections`: backend inspection results for selected files.
- `selectedPath`: active file in the queue.
- output options: format, preset, metadata policy, auxiliary data policy,
  JPEG quality, PNG bit depth, and conversion scope.
- `job`: latest conversion job and per-file results.
- `metadataTool` and `hdrBackend`: backend readiness results.
- `previewSrc` and `previewError`: rendered preview state.
- `aboutOpen`: ownership/license Info panel state.

`src/tauri.ts` is the only frontend file that should call `invoke` directly.
New commands should get a typed wrapper there before UI code uses them.

## Backend Command Model

Commands are registered in `src-tauri/src/lib.rs`:

- `inspect_images(paths)`
- `preview_image(path)`
- `convert_images(request)`
- `get_job_status(job_id)`
- `cancel_job(job_id)`
- `reveal_output(path)`
- `get_metadata_tool_status()`
- `get_hdr_backend_status()`

The website opener is intentionally handled through Tauri's opener plugin
rather than a normal in-WebView navigation link.

The command layer should stay thin. Business logic belongs in the Rust modules
under `src-tauri/src/`, especially `image_engine.rs` for image conversion
orchestration.

## Conversion Behavior

`image_engine.rs` owns the high-level conversion decisions:

- Chooses the output path with a safe `*.typeshift.*` suffix.
- Detects HEIC/HEIF and routes it to HEIC-specific backends.
- Refuses SDR fallback for `HDR JPEG`.
- Adds user-facing warnings for SDR exports, auxiliary data limits, metadata
  limits, and backend failures.
- Runs metadata copying after JPEG/HDR JPEG output is written.

Normal raster inputs use the Rust `image` crate. HEIC/HEIF currently uses a mix
of Windows WIC fallback behavior and native `libheif` feature paths. In release
builds with `native-heif`, previews and HDR JPEG use the native path; normal
HEIC to PNG/JPEG can still use WIC fallback behavior where needed.

## Metadata Handling

`metadata_writer.rs` writes metadata in-process. It does not require ExifTool.

For JPEG outputs it can:

- copy standard EXIF payloads,
- copy XMP payloads when safe,
- copy ICC color profile payloads,
- normalize EXIF orientation because pixels are already rendered,
- strip readable GPS/location references in safe metadata mode.

Apple Portrait, depth, matte, and Live Photo structures are not ordinary EXIF
fields. They need separate Apple-compatible auxiliary embedding and iOS Photos
validation before TypeShift should claim full round-trip support.

## HDR JPEG Handling

`HDR JPEG` is intentionally separate from normal `JPEG`.

- Normal PNG/JPEG/WebP/TIFF/BMP outputs are SDR compatibility exports.
- HDR JPEG requires a HEIC gain map and the native HDR backend.
- The app refuses to silently produce a normal SDR JPEG when `HDR JPEG` is
  selected.

The key backend files are:

- `native_heif_hdr.rs`
- `hdr_jpeg_writer.rs`
- `hdr_backend.rs`
- `heif_auxiliary.rs`

## Build And Test

Frontend checks:

```powershell
npm test -- --run
npm run build
```

Rust checks:

```powershell
cd src-tauri
$env:VCPKG_ROOT='C:\vcpkg-master'
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_PREFIX_PATH='C:\vcpkg-master\installed\x64-windows-static-md'
$env:PKG_CONFIG='C:\vcpkg-master\downloads\tools\msys2\1e74ca60daa10104\mingw64\bin\pkg-config.exe'
cargo test --features native-heif
```

Release build without MSI:

```powershell
$env:VCPKG_ROOT='C:\vcpkg-master'
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_PREFIX_PATH='C:\vcpkg-master\installed\x64-windows-static-md'
$env:PKG_CONFIG='C:\vcpkg-master\downloads\tools\msys2\1e74ca60daa10104\mingw64\bin\pkg-config.exe'
npx tauri build --no-bundle --features native-heif
```

The release executable is written to:

```text
src-tauri/target/release/typeshift.exe
```

Do not use plain `cargo build --release` for final app testing. It can produce
an executable that still points at the development server instead of the bundled
frontend.

## Extension Points

When adding a new converter domain:

- Keep the top-level tab state in React.
- Add a dedicated request/result type in `src/types.ts`.
- Add typed frontend wrappers in `src/tauri.ts`.
- Add thin Tauri commands in `src-tauri/src/lib.rs`.
- Put domain logic in a separate Rust module rather than expanding
  `image_engine.rs`.
- Keep each domain local-only and deterministic by default.

For `Video` and `Audio`, do not reuse image-specific types. Those modules will
need their own queue inspection, codec metadata, output settings, cancellation,
and progress semantics.
