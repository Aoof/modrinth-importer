//! `mrimport` — import mods and modpacks from other providers to Modrinth.

mod update;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use mrimport_core::{
    config::{self, Secret},
    curseforge::CurseForge,
    events::{Event, Reporter},
    export::mrpack::{open_with_default_app, write_mrpack, ExportOptions},
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
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show what a pack, instance or mods folder contains.
    Inspect {
        path: PathBuf,
        #[command(flatten)]
        load: LoadArgs,
    },
    /// Convert a pack to a Modrinth `.mrpack`.
    Convert(ConvertArgs),
    /// Publish a project and its versions to Modrinth.
    Publish(PublishArgs),
    /// Store or remove API credentials in the OS keychain.
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
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
    /// `.zip` pack, launcher instance folder, packwiz folder or `mods` folder.
    path: PathBuf,
    /// Output file (default: `<name>.mrpack` in the current directory).
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    load: LoadArgs,
    /// Loader version to record in the pack (looked up automatically if omitted).
    #[arg(long)]
    loader_version: Option<String>,
    /// Don't bundle mods that Modrinth doesn't host.
    #[arg(long)]
    no_bundle: bool,
    /// Download and bundle unmatched mods that only have a download URL.
    #[arg(long)]
    download_remote: bool,
    /// Accept the best candidate when a mod has several possible matches.
    #[arg(long)]
    accept_ambiguous: bool,
    /// Write a Markdown report next to the output.
    #[arg(long)]
    report: bool,
    /// Open the result in the Modrinth App when done.
    #[arg(long)]
    open: bool,
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

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Cmd::Inspect { path, load } => inspect(&cli, path, load).await,
        Cmd::Convert(a) => convert(&cli, a).await,
        Cmd::Publish(a) => publish_cmd(&cli, a).await,
        Cmd::Auth { action } => auth(action),
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

fn load_ctx(rep: Reporter) -> LoadCtx {
    LoadCtx {
        curseforge: config::get_secret(Secret::CurseForgeKey).map(CurseForge::new),
        reporter: rep,
    }
}

async fn inspect(cli: &Cli, path: &std::path::Path, load: &LoadArgs) -> Result<()> {
    let pack = providers::load(path, &load_opts(load), &load_ctx(Reporter::silent())).await?;
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
    let mut pack = providers::load(&a.path, &load_opts(&a.load), &load_ctx(rep.clone())).await?;
    let mr = modrinth(&cli.api_base, false)?;
    let mut resolved = resolve::resolve(&pack, &mr, &rep).await?;
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
        bundle_unmatched: !a.no_bundle,
        download_remote: a.download_remote,
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
