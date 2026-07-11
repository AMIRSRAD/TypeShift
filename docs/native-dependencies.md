# Native Dependency Policy

TypeShift v1 is designed to bundle its image conversion dependencies on Windows
instead of depending on installed system codecs.

## Planned Native Libraries

- `libheif` for HEIC/HEIF container decode.
- HEVC/AV1 codec dependencies required by the selected `libheif` build.
- LittleCMS for ICC profile transforms.
- Native JPEG metadata writer for standard EXIF/XMP/ICC payloads.
- Future Apple-compatible auxiliary writer for Portrait depth/matte data and
  Live Photo identifiers.

## Current Implementation Status

The Tauri/Rust app now includes two HEIC-related paths:

- a native `libheif` path, enabled by the `native-heif` feature, for HEIC
  preview generation and HDR JPEG primary/gain-map conversion;
- a Windows Imaging Component fallback for HEIC inspection and normal PNG/JPEG
  conversion when the required Microsoft HEIF/HEVC codecs are installed.

JPEG metadata preservation is implemented in-process through the Rust native
metadata writer. It copies standard EXIF/XMP/ICC payloads into JPEG outputs,
normalizes orientation because pixels are already rendered, and strips readable
GPS/location references for the safe metadata policy.

The native HDR path is still not a claim of complete iPhone parity. The app
keeps WIC output labeled with warnings because codec availability and color
behavior can vary by machine. A fully validated `libheif` + LittleCMS pipeline
with fixture acceptance remains the requirement before claiming complete
professional HEIC color fidelity.

## Packaging Requirements

Before enabling native HEIC conversion:

- Include license notices for every bundled native library.
- Validate the native metadata writer on fixture images from iPhone, Android,
  and common camera JPEG sources.
- Document dynamic vs static linking choices.
- Confirm installer output contains all required runtime DLLs.
- Run the fixture suite for Display P3, sRGB, orientation, alpha, and HDR/gain-map
  samples.

Before enabling iPhone Portrait JPEG export:

- Prove libheif auxiliary item extraction on real iPhone Portrait fixtures.
- Export sidecars for every detected depth, disparity, matte, semantic matte, and
  gain-map item.
- Prove re-imported JPEGs are recognized by iOS Photos as Portrait/HDR assets.
- Keep Live Photo as a paired still-plus-MOV workflow, not a PNG/JPEG-only
  metadata flag.
