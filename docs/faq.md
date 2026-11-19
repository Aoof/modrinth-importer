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

**macOS says the app is damaged, or from an unidentified developer.**
Release builds are ad-hoc signed, not notarized by Apple (that needs a paid Apple Developer account). The install script clears the download quarantine flag for you. If you installed the `.dmg` by hand, run this once, then open the app normally:

```sh
xattr -dr com.apple.quarantine "/Applications/Modrinth Importer.app"
```

Alternatively right-click the app and choose *Open*. Maintainers can enable full signing and notarization by adding the repository secrets `APPLE_CERTIFICATE` (base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific password) and `APPLE_TEAM_ID`; the release workflow picks them up automatically.

**Windows SmartScreen warns.**
The Windows installer isn't code-signed. Choose *More info → Run anyway*.
