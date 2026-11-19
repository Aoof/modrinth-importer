# FAQ

**Why were some mods "matched by name"?**
Modrinth didn't have the identical jar, so the newest compatible release was used. Review those in the app (they're marked *By name*) or in the report.

**Why do I need a CurseForge API key?**
CurseForge modpack zips only list project and file IDs. Their API is the only way to learn the file names and hashes. Keys are free at <https://console.curseforge.com>. Instance folders and other sources don't need one.

**Some mods were skipped.**
They aren't on Modrinth and no jar was available to bundle (or bundling was off). The report lists them. Bundling third-party mods may need the author's permission, so `--download-remote` is opt-in.

**Can it install the pack for me?**
`--open` / "Open in Modrinth App" hands the `.mrpack` to the Modrinth App, which creates the instance. Direct writes into the app's database aren't attempted because that format is internal.

**The keychain isn't available (Linux server, CI).**
Use the `MODRINTH_TOKEN` and `CURSEFORGE_API_KEY` environment variables.

**macOS says the app is from an unidentified developer / Windows SmartScreen warns.**
Release builds aren't code-signed yet. The install script removes the macOS quarantine flag; on Windows choose *More info → Run anyway*.
