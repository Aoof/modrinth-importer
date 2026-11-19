# Supported sources

Detection is automatic: give `convert`/`inspect` a `.zip` or a folder and the format is recognised from its files. A zip that wraps everything in one top-level folder works too.

| Source | Recognised by | Notes |
| --- | --- | --- |
| CurseForge modpack | `manifest.json` with `manifestType: minecraftModpack` | A Core API key looks up exact file hashes and names; without one, use the experimental keyless lookup. `overrides/` is carried over. Can also be fetched from a link or search: see [Importing from CurseForge](curseforge.md). |
| CurseForge app instance | `minecraftinstance.json` | Uses the jars on disk; no API key needed. |
| Prism / MultiMC | `mmc-pack.json` | Minecraft version and loader from the components list. Game folder is `.minecraft` or `minecraft`. |
| ATLauncher | `instance.json` with a `launcher` object | |
| FTB App | `instance.json` with `uuid`/`mcVersion` | Best effort; the format isn't formally documented. Pass `--mc-version`/`--loader` if detection misses. |
| Technic | `bin/version.json` or `bin/modpack.jar` | Best effort. |
| packwiz | `pack.toml` | Reads `*.pw.toml` metadata (URL + hash). Side hints become client/server env. |
| Mods folder | a folder of `.jar` files | The loader is inferred from jar metadata; Minecraft version must be given. |

Carried-over non-mod files: `config`, `defaultconfigs`, `kubejs`, `scripts`, `resourcepacks`, `shaderpacks`, `datapacks`, `options.txt`, `servers.dat`. Worlds and saves are never included.

Disabled mods (`*.jar.disabled`) are ignored.
