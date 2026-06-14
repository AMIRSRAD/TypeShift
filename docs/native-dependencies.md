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

The Tauri/Rust app now includes a Windows Imaging Component fallback for HEIC
inspection and conversion. This path is local and can convert HEIC files when the
required Microsoft HEIF/HEVC codecs are installed on Windows.

JPEG metadata preservation is implemented in-process through the Rust native
metadata writer. It copies standard EXIF/XMP/ICC payloads into JPEG outputs,
normalizes orientation because pixels are already rendered, and strips readable
GPS/location references for the safe metadata policy.

This is not the final bundled backend. The app labels WIC output with warnings
because codec availability and color behavior can vary by machine. The bundled
`libheif` + LittleCMS backend remains the required path before claiming full
professional HEIC fidelity.

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
