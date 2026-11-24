pub mod ai_sql;
pub mod cloud;
pub mod collaboration;
pub mod commands;
pub mod connections;
pub mod data_export;
pub mod data_import;
pub mod database;
pub mod development;
pub mod dialect;
pub mod downloader;
pub mod extractor;
pub mod files;
pub mod game_dungeons;
pub mod hosts;
pub mod input_share;
pub mod json_history;
pub mod markdown;
pub mod mongo;
pub mod mq;
pub mod network;
pub mod queries;
pub mod redis_data;
pub mod remote_desktop;
pub mod servers;
pub mod terminal;
pub mod tools;
pub mod trae;

#[cfg(test)]
#[path = "../tests/support/ssh_jump.rs"]
mod ssh_fixture;

pub use database::{Databases, QueryResult};

/// Select one TLS provider explicitly when protocol crates enable multiple providers.
pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub mod backup;
pub(crate) static ACCESS_ADAPTER: &[u8] = include_bytes!(env!("TOMATO_ACCESS_EXPORTER"));
