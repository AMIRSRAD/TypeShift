# Windows libheif Auxiliary Backend

## Goal

Match iOS-style HEIC to JPEG behavior as closely as possible on Windows:

- Decode the rendered HEIC image with color management.
- Extract HEIC auxiliary image items.
- Preserve or export iPhone portrait/depth/HDR-related assets.
- Write JPEG outputs that iOS Photos can recognize where the JPEG container
  supports the required auxiliary data.

This is separate from the current Windows WIC fallback. WIC can decode a rendered
image but does not expose decoded auxiliary images. TypeShift now parses the HEIC
container item graph itself so it can detect and preserve raw auxiliary and
metadata payloads as sidecars, but viewable sidecar decoding and
Apple-compatible JPEG embedding still require the native libheif/metadata writer
path.

## Source Data To Extract

The libheif backend must inspect each HEIC for:

- Primary rendered image item.
- Thumbnails.
- EXIF and XMP metadata item references.
- ICC profile or nclx color properties.
- Auxiliary image references and auxiliary type strings.
- Depth or disparity maps.
- Portrait effects matte.
- Semantic mattes such as hair, skin, teeth, glasses, and sky when present.
- HDR gain map and gain-map metadata.
- Alternate rendered items or derived image items.

## Output Strategies

### JPEG With Apple-Compatible Auxiliary Data

Target behavior:

- Write the rendered JPEG.
- Preserve safe EXIF/XMP and Apple MakerNotes required by Photos.
- Attach depth/matte auxiliary data using an Apple-compatible structure.
- Preserve ICC profile.
- Preserve or transcode HDR gain-map data only when the JPEG gain-map structure
  is understood and validated on iOS Photos.

This must not be enabled until an exported JPEG is re-imported into iOS Photos
and still exposes the expected Portrait/HDR behavior.

### Sidecar Export

Sidecar export is the safer first implementation. The current implementation
writes raw HEIC auxiliary and metadata item payloads, not decoded depth/matte
PNG/TIFF images yet:

- `photo.typeshift.jpg`
- `photo.typeshift.depth-12.hvc1.heif-item`
- `photo.typeshift.gain-map-13.hvc1.heif-item`
- `photo.typeshift.metadata-3.exif.exif`
- `photo.typeshift.aux-manifest.json`

Sidecars let users verify that extraction works before TypeShift attempts to
write Apple-compatible JPEG auxiliary containers.

### Live Photo

Live Photo round-trip is not solved by HEIC to JPEG alone. It requires:

- Still image output.
- Paired `.MOV` asset.
- Matching Apple content identifier / media group identifiers in both files.
- Correct still-image-time metadata in the movie.

Live Photo support should be implemented as a separate paired-asset workflow.

## Rust Integration Plan

Use a dedicated backend module instead of extending the WIC fallback:

```text
src-tauri/src/heif_auxiliary.rs
```

Responsibilities:

- `inspect_auxiliary_items(path) -> HeifAuxiliaryInspection`
- `extract_auxiliary_sidecars(source, output) -> Vec<AuxiliarySidecar>`
- `convert_heic_to_jpeg_with_auxiliary(request) -> ConversionResult`

The native HEIC pieces are feature-gated through the current project feature:

```text
cargo build --features native-heif
```

Windows dependency options:

- vcpkg-provided `libheif` through `libheif-sys`.
- embedded `libheif` source if codec dependencies and licenses are acceptable.

## Acceptance Criteria

Before enabling the backend by default:

- Fixture with iPhone Portrait HEIC reports at least one depth/disparity or matte
  auxiliary item.
- Sidecar extraction writes deterministic files for every detected auxiliary
  item.
- iOS Photos can re-import a generated JPEG and still expose Portrait behavior,
  if JPEG auxiliary embedding is enabled.
- HDR fixture with gain map is either preserved into a validated HDR-capable
  output or rejected with no SDR output written.
- Live Photo fixtures are rejected unless the paired `.MOV` is present and the
  paired-asset workflow is selected.
