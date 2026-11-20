# Changelog

Release notes are generated from commits and published on the [Releases page](https://github.com/Aoof/modrinth-importer/releases).

## Unreleased
- `mrimport self-update` verifies every download first, then removes the old files and reinstalls the CLI and the desktop app the way the installers do; it never downgrades.
- The desktop app offers to update and restart when it opens (optional).
- Fix: a failed `.mrpack` export no longer leaves a truncated archive ("Could not find EOCD"); the pack is written to a temp file and renamed when complete, jars are stored instead of recompressed, and the archive phase reports progress.
- Windows: the CLI embeds version info, an icon and a manifest to avoid Defender false positives; the installer explains a blocked file.
- Import a CurseForge modpack by link, project ID or name search (CLI `search`/`convert <link>`, app *Find a modpack on CurseForge*), with a Core API key.
- Modpacks are assumed not to exist on Modrinth: only exact (hash) matches use Modrinth; everything else is downloaded from its source and bundled, and what can't be downloaded is labelled with a link and resolved by supplying the exact file (verified by hash or exact file name).
- Advanced options (download, bundle, match by name, keyless lookup) in the app and `mrimport config`.
- Experimental keyless lookup through cursemaven.com and api.cfwidget.com.
- Warns when a saved CurseForge key looks like an author upload token.

## 0.1.0
- Initial release: CLI and desktop app; import from CurseForge, Prism/MultiMC, ATLauncher, FTB, Technic, packwiz and mods folders; publish from jars, GitHub releases and CurseForge.
