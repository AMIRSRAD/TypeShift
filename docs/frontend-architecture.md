# Frontend Architecture

The frontend is a React and TypeScript app in `src/`. It is designed as a
single desktop workspace rather than a multipage website.

## Files

```text
src/App.tsx      Main UI, state, event handlers, and converter views
src/styles.css   Dark desktop layout, tabs, queue, preview, settings, results
src/tauri.ts     Typed wrappers around Tauri commands
src/types.ts     TypeScript contracts matching Rust request/result shapes
src/main.tsx     React bootstrap
```

## Top-Level Layout

`App.tsx` renders three major regions:

```text
topbar
  brand
  Image / Video / Audio tabs
  Info button
  add-files button

workspace
  file queue
  preview, inspection, and results
  conversion settings

action-bar
  queue status
  convert button
```

The `Image` tab is the active implemented converter. `Video` and `Audio` render
reserved placeholder workspaces.

The `Info` button opens an in-app About panel. It lists the creator, website,
license status, and ownership notice. Website actions call Tauri's opener
plugin so the site opens in the system browser instead of replacing the app
WebView.

## State Responsibilities

`App.tsx` owns UI state:

- queue contents,
- selected file,
- inspection results,
- current output options,
- advanced panel state,
- latest conversion job,
- preview state,
- backend readiness state,
- About/Info panel state.

The frontend does not parse source files. It asks Rust to inspect and convert
files through Tauri commands.

## Tauri Calls

Only `src/tauri.ts` should call `invoke` directly. UI components should use
typed wrappers such as:

```ts
inspectImages(paths)
previewImage(path)
convertImages(request)
revealOutput(path)
```

This keeps command names and payload shapes in one place.

External website opening is not an `invoke` command; it uses
`@tauri-apps/plugin-opener` directly from the Info panel.

## Preview Handling

The frontend calls `previewImage(path)` after a file is selected. The backend
creates a temporary PNG preview and returns a base64 data URL. This avoids
WebView path permission issues and broken local-image URLs.

The preview UI has separate states:

- empty state,
- loading,
- rendered preview,
- preview unavailable.

Loaded previews use `object-fit: contain` so portrait and landscape images stay
fully visible.

## Results Handling

The `job` state contains the latest `ConversionJob`. Results render below the
inspection grid and are intentionally compact:

- summary row,
- per-file output row,
- badges for common outcomes,
- collapsible technical details,
- sidecar outputs when present.

Technical warnings are hidden behind a disclosure because normal users should
not see backend-level details unless they need them.

## Layout Rules

The app shell itself should not scroll. Individual lists can scroll:

- file queue,
- settings panel,
- results panel,
- technical details when expanded.

When changing CSS, keep these constraints:

- default window size must show all primary controls,
- fullscreen should let the preview expand,
- result rows should not steal all vertical space,
- empty-state preview should not reuse the real image split layout,
- text must wrap or truncate intentionally.

## Adding New UI Modules

For `Video` or `Audio`:

1. Add domain-specific state instead of reusing image state.
2. Add domain-specific types in `src/types.ts`.
3. Add Tauri wrappers in `src/tauri.ts`.
4. Add a dedicated workspace component or module.
5. Keep the shared topbar tabs and footer behavior consistent.

Do not expose a converter option until the backend can actually honor it.
