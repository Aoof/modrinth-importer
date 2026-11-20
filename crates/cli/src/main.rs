//! `mrimport` — import mods and modpacks from other providers to Modrinth.

mod update;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use mrimport_core::{
    cflookup::{Lookup, LookupBases},
    cfmodpack,
    config::{self, Secret},
    curseforge::CurseForge,
    debuglog, diagnostics,
    events::{Event, Reporter},
    export::mrpack::{open_with_default_app, write_mrpack, ExportOptions},
    fetch,
    manual::{self, ManualOptions},
    model::Loader,
    modrinth::Modrinth,
    providers::{self, LoadCtx, LoadOptions},
    publish::{self, ProjectSpec, PublishPlan, ReleaseDefaults},
    resolve::{self, Resolution},
};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "mrimport", version, about, long_about = None)]
struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
    /// Override the Modrinth API base URL (e.g. https://staging-api.modrinth.com/v2).
    #[arg(long, global = true, env = "MODRINTH_API_BASE", hide = true)]
    api_base: Option<String>,
    /// Use community lookup services (cursemaven.com, api.cfwidget.com) when the CurseForge key
    /// is missing or rejected. Experimental. Make it permanent: `mrimport config set keyless on`.
    #[arg(long, global = true, help_heading = "Advanced options")]
    keyless: bool,
    /// Write a debug log (API calls, statuses, errors; no credentials) to FILE
    /// (default: mrimport-debug.log) to attach to bug reports.
    #[arg(long, global = true, value_name = "FILE", num_args = 0..=1,
          default_missing_value = "mrimport-debug.log", alias = "debug")]
    debug_log: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show what a pack, instance or mods folder contains.
    Inspect {
        /// A pack, instance or mods folder, or (with a CurseForge API key) a CurseForge modpack link.
        path: PathBuf,
        #[command(flatten)]
        load: LoadArgs,
    },
    /// Convert a pack to a Modrinth `.mrpack`.
    Convert(ConvertArgs),
    /// Search CurseForge modpacks by name (needs a CurseForge Core API key).
    Search {
        /// What to look for, e.g. `all the mods`.
        #[arg(required = true)]
        query: Vec<String>,
        /// How many results to show (1-50).
        #[arg(long, default_value_t = 10)]
        limit: u32,
    },
    /// Publish a project and its versions to Modrinth.
    Publish(PublishArgs),
    /// Store or remove API credentials in the OS keychain.
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// Change persistent settings.
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Check connectivity and credentials and write a debug log for bug reports.
    Doctor,
    /// Remove stored credentials, settings, caches and app data (not the program files).
    Purge {
        /// Don't ask for confirmation.
        #[arg(short, long)]
        yes: bool,
        /// List what would be removed without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Update this executable to the latest release.
    SelfUpdate {
        /// Only check, don't install.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Args, Clone)]
struct LoadArgs {
    /// Pack display name.
    #[arg(long)]
    name: Option<String>,
    /// Minecraft version, required when it cannot be detected (bare mods folders).
    #[arg(long)]
    mc_version: Option<String>,
    #[arg(long, value_enum)]
    loader: Option<LoaderArg>,
}

#[derive(Args)]
struct ConvertArgs {
    /// `.zip` pack, launcher instance folder, packwiz folder or `mods` folder, or a CurseForge
    /// modpack link (`https://www.curseforge.com/minecraft/modpacks/<name>`, `cf:<projectId>`).
    /// Links need a CurseForge Core API key and are downloaded for you.
    path: PathBuf,
    /// Output file (default: `<name>.mrpack` in the current directory).
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    load: LoadArgs,
    /// Loader version to record in the pack (looked up automatically if omitted).
    #[arg(long)]
    loader_version: Option<String>,
    /// Don't bundle files that Modrinth doesn't host (list them in the report instead).
    #[arg(long, help_heading = "Advanced options")]
    no_bundle: bool,
    /// Don't download mods that aren't an exact match on Modrinth; they are listed for manual
    /// download instead. (By default they are downloaded from CurseForge and bundled.)
    #[arg(long, help_heading = "Advanced options")]
    no_download: bool,
    /// Also search Modrinth by name for a *similar* mod when there is no exact match. Off by
    /// default: a modpack is assumed not to exist on Modrinth, and this substitutes other files.
    #[arg(long, help_heading = "Advanced options")]
    match_by_name: bool,
    /// Deprecated: downloading is now the default; see --no-download.
    #[arg(long, hide = true)]
    download_remote: bool,
    /// With --match-by-name, accept the best candidate when a mod has several possible matches.
    #[arg(long, help_heading = "Advanced options")]
    accept_ambiguous: bool,
    /// Write a Markdown report next to the output.
    #[arg(long)]
    report: bool,
    /// Open the result in the Modrinth App when done.
    #[arg(long)]
    open: bool,
    /// Folder(s) with files you downloaded by hand for mods that couldn't be found
    /// automatically. Files are verified against the exact file the pack asks for.
    #[arg(long, value_name = "DIR")]
    mods_dir: Vec<PathBuf>,
    /// Supply a file for one mod explicitly: PROJECT_ID=PATH (repeatable). Needed when the pack
    /// gives nothing to match a file by (no hash and no file name known).
    #[arg(long, value_name = "PROJECT_ID=PATH")]
    manual_file: Vec<String>,
    /// Accept manually supplied files whose exact version can't be verified.
    #[arg(long)]
    accept_unverified: bool,
    /// Accept manually supplied files that are verifiably a different file. Use with care.
    #[arg(long)]
    force_manual: bool,
}

#[derive(Args)]
struct PublishArgs {
    #[command(subcommand)]
    source: PublishSource,
}

#[derive(Subcommand)]
enum PublishSource {
    /// Publish local jar files.
    Jars {
        files: Vec<PathBuf>,
        #[arg(long)]
        version_number: String,
        #[command(flatten)]
        common: PublishCommon,
    },
    /// Publish `.jar` assets from a GitHub repository's releases (`owner/repo`).
    Github {
        repo: String,
        #[command(flatten)]
        common: PublishCommon,
    },
    /// Migrate a CurseForge project (numeric id) you own.
    Curseforge {
        project_id: u64,
        #[command(flatten)]
        common: PublishCommon,
    },
}

#[derive(Args)]
struct PublishCommon {
    #[arg(long)]
    slug: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    /// Markdown description (default: the summary).
    #[arg(long)]
    body: Option<String>,
    /// SPDX license id.
    #[arg(long, default_value = "LicenseRef-All-Rights-Reserved")]
    license: String,
    #[arg(long, value_delimiter = ',')]
    categories: Vec<String>,
    #[arg(long, default_value = "optional")]
    client_side: String,
    #[arg(long, default_value = "optional")]
    server_side: String,
    #[arg(long, value_delimiter = ',')]
    game_versions: Vec<String>,
    #[arg(long, value_delimiter = ',', value_enum)]
    loaders: Vec<LoaderArg>,
    /// The project already exists on Modrinth; only upload new versions.
    #[arg(long)]
    existing: bool,
    /// Show the plan without creating or uploading anything.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Show the advanced settings and their current values.
    Show,
    /// Change an advanced setting: download, bundle, match-by-name or keyless.
    Set {
        key: String,
        #[arg(value_enum)]
        state: OnOff,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Save a Modrinth personal access token (needs create-project/version scopes).
    Modrinth {
        #[arg(long)]
        token: Option<String>,
    },
    /// Save a CurseForge API key (https://console.curseforge.com).
    Curseforge {
        #[arg(long)]
        key: Option<String>,
    },
    /// Show which credentials are stored.
    Status,
    /// Remove stored credentials.
    Logout,
}

#[derive(Clone, Copy, ValueEnum)]
enum LoaderArg {
    Forge,
    Neoforge,
    Fabric,
    Quilt,
}

impl From<LoaderArg> for Loader {
    fn from(l: LoaderArg) -> Self {
        match l {
            LoaderArg::Forge => Loader::Forge,
            LoaderArg::Neoforge => Loader::NeoForge,
            LoaderArg::Fabric => Loader::Fabric,
            LoaderArg::Quilt => Loader::Quilt,
        }
    }
}

fn reporter(quiet: bool) -> Reporter {
    if quiet {
        return Reporter::silent();
    }
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg:<48} [{bar:30.green/black}] {pos}/{len}",
        )
        .unwrap()
        .progress_chars("=> "),
    );
    Reporter::new(move |e| match e {
        Event::Stage { name } => {
            bar.set_length(0);
            bar.set_position(0);
            bar.set_message(name);
            bar.enable_steady_tick(std::time::Duration::from_millis(100));
        }
        Event::Progress { done, total } => {
            bar.set_length(total as u64);
            bar.set_position(done as u64);
        }
        Event::Log { message } => bar.println(message),
    })
}

