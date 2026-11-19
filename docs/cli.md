# CLI reference

All commands accept `--json` for machine-readable output and `--debug-log [FILE]` to write a debug log (default `mrimport-debug.log`) that you can attach to bug reports. It never contains credentials. See [Troubleshooting](troubleshooting.md).

## `mrimport inspect <PATH>`

Detects the pack format and prints name, Minecraft version, loader, mod count and config files.
Options: `--name`, `--mc-version`, `--loader fabric|forge|neoforge|quilt`.

## `mrimport convert <PATH>`

Converts a pack to `.mrpack`.

| Option | Meaning |
| --- | --- |
| `-o, --output FILE` | Output path (default `<name>.mrpack`) |
| `--name`, `--mc-version`, `--loader` | Override detected values; `--mc-version` is required for bare mods folders |
| `--loader-version V` | Loader version to record; looked up automatically when missing |
| `--no-bundle` | Don't bundle mods that Modrinth doesn't host |
| `--download-remote` | Download remote-only unmatched mods (e.g. from the CurseForge CDN) and bundle them. Check each mod's license first |
| `--accept-ambiguous` | Take the best candidate when several projects fit a mod's name |
| `--report` | Write `<output>.report.md` |
| `--open` | Open the result in the Modrinth App |

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
| `GITHUB_TOKEN` | Optional; raises GitHub API rate limits when publishing from releases |
| `MODRINTH_API_BASE` | Use another API, e.g. `https://staging-api.modrinth.com/v2` |
