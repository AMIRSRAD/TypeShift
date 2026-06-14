# Developer Workflows

This document lists common development tasks for TypeShift.

## Install Dependencies

```powershell
npm install
```

Rust dependencies are resolved by Cargo from `src-tauri/`.

## Run Frontend Dev Server

```powershell
npm run dev -- --host 127.0.0.1
```

Use this only for frontend iteration. The real desktop app should be tested
through Tauri because file dialogs, filesystem access, and conversion commands
depend on the Tauri runtime.

## Run Tauri Dev App

```powershell
npm run tauri dev -- --features native-heif
```

If native HEIC dependencies are needed, set the same environment variables used
for release builds before starting dev.

## Run Frontend Checks

```powershell
npm test -- --run
npm run build
```

`npm run build` runs TypeScript compilation and Vite production bundling.

## Run Rust Tests

```powershell
cd src-tauri
$env:VCPKG_ROOT='C:\vcpkg-master'
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_PREFIX_PATH='C:\vcpkg-master\installed\x64-windows-static-md'
$env:PKG_CONFIG='C:\vcpkg-master\downloads\tools\msys2\1e74ca60daa10104\mingw64\bin\pkg-config.exe'
cargo test --features native-heif
```

Some local fixture tests are ignored by default because they require private
sample photos. Do not commit personal HEIC samples.

## Build Release Exe

Do not build MSI installers during normal development.

```powershell
$env:VCPKG_ROOT='C:\vcpkg-master'
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_PREFIX_PATH='C:\vcpkg-master\installed\x64-windows-static-md'
$env:PKG_CONFIG='C:\vcpkg-master\downloads\tools\msys2\1e74ca60daa10104\mingw64\bin\pkg-config.exe'
npx tauri build --no-bundle --features native-heif
```

The executable is:

```text
src-tauri/target/release/typeshift.exe
```

## Manual QA Checklist

Before pushing a UI or conversion change:

- open the app at the default window size,
- verify empty state layout,
- add one HEIC image,
- verify preview scaling,
- convert selected image,
- verify result rows and technical details are visible,
- switch Image, Video, and Audio tabs,
- verify the app does not show fake controls for unfinished modules.

For conversion changes:

- test PNG output,
- test normal JPEG output,
- test HDR JPEG with an HDR HEIC fixture,
- verify output naming and auto-numbering,
- verify metadata behavior for safe and preserve-all modes,
- verify GPS stripping when using safe metadata mode.

## Git Hygiene

Ignored files include:

- `node_modules/`,
- `dist/`,
- `src-tauri/target/`,
- `src-tauri/gen/`,
- local converted outputs,
- local sample HEIC files.

Use `git status --short --ignored` when checking that build artifacts and
private samples are not staged.

## Formatting Note

If `cargo fmt` fails because `rustfmt` is missing, install it with:

```powershell
rustup component add rustfmt
```

Do not reformat the vendored `src-tauri/vendor/libheif-sys/vendor/libheif`
source tree unless intentionally updating the vendor dependency.