fn modrinth(cli_base: &Option<String>, need_token: bool) -> Result<Modrinth> {
    let token = config::get_secret(Secret::ModrinthToken);
    if need_token && token.is_none() {
        bail!("no Modrinth token found. Run `mrimport auth modrinth` or set MODRINTH_TOKEN.");
    }
    Ok(match cli_base {
        Some(b) => Modrinth::with_base(b.clone(), token),
        None => Modrinth::new(token),
    })
}

fn read_secret(arg: Option<String>, prompt: &str) -> Result<String> {
    if let Some(v) = arg {
        return Ok(v);
    }
    if std::io::stdin().is_terminal() {
        return Ok(rpassword::prompt_password(prompt)?);
    }
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s.trim().to_string())
}

static DEBUG_LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn enable_debug_log(path: &std::path::Path) {
    match debuglog::enable_file(path) {
        Ok(()) => {
            let _ = DEBUG_LOG_PATH.set(path.to_path_buf());
        }
        Err(e) => eprintln!("warning: could not write debug log {}: {e}", path.display()),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        match DEBUG_LOG_PATH.get() {
            Some(p) => eprintln!(
                "\nDebug log saved to {}. Attach it to your bug report; it contains no credentials.",
                p.display()
            ),
            None => eprintln!("\nTip: rerun with --debug-log to write a log you can attach to a bug report, or run `mrimport doctor`."),
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let wants_log = cli.debug_log.clone().or_else(|| {
        matches!(cli.command, Cmd::Doctor).then(|| PathBuf::from("mrimport-debug.log"))
    });
    if let Some(p) = wants_log {
        enable_debug_log(&p);
    }
    match &cli.command {
        Cmd::Inspect { path, load } => inspect(&cli, path, load).await,
        Cmd::Convert(a) => convert(&cli, a).await,
        Cmd::Search { query, limit } => search(&cli, &query.join(" "), *limit).await,
        Cmd::Publish(a) => publish_cmd(&cli, a).await,
        Cmd::Auth { action } => auth(action),
        Cmd::Config { action } => config_cmd(action),
        Cmd::Doctor => doctor(&cli).await,
        Cmd::Purge { yes, dry_run } => purge(*yes, *dry_run),
        Cmd::SelfUpdate { check } => update::run(*check).await,
    }
}

fn load_opts(l: &LoadArgs) -> LoadOptions {
    LoadOptions {
        name: l.name.clone(),
        mc_version: l.mc_version.clone(),
        loader: l.loader.map(Into::into),
    }
}

fn keyless_enabled(cli: &Cli) -> bool {
    cli.keyless || config::load_settings().keyless_lookup
}

/// Saved advanced settings, overridden by command-line flags.
struct Advanced {
    keyless: bool,
    match_by_name: bool,
    download: bool,
    bundle: bool,
}

fn advanced(cli: &Cli, a: &ConvertArgs) -> Advanced {
    let s = config::load_settings();
    Advanced {
        keyless: cli.keyless || s.keyless_lookup,
        match_by_name: a.match_by_name || s.match_by_name,
        download: !a.no_download && s.download_unmatched,
        bundle: !a.no_bundle && s.bundle_unmatched,
    }
}

/// Deletes the folder of downloaded mods when conversion ends, however it ends.
struct TempDownloads(PathBuf);
impl Drop for TempDownloads {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lookup(cli: &Cli) -> Lookup {
    let mut bases = LookupBases::default();
    // Test hooks: point the keyless services at local stand-ins.
    if let Ok(u) = std::env::var("MRIMPORT_CURSEMAVEN_BASE") {
        bases.cursemaven = u;
    }
    if let Ok(u) = std::env::var("MRIMPORT_CFWIDGET_BASE") {
        bases.cfwidget = u;
    }
    Lookup::new(
        config::get_secret(Secret::CurseForgeKey).map(cf_client),
        keyless_enabled(cli),
    )
    .with_bases(bases)
    .with_cache_dir(config::cache_dir())
}

fn load_ctx(cli: &Cli, rep: Reporter) -> LoadCtx {
    LoadCtx::with_lookup(lookup(cli), rep)
}

async fn inspect(cli: &Cli, path: &std::path::Path, load: &LoadArgs) -> Result<()> {
    let (path, _dl) = materialize(cli, path, &reporter(cli.json)).await?;
    let pack = providers::load(&path, &load_opts(load), &load_ctx(cli, Reporter::silent())).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&pack)?);
        return Ok(());
    }
    println!("{} {}", pack.name, pack.version);
    println!("  source:    {:?}", pack.source);
    println!("  minecraft: {}", pack.mc_version);
    println!(
        "  loader:    {} {}",
        pack.loader.modrinth_id(),
        pack.loader_version.clone().unwrap_or_default()
    );
    println!("  mods:      {}", pack.mods.len());
    println!("  overrides: {} files", pack.overrides.len());
    Ok(())
}

