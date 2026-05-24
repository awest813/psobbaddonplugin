# PSOBB Addon Launcher (Tauri)

Windows-first launcher for installing and updating this addon package without manual zip extraction.

## Goals implemented

- Separate launcher boundary under `launcher/` so existing plugin build flow remains stable.
- Tauri + Rust backend for privileged operations (filesystem, download, rollback, process launch).
- Minimal frontend UI for status, install/update, settings, launch, and logs.
- Install/update flows with preflight and dry-run support.
- Backup + rollback support for tracked addon files.
- Latest release install from GitHub Releases (`bbmod.zip`) plus local zip install path.
- SHA-256 verification support (explicit checksum and auto-check when release checksum asset is available).
- Structured JSON-line logs written in app data.

## File safety model

Only these payload paths are applied from release zips:

- `addons/**`
- `dinput8.dll`
- `dinput8.pdb`
- `README.md`
- `CHANGELOG.md`

Archive entries outside this allowlist are ignored.

## Runtime dependency reminder

The game environment still requires the Visual C++ Redistributable for Visual Studio 2015.

## Development

```bash
cd launcher
npm install
npm run tauri:dev
```

## Build (Windows preferred)

```bash
cd launcher
npm install
npm run tauri:build
```

Bundled launcher artifacts are generated under `launcher/src-tauri/target/release/bundle/`.
