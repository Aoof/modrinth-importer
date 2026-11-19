# Changelog

Release notes are generated from commits and published on the [Releases page](https://github.com/Aoof/modrinth-importer/releases).

## Unreleased
- Import a CurseForge modpack by link, project ID or name search (CLI `search`/`convert <link>`, app *Find a modpack on CurseForge*), with a Core API key.
- Modpacks are assumed not to exist on Modrinth: only exact (hash) matches use Modrinth; everything else is downloaded from its source and bundled, and what can't be downloaded is labelled with a link and resolved by supplying the exact file (verified by hash or exact file name).
- Advanced options (download, bundle, match by name, keyless lookup) in the app and `mrimport config`.
- Experimental keyless lookup through cursemaven.com and api.cfwidget.com.
- Warns when a saved CurseForge key looks like an author upload token.

## 0.1.0
- Initial release: CLI and desktop app; import from CurseForge, Prism/MultiMC, ATLauncher, FTB, Technic, packwiz and mods folders; publish from jars, GitHub releases and CurseForge.