async fn convert(cli: &Cli, a: &ConvertArgs) -> Result<()> {
    let rep = reporter(cli.json);
    let (pack_path, _pack_dl) = materialize(cli, &a.path, &rep).await?;
    let mut pack =
        providers::load(&pack_path, &load_opts(&a.load), &load_ctx(cli, rep.clone())).await?;
    let mr = modrinth(&cli.api_base, false)?;
    let lk = lookup(cli);
    let adv = advanced(cli, a);
    if !cli.json {
        let on = |b: bool| if b { "on" } else { "off" };
        println!(
            "Advanced options: download mods not on Modrinth {} | bundle them {} | match by name {} | keyless lookup {}  (see `mrimport config show`)",
            on(adv.download), on(adv.bundle), on(adv.match_by_name), on(adv.keyless)
        );
    }
    let mut resolved = resolve::resolve_with(
        &pack,
        &mr,
        &rep,
        resolve::ResolveOptions {
            enricher: Some(&lk),
            fuzzy: adv.match_by_name,
        },
    )
    .await?;
    // Only exact matches come from Modrinth; fetch everything else from its source.
    let downloads =
        TempDownloads(std::env::temp_dir().join(format!("mrimport-dl-{}", std::process::id())));
    if adv.download {
        let r = fetch::fetch_unmatched(&mut resolved, &downloads.0, &mr, &rep).await?;
        if !cli.json && (r.fetched > 0 || !r.failed.is_empty()) {
            println!(
                "Downloaded {} mod(s) that aren't on Modrinth ({} found on Modrinth after all, {} couldn't be downloaded).",
                r.fetched, r.found_on_modrinth, r.failed.len()
            );
        }
    }
    manual_stage(cli, a, &mr, &mut resolved).await?;
    if a.accept_ambiguous {
        for r in &mut resolved {
            if let Resolution::Ambiguous { candidates } = &r.resolution {
                r.resolution = Resolution::Matched {
                    candidate: candidates[0].clone(),
                    confidence: resolve::Confidence::Fuzzy,
                };
            }
        }
    }
    let out = a.output.clone().unwrap_or_else(|| {
        let safe: String = pack
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        PathBuf::from(format!("{safe}.mrpack"))
    });
    if let Some(v) = &a.loader_version {
        pack.loader_version = Some(v.clone());
    }
    let opts = ExportOptions {
        bundle_unmatched: adv.bundle,
        download_remote: adv.download || a.download_remote,
        loader_version: a.loader_version.clone(),
    };
    let report = write_mrpack(&pack, &resolved, &out, &opts, &mr, &rep).await?;
    if a.report {
        std::fs::write(out.with_extension("report.md"), report.to_markdown())?;
    }
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("\nWrote {}", out.display());
        println!(
            "  {} matched by hash, {} by name, {} bundled, {} skipped",
            report.matched_exact,
            report.matched_fuzzy,
            report.bundled.len(),
            report.skipped.len()
        );
        for s in &report.skipped {
            println!("  - skipped {}: {}", s.file_name, s.reason);
        }
    }
    if a.open {
        open_with_default_app(&out).context("opening the Modrinth App")?;
    }
    Ok(())
}

