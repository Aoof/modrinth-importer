# Modrinth Importer

Bring your mods and modpacks to [Modrinth](https://modrinth.com) from other providers. One app, two interfaces: a desktop GUI and a scriptable `mrimport` CLI.

> Unofficial community project. Not affiliated with or endorsed by Modrinth or CurseForge.

## What it does

**Import packs (for players).** Point it at a pack and get a Modrinth `.mrpack` you can open in the Modrinth App. Every mod is matched to Modrinth by file hash first, then by name for the rest. Mods that Modrinth doesn't host can be bundled into the pack, and you get a report of what was skipped.

| Source | Input |
| --- | --- |
| CurseForge | modpack `.zip` (needs a free [API key](https://console.curseforge.com)) or a CurseForge app instance folder |
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

Options: `--cli-only`, `--gui-only`, `--version X.Y.Z`, `--system` (CLI to `/usr/local/bin`), `--uninstall`. Pass them as `sh -s -- --cli-only` when piping, or download the script first. Prefer manual installs? Grab files from [Releases](https://github.com/Aoof/modrinth-importer/releases).

Update the CLI with `mrimport self-update`. To update the desktop app, run the installer again.

## Quick start

```sh
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

In the desktop app, drop a pack onto the window, review the matches, and export.

Credentials: `mrimport auth modrinth` and `mrimport auth curseforge` store them in your OS keychain. `MODRINTH_TOKEN` and `CURSEFORGE_API_KEY` environment variables also work (useful in CI).

More: [CLI reference](docs/cli.md) · [Providers](docs/providers.md) · [Publishing](docs/publishing.md) · [How matching works](docs/architecture.md) · [FAQ](docs/faq.md)

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
