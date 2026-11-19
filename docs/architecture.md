# How it works

```
path ──► provider ──► ImportedPack ──► resolver ──► ResolvedMod[] ──► exporter ──► .mrpack
```

Code lives in `crates/core` (all logic), with `crates/cli` and `src-tauri` + `ui` as thin front ends.

## Providers (`providers/`)
Every source is read through a `Tree`, a uniform view over a folder or a zip. A provider turns it into an `ImportedPack`: Minecraft version, loader, a list of mods (hashes, names, local file or URL) and override files.

## Resolver (`resolve.rs`)
1. **Exact**: SHA-1 (then SHA-512) hashes are sent to Modrinth's batch `version_files` endpoint. A hit means the identical jar is on Modrinth.
2. **By name**: for the rest, search Modrinth with the mod's slug/name hint (from jar metadata or the CurseForge API) filtered by loader and Minecraft version. A slug or title match is accepted and the newest release version for that loader/Minecraft version is used. This is *a* compatible version, not necessarily the exact one in your pack.
3. Otherwise the mod is **ambiguous** (up to three candidates to choose from) or **unmatched**.

## Exporter (`export/mrpack.rs`)
Writes `modrinth.index.json` ([format](https://docs.modrinth.com/docs/modpacks/format/)) with Modrinth CDN download URLs, and puts config files plus any bundled jars under `overrides/`. A missing loader version is looked up from the loader's official metadata.

## Publishing (`publish.rs`)
Creates the project as a draft (`POST /project`), then uploads each version (`POST /version`, multipart), skipping duplicates by version number or SHA-512.

## Security notes
Credentials go to the OS keychain. Zip entries with unsafe paths are ignored on read, and unsafe override paths are never written. The self-updater and installers verify SHA-256 checksums published with each release. Release binaries are not code-signed yet, so macOS and Windows may show a first-run warning.