async fn publish_cmd(cli: &Cli, a: &PublishArgs) -> Result<()> {
    let rep = reporter(cli.json);
    let (common, project, releases) = match &a.source {
        PublishSource::Jars {
            files,
            version_number,
            common,
        } => {
            let d = defaults(common);
            let releases = publish::releases_from_jars(files, &d, version_number)?;
            (common, None, releases)
        }
        PublishSource::Github { repo, common } => {
            let d = defaults(common);
            (common, None, publish::releases_from_github(repo, &d).await?)
        }
        PublishSource::Curseforge { project_id, common } => {
            let key = config::get_secret(Secret::CurseForgeKey)
                .context("no CurseForge API key. Run `mrimport auth curseforge`.")?;
            let (p, r) = publish::releases_from_curseforge(
                &CurseForge::new(key),
                *project_id,
                &defaults(common),
            )
            .await?;
            (common, Some(p), r)
        }
    };
    let base = project.unwrap_or(ProjectSpec {
        title: String::new(),
        slug: String::new(),
        summary: String::new(),
        body: String::new(),
        categories: vec![],
        client_side: "optional".into(),
        server_side: "optional".into(),
        license_id: String::new(),
        source_url: None,
        issues_url: None,
    });
    let summary = common.summary.clone().unwrap_or(base.summary);
    let spec = ProjectSpec {
        title: common.title.clone().unwrap_or(base.title),
        slug: common.slug.clone().unwrap_or(base.slug),
        body: common.body.clone().unwrap_or_else(|| {
            if base.body.is_empty() {
                summary.clone()
            } else {
                base.body
            }
        }),
        summary,
        categories: if common.categories.is_empty() {
            base.categories
        } else {
            common.categories.clone()
        },
        client_side: common.client_side.clone(),
        server_side: common.server_side.clone(),
        license_id: common.license.clone(),
        source_url: base.source_url,
        issues_url: base.issues_url,
    };
    let plan = PublishPlan {
        create_project: !common.existing,
        project: spec,
        releases,
    };
    let mr = modrinth(&cli.api_base, !common.dry_run)?;
    let report = publish::publish(&plan, &mr, common.dry_run, &rep).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "\n{}: {} version(s) {}, {} skipped",
            if report.dry_run { "Dry run" } else { "Done" },
            report.uploaded.len(),
            if report.dry_run {
                "would be uploaded"
            } else {
                "uploaded"
            },
            report.skipped.len()
        );
        for s in &report.skipped {
            println!("  - skipped {s}");
        }
        if let Some(id) = &report.project_id {
            println!("Project: https://modrinth.com/project/{id}  (created as a draft; submit it for review on the site)");
        }
    }
    Ok(())
}

