# Importing from CurseForge

Everything about getting a CurseForge modpack onto Modrinth in one place: what you need, what the app does with each mod, and how to resolve the rest.

## What you need

| You have | You can |
| --- | --- |
| A CurseForge **Core API key** | Search by name, paste a modpack link or ID, and get an exact, 1:1 import of any modpack `.zip` |
| No key | Import a modpack `.zip` you downloaded yourself, using the experimental [keyless lookup](#keyless-lookup-experimental) |

Create the key at <https://console.curseforge.com>. It is about 60 characters and starts with `$2a$`. It is **not** the author upload token from `authors.curseforge.com` (that one looks like `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` and only uploads files to your own projects, so the Core API rejects it with a 403). The app warns you when a saved key looks like the wrong kind. CurseForge's documentation says third-party tools may need to apply for key access through a form; see their [API page](https://docs.curseforge.com/rest-api/).

Add the key with `mrimport auth curseforge` or in **Settings → CurseForge API key**.

## Find a modpack without leaving the app

Needs the Core API key.

**App:** on the Import screen, use *Find a modpack on CurseForge*. Type a name (results appear as you type), or paste a link, a project ID or a `curseforge://` link. Pick a version (the project's main file is preselected, server packs are hidden) and press *Import this version*. The pack is downloaded, checked against the SHA-1 CurseForge lists, and imported.

**CLI:**

```sh
mrimport search "all the mods"                 # lists packs with a ready-to-run command
mrimport convert https://www.curseforge.com/minecraft/modpacks/all-the-mods-10
mrimport convert cf:925200                     # project id (latest file)
mrimport convert cf:925200:6000123             # a specific file
mrimport convert "curseforge://install?addonId=925200&fileId=6000123"
```

Accepted links: `…/minecraft/modpacks/<name>`, with `/files/<id>` or `/download/<id>` for a specific version, `…/projects/<id>`, and the CurseForge app's `curseforge://install` links. Only `curseforge.com` hosts are accepted, and a link to a mod (not a modpack) is refused with an explanation.

If the author doesn't allow downloads outside CurseForge, you get the file page link: download the `.zip` in your browser and drop it on the app (or pass it to `mrimport convert`).

## What happens to each mod

A modpack is assumed **not** to exist on Modrinth, so Modrinth is only used where it has the *identical file*:

1. **Exact match.** The file's SHA-1/SHA-512 is looked up on Modrinth. If the same file is there, the pack points at Modrinth's copy.
2. **Download.** Everything else is downloaded from its original source (CurseForge, or the cursemaven mirror in keyless mode) and **bundled** in the pack. Downloads are verified against the SHA-1 CurseForge lists, and a file that fails the check is never used.
3. **Your turn.** Anything that can't be downloaded (the author restricts it, the link is gone) is labelled **Needs your file**, with links to its CurseForge page, until you supply it.

Nothing is silently replaced by a *similar* mod. That only happens if you turn on *Match by name* (below).

### Supplying a file by hand

For each mod that needs one, the review screen shows what is expected and links to the CurseForge file page. Download the file, then:

- **App:** drop the `.jar` files (or a folder) on the window, press *Scan Downloads folder*, or use *Choose file…* on a row.
- **CLI:** `--mods-dir ~/Downloads`, or `--manual-file PROJECT_ID=path/to/file.jar`. On a terminal the CLI asks for the folder itself.

Every file is checked against the **exact** file the pack asks for:

| What is known about the wanted file | Check | Result |
| --- | --- | --- |
| SHA-1 (needs the Core API key) | hashes are equal | exact |
| Exact file name | names are equal (browser suffixes such as ` (1)` are ignored) | exact |
| Nothing | can't be checked | held back until you accept it |

A file that is verifiably a different version is refused, with the reason. *Use anyway* (app) or `--force-manual` (CLI) overrides that, and `--accept-unverified` accepts files that can't be checked. A file that turns out to be on Modrinth (by hash) is pointed at Modrinth; otherwise it is bundled as-is.

## Advanced options

Saved for both the app and the CLI. In the app they are on the Import screen and in Settings; in the CLI, see `mrimport config show` and the *Advanced options* heading of `mrimport convert --help`.

| Option (CLI key) | Default | Meaning |
| --- | --- | --- |
| Download mods that aren't on Modrinth (`download`) | on | Fetch non-matching mods from CurseForge and bundle them |
| Bundle files that aren't on Modrinth (`bundle`) | on | Put those files (and ones you supply) in the pack; off lists them in the report |
| Match by name (`match-by-name`) | **off** | Search Modrinth for a *similar* mod when there is no exact match. Substitutes different files |
| Keyless lookup (`keyless`) | off | Use community services when there's no working key |

```sh
mrimport config show
mrimport config set match-by-name on
mrimport convert pack.zip --no-download      # one-off, without changing the saved setting
```

Bundling third-party mods may need the author's permission. The defaults follow the assumption that you are importing a pack you are entitled to move; turn *download*/*bundle* off if that is not the case.

## Keyless lookup (experimental)

Without a working key, a CurseForge modpack `.zip` only lists numeric project and file IDs. With keyless lookup on, for IDs the Core API can't answer:

1. the jar is fetched from the community mirror **cursemaven.com** and hashed locally, which still allows an *exact* Modrinth match and a verified bundle;
2. for what is still unresolved, **api.cfwidget.com** supplies the project name, page link and exact file name, which makes the manual step clearer and verifiable by file name.

Cost: your pack's CurseForge project and file IDs are sent to those two third-party services, and they can change or disappear. It is off by default. Hashes are cached in your cache folder (removed by `mrimport purge`).

## Limits and known gaps

- **Checked against the real CurseForge API** (with a Core API key, on 2026-11-19): search by name, link and slug lookup, the version list, the pack download from CurseForge's CDN, the per-mod file lookup, and a complete import of Fabulously Optimized (51 mods, all exact on Modrinth). That confirmed the Minecraft game id (432) and the modpack class id (4471), which are constants in `crates/core/src/curseforge/mod.rs`.
- **Not yet checked against the real API:** a modpack or mod whose author disallows third-party downloads (no download URL), and importing a pack where many mods are *not* on Modrinth (the download-and-bundle path ran against mocks and a stand-in server). Files are sorted by the app itself because the order of a project's file list isn't specified in the documentation this was built from.
- If CurseForge changes the game or class ids, search returns nothing until the constants are updated; slug and ID lookups don't depend on the class id.
