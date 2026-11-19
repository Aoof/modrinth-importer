//! Reads loader/mod metadata embedded in mod jars.

use crate::model::Loader;
use std::io::Read;

#[derive(Debug, Default, Clone)]
pub struct JarMeta {
    pub loader: Option<Loader>,
    pub mod_id: Option<String>,
    pub name: Option<String>,
}

pub fn inspect(bytes: &[u8]) -> JarMeta {
    let mut meta = JarMeta::default();
    let Ok(mut a) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return meta;
    };
    let mut read = |name: &str| -> Option<String> {
        let mut f = a.by_name(name).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s)
    };

    if let Some(s) = read("quilt.mod.json") {
        meta.loader = Some(Loader::Quilt);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            meta.mod_id = v["quilt_loader"]["id"].as_str().map(String::from);
            meta.name = v["quilt_loader"]["metadata"]["name"]
                .as_str()
                .map(String::from);
        }
    } else if let Some(s) = read("fabric.mod.json") {
        meta.loader = Some(Loader::Fabric);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            meta.mod_id = v["id"].as_str().map(String::from);
            meta.name = v["name"].as_str().map(String::from);
        }
    } else if let Some(s) = read("META-INF/neoforge.mods.toml") {
        meta.loader = Some(Loader::NeoForge);
        fill_from_mods_toml(&s, &mut meta);
    } else if let Some(s) = read("META-INF/mods.toml") {
        // 1.20.2+ NeoForge jars may still ship mods.toml; loaderVersion hints can't tell, assume Forge.
        meta.loader = Some(Loader::Forge);
        fill_from_mods_toml(&s, &mut meta);
    }
    meta
}

fn fill_from_mods_toml(s: &str, meta: &mut JarMeta) {
    if let Ok(v) = s.parse::<toml::Table>() {
        if let Some(m) = v
            .get("mods")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
        {
            meta.mod_id = m.get("modId").and_then(|x| x.as_str()).map(String::from);
            meta.name = m
                .get("displayName")
                .and_then(|x| x.as_str())
                .map(String::from);
        }
    }
}
