# Publishing to Modrinth

1. Create a personal access token at <https://modrinth.com/settings/pats> with the *create projects* and *create versions* scopes, then run `mrimport auth modrinth` (or use the Settings page in the app).
2. Pick a source:
   - **Local jars**: `mrimport publish jars build/libs/my-mod-1.0.jar --version-number 1.0 ...`
   - **GitHub releases**: each non-draft release with `.jar` assets becomes a version. Minecraft versions are guessed from file and tag names (e.g. `1.20.1`) unless you pass `--game-versions`; pass `--loaders` as GitHub assets don't declare one.
   - **CurseForge project**: title and summary are copied; files whose authors disabled third-party distribution are skipped.
3. Run with `--dry-run` first to see what would be uploaded.
4. Run for real. A new project is created **as a draft**; open it on modrinth.com to add an icon, description and gallery, then submit it for review. Modrinth's content rules apply.

Re-running is safe: versions whose number or file hash already exists on the project are skipped. Use `--existing` to add versions to a project you already created.

Test against Modrinth's staging site first by setting `MODRINTH_API_BASE=https://staging-api.modrinth.com/v2` and a token from <https://staging.modrinth.com>.

Only publish work you own or have permission to redistribute.
