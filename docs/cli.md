# CLI reference

All commands accept `--json` for machine-readable output and `--debug-log [FILE]` to write a debug log (default `mrimport-debug.log`) that you can attach to bug reports. It never contains credentials. See [Troubleshooting](troubleshooting.md).

## `mrimport inspect <PATH>`

Detects the pack format and prints name, Minecraft version, loader, mod count and config files.
Options: `--name`, `--mc-version`, `--loader fabric|forge|neoforge|quilt`.

## `mrimport search <QUERY>`

Searches CurseForge modpacks by name (most popular first) and prints a ready-to-run `convert` command for each. Needs a CurseForge Core API key. `--limit N` (1-50, default 10).

## `mrimport convert <PATH or LINK>`

Converts a pack to `.mrpack`. `PATH` is a pack `.zip`, an instance/packwiz/mods folder, **or a CurseForge modpack link** (`https://www.curseforge.com/minecraft/modpacks/<name>[/files/<id>]`, `cf:<projectId>[:<fileId>]`, a project ID, or a `curseforge://install?addonId=…` link), which is downloaded for you (Core API key required). `inspect` accepts the same.

Only exact (hash) matches use Modrinth. Other mods are downloaded and bundled; what can't be downloaded is listed with a CurseForge link. See [Importing from CurseForge](curseforge.md).

| Option | Meaning |
| --- | --- |
| `-o, --output FILE` | Output path (default `<name>.mrpack`) |
| `--name`, `--mc-version`, `--loader` | Override detected values; `--mc-version` is required for bare mods folders |
| `--loader-version V` | Loader version to record; looked up automatically when missing |
| `--report` | Write `<output>.report.md` |
| `--open` | Open the result in the Modrinth App |
| `--mods-dir DIR` | Folder(s) with files you downloaded by hand; each is verified against the exact file the pack asks for |
| `--manual-file PROJECT_ID=PATH` | Supply a file for one mod explicitly (when nothing is known to match it by) |
| `--accept-unverified` | Accept supplied files whose exact version can't be verified |
| `--force-manual` | Accept supplied files that are verifiably a different file |

**Advanced options** (also saved with `mrimport config`):

| Option | Meaning |
| --- | --- |
| `--no-download` | Don't download mods that aren't on Modrinth; list them for manual download |
| `--no-bundle` | Don't put non-Modrinth files in the pack; list them in the report |
| `--match-by-name` | Also search Modrinth for a *similar* mod when there is no exact match (off by default) |
| `--accept-ambiguous` | With `--match-by-name`, take the best candidate when several fit |
| `--keyless` | Use the community lookup services when the CurseForge key is missing or rejected |

On a terminal, mods that still need a file are listed with links and the CLI asks for the folder to scan, then for any file it can't pair automatically.

## `mrimport config`

`config show` lists the advanced settings and their values. `config set <download|bundle|match-by-name|keyless> <on|off>` changes one. The same settings appear under *Advanced options* in the app.

## `mrimport publish`

Subcommands: `jars <FILES...> --version-number V`, `github <owner/repo>`, `curseforge <PROJECT_ID>`.

Shared options: `--slug`, `--title`, `--summary`, `--body`, `--license` (SPDX id), `--categories a,b`, `--client-side`, `--server-side`, `--game-versions 1.20.1,1.20.4`, `--loaders fabric,forge`, `--existing` (project already exists), `--dry-run`.

## `mrimport auth`

`auth modrinth`, `auth curseforge` (prompt hidden; or `--token`/`--key`, or pipe on stdin), `auth status`, `auth logout`.

## `mrimport doctor`

Checks that Modrinth is reachable, shows where your Modrinth token and CurseForge key come from and whether the key's shape looks right, and tests the CurseForge key against the API. Always writes `mrimport-debug.log`.

## `mrimport purge [--yes] [--dry-run]`

Removes everything the app stores outside its program files: settings, caches, webview data, saved state and the credentials in your OS keychain. `--dry-run` only lists them. Without `--yes` it asks first and refuses to run non-interactively. Program files are removed by the installers' `--uninstall`.

## `mrimport self-update [--check]`

Downloads the newest release binary for your platform, verifies its SHA-256 against the release's `SHA256SUMS`, and replaces the running executable.

## Environment

| Variable | Purpose |
| --- | --- |
| `MODRINTH_TOKEN` | Modrinth personal access token (overrides keychain) |
| `CURSEFORGE_API_KEY` | CurseForge API key (overrides keychain) |
| `MRIMPORT_NO_KEYCHAIN` | Set to `1` to never touch the OS keychain (containers, CI) |
| `GITHUB_TOKEN` | Optional; raises GitHub API rate limits when publishing from releases |
| `MODRINTH_API_BASE` | Use another API, e.g. `https://staging-api.modrinth.com/v2` |