fn defaults(c: &PublishCommon) -> ReleaseDefaults {
    ReleaseDefaults {
        game_versions: c.game_versions.clone(),
        loaders: c.loaders.iter().copied().map(Into::into).collect(),
        version_type: None,
    }
}

fn auth(action: &AuthCmd) -> Result<()> {
    match action {
        AuthCmd::Modrinth { token } => {
            let t = read_secret(token.clone(), "Modrinth token: ")?;
            config::set_secret(Secret::ModrinthToken, t.trim())?;
            println!("Modrinth token saved to the OS keychain.");
        }
        AuthCmd::Curseforge { key } => {
            let k = read_secret(key.clone(), "CurseForge API key: ")?;
            config::set_secret(Secret::CurseForgeKey, k.trim())?;
            println!("CurseForge API key saved to the OS keychain.");
            if let Some(w) = debuglog::key_warning(&k) {
                eprintln!("\nwarning: {w}");
            }
        }
        AuthCmd::Status => {
            for (name, s) in [
                ("Modrinth token", Secret::ModrinthToken),
                ("CurseForge key", Secret::CurseForgeKey),
            ] {
                println!(
                    "{name}: {}",
                    if config::get_secret(s).is_some() {
                        "set"
                    } else {
                        "not set"
                    }
                );
            }
        }
        AuthCmd::Logout => {
            let _ = config::delete_secret(Secret::ModrinthToken);
            let _ = config::delete_secret(Secret::CurseForgeKey);
            println!("Stored credentials removed.");
        }
    }
    Ok(())
}

