use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Forge,
    NeoForge,
    Fabric,
    Quilt,
    Vanilla,
}

impl Loader {
    /// Identifier used by Modrinth's `loaders` fields.
    pub fn modrinth_id(self) -> &'static str {
        match self {
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
            Loader::Fabric => "fabric",
            Loader::Quilt => "quilt",
            Loader::Vanilla => "minecraft",
        }
    }
    /// Key used in `.mrpack` `dependencies`.
    pub fn mrpack_key(self) -> Option<&'static str> {
        match self {
            Loader::Forge => Some("forge"),
            Loader::NeoForge => Some("neoforge"),
            Loader::Fabric => Some("fabric-loader"),
            Loader::Quilt => Some("quilt-loader"),
            Loader::Vanilla => None,
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "forge" | "minecraftforge" => Some(Loader::Forge),
            "neoforge" | "neo-forge" => Some(Loader::NeoForge),
            "fabric" | "fabric-loader" | "net.fabricmc.fabric-loader" => Some(Loader::Fabric),
            "quilt" | "quilt-loader" | "org.quiltmc.quilt-loader" => Some(Loader::Quilt),
            "vanilla" | "minecraft" | "" => Some(Loader::Vanilla),
            _ => None,
        }
    }
}

/// Where the bytes of a local file live.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Origin {
    File { path: PathBuf },
    ZipEntry { zip: PathBuf, name: String },
}

impl Origin {
    pub fn read(&self) -> crate::Result<Vec<u8>> {
        use std::io::Read;
        match self {
            Origin::File { path } => Ok(std::fs::read(path)?),
            Origin::ZipEntry { zip, name } => {
                let mut a = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
                let mut f = a.by_name(name)?;
                let mut buf = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CfRef {
    pub project_id: u64,
    pub file_id: u64,
}

/// Where information about a CurseForge file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupSource {
    /// CurseForge Core API (needs an API key): exact names, hashes and links.
    CoreApi,
    /// cursemaven.com: the jar itself, hashed locally (keyless, third party).
    CurseMaven,
    /// api.cfwidget.com: project and file names, links (keyless, third party).
    CfWidget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModEntry {
    pub file_name: String,
    /// Human-readable name when known (used for fuzzy matching).
    pub name_hint: Option<String>,
    pub slug_hint: Option<String>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: Option<u64>,
    /// Local bytes, if the provider ships the jar itself.
    pub origin: Option<Origin>,
    pub download_url: Option<String>,
    pub cf: Option<CfRef>,
    /// The exact file name this entry should have, when known (used to verify manual downloads).
    #[serde(default)]
    pub expected_file_name: Option<String>,
    /// CurseForge project page, when known.
    #[serde(default)]
    pub cf_url: Option<String>,
    /// Which lookups contributed to this entry's data.
    #[serde(default)]
    pub lookup_sources: Vec<LookupSource>,
    pub client_required: bool,
    pub server_required: bool,
}

impl ModEntry {
    /// A name fit for showing people: never the internal `cf-<project>-<file>.jar` placeholder.
    pub fn display_name(&self) -> String {
        if let Some(n) = self.expected_file_name.as_ref().or(self.name_hint.as_ref()) {
            return n.clone();
        }
        match self.cf {
            Some(cf) => format!("CurseForge project {} (file {})", cf.project_id, cf.file_id),
            None => self.file_name.clone(),
        }
    }

    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            name_hint: None,
            slug_hint: None,
            sha1: None,
            sha512: None,
            size: None,
            origin: None,
            download_url: None,
            cf: None,
            expected_file_name: None,
            cf_url: None,
            lookup_sources: vec![],
            client_required: true,
            server_required: true,
        }
    }
}

/// A non-mod file copied verbatim (configs, scripts, resource packs...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideFile {
    /// Path relative to the game directory, `/`-separated.
    pub path: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CurseForge,
    Prism,
    ATLauncher,
    Ftb,
    Technic,
    Packwiz,
    Modrinth,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedPack {
    pub source: SourceKind,
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub mc_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub mods: Vec<ModEntry>,
    pub overrides: Vec<OverrideFile>,
}
