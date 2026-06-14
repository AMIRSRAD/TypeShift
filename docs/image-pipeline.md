# Image Conversion Pipeline

## Principle

TypeShift must preserve the rendered appearance of an image, not just its pixel
dimensions. Color management is part of conversion correctness.

The default mode should be "visual match":

1. Decode the source with its embedded color profile, transfer function, bit
   depth, alpha, and orientation.
2. Convert through a managed color pipeline.
3. Encode the target with an explicit color profile and deterministic settings.
4. Validate the output against a color-managed reference render.

## HEIC/HEIF Color Risks

HEIC images from iPhones commonly use Display P3, HEVC compression, EXIF
orientation, and sometimes HDR/gain-map metadata. A naive HEIC to PNG conversion
often fails because it:

- Drops the embedded ICC or nclx color information.
- Treats Display P3 pixels as untagged sRGB.
- Converts through an 8-bit intermediate too early.
- Ignores EXIF orientation.
- Ignores HDR or gain-map metadata and produces an arbitrary SDR result.
- Writes a PNG without an ICC profile, gAMA, or cHRM intent.

This is why two converters can produce PNG files with the same dimensions but
different colors.

## Required Pipeline Behavior

### Decode

- Read embedded ICC profiles and HEIF nclx color properties.
- Apply EXIF orientation exactly once.
- Preserve bit depth in the working buffer when possible.
- Detect alpha and premultiplication behavior.
- Detect SDR, HDR, and gain-map sources separately.

### Color Management

- Use an explicit color-management engine, not implicit platform defaults.
- Keep a high-precision intermediate representation before final encoding.
- Default to preserving the source color space when the target format supports
  it, for example PNG with an embedded Display P3 ICC profile.
- Offer an explicit "convert to sRGB" mode for maximum compatibility.
- Never silently strip color profiles unless the user chooses a metadata-stripped
  preset.

### Encode

For PNG output:

- Embed the output ICC profile.
- Preserve 16-bit output where useful and supported.
- Preserve alpha losslessly.
- Avoid writing untagged wide-gamut PNG files.

For JPEG output:

- Embed the output ICC profile.
- Make chroma subsampling explicit.
- Make quality and progressive settings explicit.
- Warn when alpha must be flattened.

For Adaptive HDR JPEG output:

- Use a gain-map JPEG container compatible with ISO 21496 / Adaptive HDR.
- Decode the HEIC primary image and HDR gain-map auxiliary image through the
  native HEIC backend.
- Map Apple HEIC gain-map metadata into the JPEG gain-map metadata structure.
- Embed color and safe metadata only after proving iOS Photos and modern HDR
  viewers recognize the output as HDR.
- Never fall back to a normal SDR JPEG under the same output target.

### HDR and Gain Maps

HDR HEIC and gain-map HEIC require a named policy:

- `preserve-hdr`: keep HDR metadata when the target supports it.
- `tone-map-sdr`: convert to SDR using a documented tone mapper.
- `reject-hdr`: fail with a clear message until the format is supported.

The first milestone should use `reject-hdr` or `tone-map-sdr` only after test
fixtures prove the output is correct.

## Validation Strategy

Correctness needs fixtures, not eyeballing.

Use a fixture set containing:

- iPhone HEIC Display P3 photos.
- sRGB HEIC photos.
- HEIC files with EXIF orientation variants.
- HEIC files with transparent content if available.
- HDR/gain-map HEIC samples.
- Synthetic color chart images with known RGB/Lab values.

For each fixture:

- Verify output dimensions after orientation.
- Verify embedded output color profile.
- Compare a color-managed render of source and output using Delta E.
- Verify metadata policy.
- Verify deterministic output by hashing repeated conversions.

## First Implementation Slice

Build a CLI conversion core before polishing the UI:

```text
typeshift image convert input.heic output.png --mode visual-match
typeshift image convert input.heic output.png --mode srgb-compatible
typeshift image inspect input.heic
```

The UI should call this same core instead of implementing conversion logic
directly. This keeps desktop, batch, and future server workflows consistent.

## Current Raster Format Support

The current app supports these common non-HEIC raster paths through the Rust
`image` backend:

- Inputs: PNG, JPEG, WebP, TIFF, BMP, GIF, ICO.
- Outputs: PNG, JPEG, WebP, TIFF, BMP.

GIF conversion currently decodes the first frame only. Animation preservation
must be treated as a separate feature.

HEIC/HEIF conversion currently uses the Windows WIC fallback and supports PNG or
JPEG output in that fallback path.