fn purge(yes: bool, dry_run: bool) -> Result<()> {
    let paths = config::local_data_paths();
    for p in &paths {
        println!("  {}", p.display());
    }
    // Credentials are never read here: just deleted, so the OS doesn't prompt for keychain access.
    println!("  stored credentials in the OS keychain (if any)");
    if dry_run {
        return Ok(());
    }
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("refusing to delete without confirmation; pass --yes");
        }
        eprint!("Remove the items above? [y/N] ");
        let mut a = String::new();
        std::io::stdin().read_line(&mut a)?;
        if !a.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }
    let n = config::purge_secrets();
    let removed = config::purge_local_data()?;
    println!("Removed {} item(s) and {n} credential(s).", removed.len());
    Ok(())
}

async fn doctor(cli: &Cli) -> Result<()> {
    let mr = modrinth(&cli.api_base, false)?;
    println!("{}", diagnostics::run(&mr, keyless_enabled(cli)).await);
    if let Some(p) = DEBUG_LOG_PATH.get() {
        println!(
            "\nDebug log saved to {} (no credentials; home folder shown as ~). Attach it to your bug report.",
            p.display()
        );
    }
    Ok(())
}

fn print_settings(s: &config::Settings) {
    for i in s.describe() {
        println!(
            "{:<14} {}\n               {}\n               {}\n",
            i.key,
            if i.value { "ON" } else { "off" },
            i.title,
            i.description
        );
    }
}

fn config_cmd(action: &ConfigCmd) -> Result<()> {
    let mut s = config::load_settings();
    match action {
        ConfigCmd::Show => {
            println!("Advanced settings (change with `mrimport config set <name> on|off`):\n");
            print_settings(&s);
        }
        ConfigCmd::Set { key, state } => {
            if !s.set(key, matches!(state, OnOff::On)) {
                let keys: Vec<_> = s.describe().iter().map(|i| i.key).collect();
                bail!("unknown setting '{key}'. Available: {}", keys.join(", "));
            }
            config::save_settings(&s)?;
            println!(
                "{key} is now {}.",
                if matches!(state, OnOff::On) {
                    "ON"
                } else {
                    "off"
                }
            );
        }
    }
    Ok(())
}

fn short(items: &[(usize, manual::ManualTask)]) -> String {
    let mut out = String::new();
    for (_, t) in items.iter().take(20) {
        let name = t
            .project_name
            .clone()
            .unwrap_or_else(|| format!("CurseForge project {}", t.project_id));
        let want = match (&t.expected_file_name, t.exactness) {
            (Some(f), manual::Exactness::Hash) => format!("{f} (exact SHA-1 check)"),
            (Some(f), _) => format!("{f} (exact file-name check)"),
            (None, _) => "file name unknown (can't be verified without a CurseForge key)".into(),
        };
        out.push_str(&format!(
            "  - {name}: {want}\n      {}\n",
            t.file_url.clone().unwrap_or_else(|| t.project_url.clone())
        ));
    }
    if items.len() > 20 {
        out.push_str(&format!(
            "  ... and {} more (see the report)\n",
            items.len() - 20
        ));
    }
    out
}

fn waiting(resolved: &[resolve::ResolvedMod]) -> Vec<(usize, manual::ManualTask)> {
    resolved
        .iter()
        .enumerate()
        .filter(|(_, r)| manual::needs_manual(r))
        .filter_map(|(i, r)| manual::task(&r.entry).map(|t| (i, t)))
        .collect()
}

