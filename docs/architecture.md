# How it works

```
path ──► provider ──► ImportedPack ──► resolver ──► ResolvedMod[] ──► exporter ──► .mrpack
```

Code lives in `crates/core` (all logic), with `crates/cli` and `src-tauri` + `ui` as thin front ends.

## Providers (`providers/`)
Every source is read through a `Tree`, a uniform view over a folder or a zip. A provider turns it into an `ImportedPack`: Minecraft version, loader, a list of mods (hashes, names, local file or URL) and override files.

## CurseForge lookup (`cflookup.rs`, `cfmodpack.rs`)
A CurseForge manifest lists only `(projectID, fileID)` pairs. `cflookup` resolves them through the Core API (exact names, SHA-1, download URL, page link) and, if the opt-in keyless lookup is on, through cursemaven.com (jar hashed locally) and api.cfwidget.com (names, exact file name, page link). `cfmodpack` handles the front door: it parses a pasted link, project ID or `curseforge://` link, picks the file (explicit id, else the main file, else the newest release, never a server pack), and downloads and verifies the pack's `.zip`.

## Resolver (`resolve.rs`)
A modpack is assumed not to exist on Modrinth, so **only exact matches are used**: SHA-1 (then SHA-512) hashes go to Modrinth's batch `version_files` endpoint, and a hit means the identical file is on Modrinth. A by-name search for a *similar* mod exists but is an opt-in advanced setting.

## Fetch (`fetch.rs`)
Every mod without an exact match is downloaded from its source into a temporary folder, streamed to disk and verified (a jar signature check, and the SHA-1 where one is known). A file that was hashed only now gets a second chance at an exact Modrinth match. Failures clear the mod's download URL so it surfaces as needing a manual file.

## Manual resolution (`manual.rs`)
Mods that can't be fetched are labelled with their CurseForge page. A supplied file is verified against the *exact* file the pack asks for (hash, else exact file name, else held back as unverified), then looked up on Modrinth by hash and bundled if it isn't there.

## Exporter (`export/mrpack.rs`)
Writes `modrinth.index.json` ([format](https://docs.modrinth.com/docs/modpacks/format/)) with Modrinth CDN download URLs for exact matches, and puts config files plus every bundled jar under `overrides/`. Mods left out are listed in the report with CurseForge links. A missing loader version is looked up from the loader's official metadata.

## Publishing (`publish.rs`)
Creates the project as a draft (`POST /project`), then uploads each version (`POST /version`, multipart), skipping duplicates by version number or SHA-512.

## Security notes
Credentials go to the OS keychain. Zip entries with unsafe paths are ignored on read, and unsafe override paths are never written. The self-updater and installers verify SHA-256 checksums published with each release. The macOS app is ad-hoc signed (Developer ID signing and notarization are supported when the Apple secrets are configured, see the FAQ); the Windows installer is not signed, so both may show a first-run warning.
