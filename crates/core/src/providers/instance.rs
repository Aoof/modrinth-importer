//! Launcher instance providers (Prism/MultiMC, ATLauncher, FTB App, Technic,
//! CurseForge app) plus the bare mods-folder fallback. They all end up
//! calling [`from_game_dir`].

use super::{jarmeta, tree::Tree, LoadOptions};
use crate::hash::{sha1_hex, sha512_hex};
use crate::model::*;
use crate::{Error, Result};

/// Non-mod folders worth carrying over.
const OVERRIDE_DIRS: &[&str] = &[
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "resourcepacks",
    "shaderpacks",
    "datapacks",
];

pub struct Meta {
    pub source: SourceKind,
    pub name: Option<String>,
    pub version: Option<String>,
    pub mc_version: Option<String>,
    pub loader: Option<Loader>,
    pub loader_version: Option<String>,
}

pub fn from_game_dir(game: &Tree, meta: Meta, opts: &LoadOptions) -> Result<ImportedPack> {
    let mut mods = vec![];
    let mut inferred: Option<Loader> = None;
    for (rel, origin) in game.under("mods") {
        let lower = rel.to_ascii_lowercase();
        if !lower.ends_with(".jar") {
            continue; // skips *.jar.disabled and non-jars
        }
        let bytes = origin.read()?;
        let jm = jarmeta::inspect(&bytes);
        if inferred.is_none() {
            inferred = jm.loader;
        }
        let file_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        let mut e = ModEntry::new(file_name);
        e.sha1 = Some(sha1_hex(&bytes));
        e.sha512 = Some(sha512_hex(&bytes));
        e.size = Some(bytes.len() as u64);
        e.name_hint = jm.name;
        e.slug_hint = jm.mod_id;
        e.origin = Some(origin);
        mods.push(e);
    }

    let mut overrides = vec![];
    for d in OVERRIDE_DIRS {
        for (rel, origin) in game.under(d) {
            overrides.push(OverrideFile {
                path: format!("{d}/{rel}"),
                origin,
            });
        }
    }
    for f in ["options.txt", "servers.dat"] {
        if let Some(o) = game.get(f) {
            overrides.push(OverrideFile {
                path: f.to_string(),
                origin: o.clone(),
            });
        }
    }

    let loader = opts
        .loader
        .or(meta.loader)
        .or(inferred)
        .unwrap_or(Loader::Vanilla);
    let mc_version = opts.mc_version.clone().or(meta.mc_version).ok_or_else(|| {
        Error::Invalid(
            "could not determine the Minecraft version; pass it explicitly (--mc-version)".into(),
        )
    })?;
    Ok(ImportedPack {
        source: meta.source,
        name: opts
            .name
            .clone()
            .or(meta.name)
            .unwrap_or_else(|| "Imported pack".into()),
        version: meta.version.unwrap_or_else(|| "1.0.0".into()),
        summary: None,
        mc_version,
        loader,
        loader_version: meta.loader_version,
        mods,
        overrides,
    })
}

fn game_root(t: &Tree) -> Tree {
    if t.files.iter().any(|(p, _)| p.starts_with(".minecraft/")) {
        t.stripped(".minecraft")
    } else if t.files.iter().any(|(p, _)| p.starts_with("minecraft/")) {
        t.stripped("minecraft")
    } else {
        t.clone()
    }
}

pub fn prism(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    #[derive(serde::Deserialize)]
    struct Pack {
        components: Vec<Comp>,
    }
    #[derive(serde::Deserialize)]
    struct Comp {
        uid: String,
        version: Option<String>,
    }
    let pack: Pack = t.read_json("mmc-pack.json")?;
    let (mut mc, mut loader, mut lv) = (None, None, None);
    for c in pack.components {
        match c.uid.as_str() {
            "net.minecraft" => mc = c.version,
            "net.fabricmc.fabric-loader" => (loader, lv) = (Some(Loader::Fabric), c.version),
            "org.quiltmc.quilt-loader" => (loader, lv) = (Some(Loader::Quilt), c.version),
            "net.minecraftforge" => (loader, lv) = (Some(Loader::Forge), c.version),
            "net.neoforged" => (loader, lv) = (Some(Loader::NeoForge), c.version),
            _ => {}
        }
    }
    let name = t.read_string("instance.cfg").ok().and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("name=").map(|v| v.trim().to_string()))
    });
    from_game_dir(
        &game_root(t),
        Meta {
            source: SourceKind::Prism,
            name,
            version: None,
            mc_version: mc,
            loader,
            loader_version: lv,
        },
        opts,
    )
}

pub fn atlauncher(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    let v: serde_json::Value = t.read_json("instance.json")?;
    let l = &v["launcher"];
    let loader = l["loaderVersion"]["type"].as_str().and_then(Loader::parse);
    from_game_dir(
        t,
        Meta {
            source: SourceKind::ATLauncher,
            name: l["name"].as_str().map(String::from),
            version: l["version"].as_str().map(String::from),
            mc_version: v["id"].as_str().map(String::from),
            loader,
            loader_version: l["loaderVersion"]["version"].as_str().map(String::from),
        },
        opts,
    )
}

pub fn ftb(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    // Best effort: FTB App's instance.json layout is not formally documented.
    let v: serde_json::Value = t.read_json("instance.json")?;
    let loader_str = v["modLoader"].as_str().unwrap_or("");
    let loader = ["neoforge", "forge", "fabric", "quilt"]
        .iter()
        .find(|k| loader_str.to_ascii_lowercase().contains(*k))
        .and_then(|k| Loader::parse(k));
    from_game_dir(
        t,
        Meta {
            source: SourceKind::Ftb,
            name: v["name"].as_str().map(String::from),
            version: v["version"].as_str().map(String::from),
            mc_version: v["mcVersion"].as_str().map(String::from),
            loader,
            loader_version: None,
        },
        opts,
    )
}

pub fn technic(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    let id = t
        .read_json::<serde_json::Value>("bin/version.json")
        .ok()
        .and_then(|v| v["id"].as_str().map(String::from))
        .unwrap_or_default();
    let mc = regex::Regex::new(r"^\d+\.\d+(\.\d+)?")
        .unwrap()
        .find(&id)
        .map(|m| m.as_str().to_string());
    let lower = id.to_ascii_lowercase();
    let loader = ["neoforge", "forge", "fabric", "quilt"]
        .iter()
        .find(|k| lower.contains(*k))
        .and_then(|k| Loader::parse(k));
    from_game_dir(
        t,
        Meta {
            source: SourceKind::Technic,
            name: None,
            version: None,
            mc_version: mc,
            loader,
            loader_version: None,
        },
        opts,
    )
}

pub fn curseforge_instance(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    let v: serde_json::Value = t.read_json("minecraftinstance.json")?;
    let base = &v["baseModLoader"];
    let loader_name = base["name"].as_str().unwrap_or("");
    let loader = ["neoforge", "forge", "fabric", "quilt"]
        .iter()
        .find(|k| loader_name.to_ascii_lowercase().starts_with(*k))
        .and_then(|k| Loader::parse(k));
    from_game_dir(
        t,
        Meta {
            source: SourceKind::CurseForge,
            name: v["name"].as_str().map(String::from),
            version: None,
            mc_version: base["minecraftVersion"]
                .as_str()
                .or(v["gameVersion"].as_str())
                .map(String::from),
            loader,
            loader_version: None,
        },
        opts,
    )
}

pub fn folder(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    from_game_dir(
        t,
        Meta {
            source: SourceKind::Folder,
            name: None,
            version: None,
            mc_version: None,
            loader: None,
            loader_version: None,
        },
        opts,
    )
}
