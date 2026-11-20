# Modrinth Importer

Bring your mods and modpacks to [Modrinth](https://modrinth.com) from other providers. One app, two interfaces: a desktop GUI and a scriptable `mrimport` CLI.

> Unofficial community project. Not affiliated with or endorsed by Modrinth or CurseForge.

## What it does

**Import packs (for players).** Point it at a pack, paste a CurseForge modpack link, or search CurseForge by name, and get a Modrinth `.mrpack` you can open in the Modrinth App. A modpack is assumed not to exist on Modrinth, so only *exact* file matches use Modrinth. Every other mod is downloaded from its source and bundled, and anything that can't be downloaded is labelled with a link so you can supply the exact file. Name-matching to similar mods is an opt-in advanced setting. [How it works →](docs/curseforge.md)

| Source | Input |
| --- | --- |
| CurseForge | a modpack link, ID or name search (needs a Core API key), a modpack `.zip`, or an app instance folder |
| Prism Launcher / MultiMC | instance folder or exported `.zip` |
| ATLauncher | instance folder |
| FTB App | instance folder (best effort) |
| Technic | pack folder (best effort) |
| packwiz | pack folder or `.zip` with `pack.toml` |
| Any `mods/` folder | folder of jars (you supply the Minecraft version) |

**Publish projects (for creators).** Create a Modrinth project and upload its versions from local jars, a GitHub repository's releases, or a CurseForge project you own. New projects are created as drafts, and versions that already exist are skipped, so re-running is safe.

## Install

macOS / Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1 | iex
```

The installers verify SHA-256 checksums and put things where each OS expects them:

| | CLI | Desktop app |
| --- | --- | --- |
| macOS | `~/.local/bin/mrimport` | `/Applications/Modrinth Importer.app` (or `~/Applications`) |
| Linux | `~/.local/bin/mrimport` | `~/.local/bin/ModrinthImporter.AppImage` + launcher entry |
| Windows | `%LOCALAPPDATA%\Programs\ModrinthImporter\bin\mrimport.exe` (added to PATH) | Start menu via the installer |

Options: `--cli-only`, `--gui-only`, `--version X.Y.Z`, `--system` (CLI to `/usr/local/bin`), `--uninstall`, `--keep-data`. Pass them as `sh -s -- --cli-only` when piping, or download the script first. Prefer manual installs? Grab files from [Releases](https://github.com/Aoof/modrinth-importer/releases).

Update the CLI with `mrimport self-update`. To update the desktop app, run the installer again.

> **Developer builds.** Every release is currently marked as a pre-release: the macOS app isn't notarized and the Windows installer isn't signed. The macOS installer seals the app with a local signature so it opens cleanly; see the [FAQ](docs/faq.md) for first-launch prompts when downloading manually.

## Uninstall

Removes the CLI, the desktop app, settings, caches, webview data and stored credentials. The installers never touch your PATH or shell profile on macOS and Linux, so there is nothing to undo there.

```sh
curl -fsSL https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.sh | sh -s -- --uninstall
```

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Aoof/modrinth-importer/master/install/install.ps1))) -Uninstall
```

Add `--keep-data` (`-KeepData` on Windows) to keep settings and credentials. If you only want to clear data and keep the programs, run `mrimport purge` (`--dry-run` lists what it would remove).

## Quick start

```sh
# Find a modpack on CurseForge and import it (needs a Core API key)
mrimport search "all the mods"
mrimport convert https://www.curseforge.com/minecraft/modpacks/all-the-mods-10 --open

# What's in this pack?
mrimport inspect ~/Downloads/MyPack.zip

# Convert it
mrimport convert ~/Downloads/MyPack.zip -o MyPack.mrpack --report --open

# Bare mods folder: say which Minecraft version it targets
mrimport convert ~/.minecraft/mods --mc-version 1.20.1

# Publish a mod from GitHub releases (dry run first)
mrimport publish github me/my-mod --slug my-mod --title "My Mod" \
  --summary "Does a thing" --license MIT --loaders fabric --dry-run
```

In the desktop app, drop a pack onto the window (or use *Find a modpack on CurseForge*), review the matches, supply any files it asks for, and export.

Credentials: `mrimport auth modrinth` and `mrimport auth curseforge` store them in your OS keychain. `MODRINTH_TOKEN` and `CURSEFORGE_API_KEY` environment variables also work (useful in CI).

More: [CurseForge](docs/curseforge.md) · [CLI reference](docs/cli.md) · [Providers](docs/providers.md) · [Publishing](docs/publishing.md) · [How matching works](docs/architecture.md) · [Troubleshooting](docs/troubleshooting.md) · [FAQ](docs/faq.md)

## Build from source

Requires Rust (stable), Node 24 and pnpm.

```sh
pnpm --dir ui install
cargo run -p mrimport-cli -- --help      # CLI
cargo tauri dev                          # desktop app (cargo install tauri-cli --locked)
cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model.

## License

[MIT](LICENSE)
