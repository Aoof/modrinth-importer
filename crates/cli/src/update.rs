//! `mrimport self-update`: the same steps as the installers. Every download is verified against
//! the release's `SHA256SUMS` first; only then are the old files removed and the new ones
//! installed. Settings and credentials are never touched.

use anyhow::{bail, Context, Result};
use mrimport_core::{events::Reporter, update};

pub struct Options {
    pub check_only: bool,
    pub cli_only: bool,
    pub gui_only: bool,
}

pub async fn run(o: Options) -> Result<()> {
    let target = env!("MRIMPORT_TARGET");
    let exe = std::env::current_exe()?;
    update::cleanup_leftovers(&exe);

    let http = update::client()?;
    let rel = update::latest(&http)
        .await
        .context("could not fetch the latest release")?;
    let current = env!("CARGO_PKG_VERSION");
    if !o.gui_only && !update::is_newer(&rel.version, current) {
        println!("Already up to date ({current}).");
        return Ok(());
    }
    println!("New version available: {} (current {current})", rel.version);
    if o.check_only {
        return Ok(());
    }

    let gui = if o.cli_only {
        None
    } else {
        update::installed_gui()
    };
    if o.gui_only && gui.is_none() {
        bail!("the desktop app isn't installed");
    }

    // 1. Download and verify everything before changing anything.
    let tmp = std::env::temp_dir().join(format!("mrimport-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let rep = progress();
    let result = async {
        let cli = if o.gui_only {
            None
        } else {
            Some(
                update::download_verified(&http, &rel, &update::cli_asset(target), &tmp, &rep)
                    .await?,
            )
        };
        let setup = match (&gui, update::gui_asset()) {
            (Some(_), Some(asset)) => {
                Some(update::download_verified(&http, &rel, &asset, &tmp, &rep).await?)
            }
            (Some(_), None) => bail!("no desktop app build exists for this platform"),
            _ => None,
        };
        println!("Checksums verified.");

        // 2. Remove the old program files and install the new ones.
        if let Some(cli) = cli {
            update::replace_cli(&cli, &exe)?;
            println!("Updated the CLI to {}.", rel.version);
        }
        if let (Some(old), Some(setup)) = (&gui, &setup) {
            println!("Reinstalling the desktop app...");
            update::install_gui(setup, old, false, None)?;
            println!("Updated the desktop app to {}.", rel.version);
        }
        anyhow::Ok(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    println!("Done.");
    Ok(())
}

fn progress() -> Reporter {
    Reporter::new(|e| {
        if let mrimport_core::events::Event::Stage { name } = e {
            println!("{name}...");
        }
    })
}