## Portrait And Auxiliary Data

iPhone portrait-mode HEIC files can contain auxiliary image items such as depth
maps, portrait mattes, semantic mattes, HDR gain maps, and alternate rendered
items. These are not the same as ordinary EXIF metadata.

The app exposes a separate Advanced option for this data:

- `Keep when supported`: request preservation when the backend and output format
  can carry the auxiliary structures.
- `Extract sidecars when supported`: write raw HEIC auxiliary item payloads,
  HEIC metadata item payloads, and a JSON manifest beside the converted output
  when depth, matte, gain-map, EXIF, XMP/XML, or related items are present.
- `Discard converted-only data`: convert only the rendered image.

Current limitation: TypeShift now parses the HEIC container item graph and can
extract raw auxiliary and metadata item payloads as sidecars, but the Windows WIC
rendered image fallback still does not decode those auxiliary images into
viewable depth or matte PNG/TIFF files. PNG/JPEG/WebP/TIFF/BMP raster outputs
generally cannot carry iPhone portrait structures as native HEIC auxiliary
items. iOS-recognized JPEG Portrait embedding remains disabled until an
Apple-compatible writer is implemented and validated on iOS Photos.

## JPEG Metadata Writer

After a successful JPEG render, TypeShift runs the in-process native metadata
writer:

- `Preserve safe metadata`: copy standard EXIF/XMP/ICC data, normalize
  orientation, and strip readable GPS/location references.
- `Preserve all metadata`: copy standard EXIF/XMP/ICC data and normalize
  orientation, while keeping location metadata.
- `Strip except color`: copy only the ICC profile when it is available.

The writer does not launch ExifTool or require `exiftool.exe`. It uses the HEIC
container parser to extract HEIC metadata payloads and a JPEG segment writer to
embed standard JPEG APP1/APP2 metadata.

Apple-private Portrait/Live Photo structures are still separate from standard
metadata. They require Apple-compatible auxiliary embedding and iOS Photos
validation before TypeShift can claim full Portrait/Live Photo round-trip.

The app exposes `get_metadata_tool_status` through Tauri as a backend readiness
signal for the native writer.

## HDR JPEG

TypeShift has one HDR-capable output target: `HDR JPEG`.

- Normal PNG/JPEG/WebP/TIFF/BMP outputs are SDR compatibility exports.
- `HDR JPEG` writes ISO 21496 / Adaptive HDR gain-map JPEG output, uses a
  distinct output name such as `photo.typeshift-hdr.jpg`, and refuses SDR
  fallback.
- If a HEIC gain-map source is converted to a normal SDR output, TypeShift
  reports that the HDR data was detected and that the selected output is SDR.

Current limitation: TypeShift detects HEIC gain maps from parsed auxiliary item
roles and from HDR metadata/string indicators such as Apple HDR gain-map and ISO
21496 markers. It can extract raw HEIC gain-map payloads when present, but it
does not apply or re-embed them into PNG/JPEG/WebP/TIFF/BMP outputs. `HDR JPEG`
uses the native gain-map backend and then runs the native JPEG metadata-copy
pass.

The app exposes `get_hdr_backend_status` through Tauri and shows whether HEIC
auxiliary decoding and gain-map JPEG writing are available in the current build.
The gain-map JPEG writer is linked through `ultrajpeg` and self-tested by
encoding an inspectable tiny gain-map JPEG. The `native-heif` feature links
libheif through vcpkg, decodes HEIC primary pixels plus the HDR gain-map
auxiliary image, and feeds both into the HDR JPEG writer. Apple-specific
gain-map metadata mapping is still conservative and must be validated against
iOS Photos fixtures before claiming exact Telegram/iPhone parity.

Live Photo round-trip is a separate feature from still-image metadata. A Live
Photo requires the paired video asset plus matching Apple metadata identifiers;
a PNG export cannot become a Live Photo simply by preserving metadata.

## Engine Decision

The Windows v1 app is planned around a bundled native HEIC engine plus explicit
ICC color management. The current Rust module exposes the command contracts and
pipeline stages, and includes a Windows WIC fallback for immediate local HEIC
conversion when Microsoft HEIF/HEVC codecs are installed.

The selected direction is:

- Native HEIC decode through bundled libheif dependencies.
- LittleCMS for ICC transforms.
- Fixture-based acceptance before enabling HEIC output.

No native backend should be enabled for user conversion until it passes the
fixture suite above.

The WIC fallback is allowed as an interim backend only when the UI reports that
it is machine-dependent and not the final validated libheif/LittleCMS path.
