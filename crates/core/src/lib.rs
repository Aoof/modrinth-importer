//! Core library for Modrinth Importer.
//!
//! The pipeline is: **detect/load** a pack with a [`providers::Provider`] →
//! **resolve** every mod against Modrinth ([`resolve`]) → **export** to a
//! `.mrpack` or a Modrinth App profile ([`export`]). Creators can also
//! **publish** projects to Modrinth ([`publish`]).

pub mod config;
pub mod curseforge;
pub mod debuglog;
pub mod diagnostics;
pub mod error;
pub mod events;
pub mod export;
pub mod hash;
pub mod model;
pub mod modrinth;
pub mod providers;
pub mod publish;
pub mod resolve;

pub use error::{Error, Result};

/// User-Agent sent to every API (Modrinth requires a unique, descriptive one).
pub fn user_agent() -> String {
    format!("Aoof/modrinth-importer/{}", env!("CARGO_PKG_VERSION"))
}