fn print_results(results: &[manual::ManualResult]) {
    for r in results {
        let mark = if r.applied { "ok " } else { "-- " };
        let name = r.mod_name.clone().unwrap_or_default();
        println!(
            "  {mark}{} {}{}",
            r.file_name,
            if name.is_empty() {
                String::new()
            } else {
                format!("({name}) ")
            },
            r.message
        );
        if r.applied && r.on_modrinth {
            println!("       found on Modrinth: the pack will download it from there");
        }
    }
}

/// Mods CurseForge lists but we couldn't fetch: apply folders given with --mods-dir, then (on a
/// terminal) walk the person through downloading and verifying the rest.
async fn manual_stage(
    cli: &Cli,
    a: &ConvertArgs,
    mr: &Modrinth,
    resolved: &mut [resolve::ResolvedMod],
) -> Result<()> {
    let opts = ManualOptions {
        accept_unverified: a.accept_unverified,
        force: a.force_manual,
    };
    if !a.mods_dir.is_empty() {
        let files: Vec<_> = manual::collect_jars(&a.mods_dir)
            .into_iter()
            .map(|(p, d)| (p, None, d))
            .collect();
        let res = manual::apply_files(resolved, &files, mr, opts).await;
        if !cli.json {
            print_results(&res);
        }
    }
    for spec in &a.manual_file {
        let (pid, path) = spec
            .split_once('=')
            .with_context(|| format!("--manual-file expects PROJECT_ID=PATH, got '{spec}'"))?;
        let pid: u64 = pid
            .trim()
            .parse()
            .with_context(|| format!("'{pid}' is not a CurseForge project id"))?;
        let Some(i) = resolved.iter().position(|r| {
            manual::needs_manual(r) && r.entry.cf.is_some_and(|c| c.project_id == pid)
        }) else {
            eprintln!("note: project {pid} doesn't need a manual file");
            continue;
        };
        let one = [(manual::expand_path(path), Some(i), false)];
        let res = manual::apply_files(resolved, &one, mr, opts).await;
        if !cli.json {
            print_results(&res);
        }
    }
    if cli.json || !std::io::stdin().is_terminal() {
        return Ok(());
    }
    loop {
        let w = waiting(resolved);
        if w.is_empty() {
            return Ok(());
        }
        println!(
            "\n{} mod(s) couldn't be found automatically. Download these exact files from CurseForge:\n\n{}",
            w.len(),
            short(&w)
        );
        let dl = manual::default_downloads_dir();
        eprint!(
            "Enter the folder with the downloaded files{}, or press Enter to continue without them: ",
            dl.as_ref().map(|d| format!(" (e.g. {})", d.display())).unwrap_or_default()
        );
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if line.trim().is_empty() {
            return Ok(());
        }
        let dir = manual::expand_path(&line);
        let files: Vec<_> = manual::collect_jars(&[dir])
            .into_iter()
            .map(|(p, d)| (p, None, d))
            .collect();
        let res = manual::apply_files(resolved, &files, mr, opts).await;
        print_results(&res);
        // Mods with nothing to match a file by: ask for each file individually.
        for (i, t) in waiting(resolved)
            .into_iter()
            .filter(|(_, t)| t.exactness == manual::Exactness::Unverifiable)
        {
            eprint!(
                "File for {} (path, or Enter to skip): ",
                t.project_name
                    .unwrap_or_else(|| format!("CurseForge project {}", t.project_id))
            );
            let mut p = String::new();
            std::io::stdin().read_line(&mut p)?;
            if p.trim().is_empty() {
                continue;
            }
            let one = [(manual::expand_path(&p), Some(i), false)];
            let res = manual::apply_files(resolved, &one, mr, opts).await;
            print_results(&res);
            for r in res.iter().filter(|r| {
                !r.applied && matches!(r.verdict, Some(manual::Verdict::Unverified { .. }))
            }) {
                eprint!(
                    "Use {}? Its exact version can't be verified. [y/N] ",
                    r.file_name
                );
                let mut yn = String::new();
                std::io::stdin().read_line(&mut yn)?;
                if yn.trim().eq_ignore_ascii_case("y") {
                    let one = [(PathBuf::from(&r.path), Some(i), false)];
                    print_results(
                        &manual::apply_files(
                            resolved,
                            &one,
                            mr,
                            ManualOptions {
                                accept_unverified: true,
                                ..opts
                            },
                        )
                        .await,
                    );
                }
            }
        }
        // Offer to accept files that matched a mod but couldn't be verified.
        let unverified: Vec<_> = res
            .iter()
            .filter(|r| !r.applied && matches!(r.verdict, Some(manual::Verdict::Unverified { .. })))
            .collect();
        for r in unverified {
            eprint!(
                "Use {} for {}? Its exact version can't be verified. [y/N] ",
                r.file_name,
                r.mod_name.clone().unwrap_or_default()
            );
            let mut yn = String::new();
            std::io::stdin().read_line(&mut yn)?;
            if yn.trim().eq_ignore_ascii_case("y") {
                let one = [(PathBuf::from(&r.path), r.index, false)];
                let res = manual::apply_files(
                    resolved,
                    &one,
                    mr,
                    ManualOptions {
                        accept_unverified: true,
                        ..opts
                    },
                )
                .await;
                print_results(&res);
            }
        }
    }
}

