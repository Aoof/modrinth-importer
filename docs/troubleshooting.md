# Troubleshooting

## CurseForge returns 403 (or 401)

The CurseForge API rejected your key. Common causes, most likely first:

1. **The `$` characters in the key were eaten by your shell.** CurseForge keys look like `$2a$10$…` (about 60 characters). In zsh/bash, `export CURSEFORGE_API_KEY="$2a$10$abc…"` expands `$2a` and `$10` to nothing. Use single quotes, or let the tool prompt you so the shell is never involved:

   ```sh
   export CURSEFORGE_API_KEY='$2a$10$…'   # single quotes
   mrimport auth curseforge               # or paste at the hidden prompt
   ```

   In the desktop app, paste the key into **Settings → CurseForge API key**.
2. The key was copied incompletely or with extra whitespace (leading/trailing whitespace is trimmed automatically).
3. The key is for another service, or was regenerated or revoked in the [CurseForge console](https://console.curseforge.com).

Run `mrimport doctor` (or **Settings → Diagnostics → Run check** in the app). It tells you where the key comes from (environment variable or keychain), whether its shape looks right, and whether the API accepts it.

## Collecting a debug log for a bug report

The log records API calls (method, URL, status, timing, rate-limit and Cloudflare headers, truncated error bodies) and your OS and app version. It never contains your keys or tokens, and your home folder is shown as `~`. It does include mod names and file names from the pack you were converting, so skim it before posting if that matters.

- **CLI:** add `--debug-log` to any command, e.g. `mrimport convert pack.zip --debug-log`, or run `mrimport doctor`. The log is written to `mrimport-debug.log` in the current folder (or pass a path: `--debug-log my.log`).
- **Desktop app:** **Settings → Diagnostics** → *Save debug log…* or *Copy log*.

Attach the file to a [new issue](https://github.com/Aoof/modrinth-importer/issues/new/choose).

## Other common problems

| Symptom | Fix |
| --- | --- |
| `could not determine the Minecraft version` | Bare mods folder: pass `--mc-version 1.20.1` (and `--loader` if needed). |
| `keychain` errors on Linux or CI | Use `MODRINTH_TOKEN` / `CURSEFORGE_API_KEY` environment variables. |
| Mods listed as skipped | They aren't on Modrinth and no jar was available; see the report and [Providers](providers.md). |
