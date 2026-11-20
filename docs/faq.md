# FAQ

**Why isn't a mod on Modrinth used?**
Only a file that is *identical* to one on Modrinth (same hash) is taken from there. A modpack is assumed not to exist on Modrinth, so the other mods are downloaded from their source and bundled in the pack rather than swapped for similar ones. *Match by name* (advanced, off by default) searches Modrinth for a similar mod instead; those matches are marked *By name* and are not the same file.

**Why do I need a CurseForge API key?**
A CurseForge modpack `.zip` only lists numeric project and file IDs. The Core API turns them into file names, hashes and download links, which is what makes the import exact. Keys are free at <https://console.curseforge.com> (CurseForge may ask third-party tools to apply). Without a key you can import a downloaded `.zip` using the experimental *keyless lookup*, or other sources, which need no key. Search and link import need the key. See [Importing from CurseForge](curseforge.md).

**Some mods say "Needs your file".**
They aren't on Modrinth and couldn't be downloaded (for example the author restricts downloads to CurseForge). Open the link, download the exact file, and drop it on the app (or use `--mods-dir`). The file is verified against the exact one the pack asks for. Mods you leave out are listed in the report with their links.

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

**Windows says the file "contains a virus or potentially unwanted software".**
That's Microsoft Defender flagging an unsigned developer build, a false positive. The PowerShell installer tells you which file was blocked and keeps going with the rest. To install it anyway, open *Windows Security → Virus & threat protection → Protection history*, select the blocked item, check that it names `mrimport-x86_64-pc-windows-msvc.exe` or `ModrinthImporter-windows-x64-setup.exe`, choose *Actions → Allow on device*, then run the installer again. You can compare the file with `SHA256SUMS` on the release first. Reporting it to Microsoft as an incorrect detection ([file submission](https://www.microsoft.com/wdsi/filesubmission)) helps clear it for everyone. Don't turn off real-time protection to get around it.

**What does uninstalling remove?**
Everything the app created: the `mrimport` binary, the desktop app, its launcher entry (Linux) or Start menu entry and registry keys (Windows), settings, caches, webview data, saved window state, and the Modrinth/CurseForge credentials in your keychain. Your packs, exported `.mrpack` files and the Modrinth App are never touched. On Windows the NSIS uninstaller for the app is run first and the `mrimport` folder and its PATH entry are removed afterwards.

**Why is every release a "developer build"?**
Releases are published as pre-releases until the macOS and Windows builds can be signed. The installers and `mrimport self-update` use the newest release, pre-releases included.