/// The Core API client, with a test hook for pointing it at a stand-in server.
fn cf_client(key: String) -> CurseForge {
    match std::env::var("MRIMPORT_CURSEFORGE_BASE") {
        Ok(base) if !base.is_empty() => CurseForge::with_base(base, key),
        _ => CurseForge::new(key),
    }
}

fn cf_key_or_explain(what: &str) -> Result<String> {
    config::get_secret(Secret::CurseForgeKey).with_context(|| {
        format!(
            "{what} needs a CurseForge Core API key (https://console.curseforge.com): run \
             `mrimport auth curseforge`. Without one, download the modpack .zip from CurseForge \
             in your browser and pass the file instead."
        )
    })
}

/// A CurseForge link or `cf:<id>` becomes a downloaded `.zip` (removed when the guard drops);
/// anything else is returned unchanged.
async fn materialize(
    cli: &Cli,
    input: &std::path::Path,
    rep: &Reporter,
) -> Result<(PathBuf, Option<TempDownloads>)> {
    let text = input.to_string_lossy().to_string();
    if input.exists() || !cfmodpack::looks_like_ref(&text) {
        return Ok((input.to_path_buf(), None));
    }
    let r = cfmodpack::parse_ref(&text)?;
    let cf = cf_client(cf_key_or_explain("Importing from a CurseForge link")?);
    let sel = cfmodpack::resolve(&cf, &r).await?;
    if !cli.json {
        println!(
            "CurseForge: {} - {} ({})",
            sel.project.name,
            sel.file.display_name,
            sel.file.game_versions.join(", ")
        );
    }
    let dir =
        TempDownloads(std::env::temp_dir().join(format!("mrimport-cf-{}", std::process::id())));
    let zip = cfmodpack::download(&cf, &sel, &dir.0, rep).await?;
    Ok((zip, Some(dir)))
}

async fn search(cli: &Cli, query: &str, limit: u32) -> Result<()> {
    let cf = cf_client(cf_key_or_explain("Searching CurseForge")?);
    let hits = cf.search_modpacks(query, limit).await?;
    if cli.json {
        let v: Vec<_> = hits
            .iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id, "name": h.name, "slug": h.slug, "summary": h.summary,
                    "downloads": h.download_count as u64, "url": cfmodpack::page_url(h),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if hits.is_empty() {
        println!("No modpacks found for '{query}'.");
        return Ok(());
    }
    for (i, h) in hits.iter().enumerate() {
        println!(
            "{:>2}. {}  ({} downloads)",
            i + 1,
            h.name,
            human(h.download_count as u64)
        );
        if !h.summary.is_empty() {
            println!("    {}", h.summary.chars().take(110).collect::<String>());
        }
        println!("    mrimport convert {}", cfmodpack::page_url(h));
    }
    Ok(())
}

fn human(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", n as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.1}B", n as f64 / 1e9),
    }
}
