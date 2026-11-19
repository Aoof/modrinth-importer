//! packwiz packs (`pack.toml`, `index.toml`, `*.pw.toml`).

use super::{tree::Tree, LoadOptions};
use crate::model::*;
use crate::{Error, Result};

pub fn load(t: &Tree, opts: &LoadOptions) -> Result<ImportedPack> {
    let pack: toml::Table = t.read_string("pack.toml")?.parse()?;
    let versions = pack.get("versions").and_then(|v| v.as_table());
    let ver = |k: &str| {
        versions
            .and_then(|v| v.get(k))
            .and_then(|x| x.as_str())
            .map(String::from)
    };
    let (loader, loader_version) = [
        ("neoforge", Loader::NeoForge),
        ("forge", Loader::Forge),
        ("fabric", Loader::Fabric),
        ("quilt", Loader::Quilt),
    ]
    .iter()
    .find_map(|(k, l)| ver(k).map(|v| (*l, Some(v))))
    .unwrap_or((Loader::Vanilla, None));
    let mc = opts
        .mc_version
        .clone()
        .or_else(|| ver("minecraft"))
        .ok_or_else(|| Error::Invalid("pack.toml has no [versions] minecraft entry".into()))?;

    let index_file = pack
        .get("index")
        .and_then(|i| i.get("file"))
        .and_then(|f| f.as_str())
        .unwrap_or("index.toml");
    let index: toml::Table = t.read_string(index_file)?.parse()?;
    let mut mods = vec![];
    let mut overrides = vec![];
    for f in index
        .get("files")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(path) = f.get("file").and_then(|p| p.as_str()) else {
            continue;
        };
        let is_meta = f.get("metafile").and_then(|m| m.as_bool()).unwrap_or(false);
        if !is_meta {
            if let Some(o) = t.get(path) {
                overrides.push(OverrideFile {
                    path: path.to_string(),
                    origin: o.clone(),
                });
            }
            continue;
        }
        let Ok(text) = t.read_string(path) else {
            continue;
        };
        let Ok(m) = text.parse::<toml::Table>() else {
            continue;
        };
        let s = |tbl: &toml::Table, k: &str| tbl.get(k).and_then(|x| x.as_str()).map(String::from);
        let mut e = ModEntry::new(s(&m, "filename").unwrap_or_else(|| path.to_string()));
        e.name_hint = s(&m, "name");
        if let Some(d) = m.get("download").and_then(|d| d.as_table()) {
            e.download_url = s(d, "url");
            let hash = s(d, "hash");
            match s(d, "hash-format").as_deref() {
                Some("sha1") => e.sha1 = hash,
                Some("sha512") => e.sha512 = hash,
                _ => {}
            }
        }
        if let Some(c) = m
            .get("update")
            .and_then(|u| u.get("curseforge"))
            .and_then(|c| c.as_table())
        {
            if let (Some(p), Some(f)) = (
                c.get("project-id").and_then(|x| x.as_integer()),
                c.get("file-id").and_then(|x| x.as_integer()),
            ) {
                e.cf = Some(CfRef {
                    project_id: p as u64,
                    file_id: f as u64,
                });
            }
        }
        match m.get("side").and_then(|x| x.as_str()) {
            Some("client") => e.server_required = false,
            Some("server") => e.client_required = false,
            _ => {}
        }
        mods.push(e);
    }

    Ok(ImportedPack {
        source: SourceKind::Packwiz,
        name: opts
            .name
            .clone()
            .or_else(|| pack.get("name").and_then(|n| n.as_str()).map(String::from))
            .unwrap_or_else(|| "packwiz pack".into()),
        version: pack
            .get("version")
            .and_then(|n| n.as_str())
            .unwrap_or("1.0.0")
            .to_string(),
        summary: pack
            .get("description")
            .and_then(|n| n.as_str())
            .map(String::from),
        mc_version: mc,
        loader: opts.loader.unwrap_or(loader),
        loader_version,
        mods,
        overrides,
    })
}
