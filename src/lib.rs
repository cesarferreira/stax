//! stax library interface
//!
//! Shared functionality for the CLI, TUI, desktop application, and integration
//! tests. The main CLI binary is in `main.rs`.

// Shared and internal modules used by the stax clients
pub mod application;
mod cache;
mod ci;
pub mod cli;
mod commands;
mod config;
mod engine;
pub mod entrypoint;
pub mod errors;
mod forge;
mod git;
mod notifications;
mod ops;
mod parallel;
mod progress;
mod remote;
mod tui;
mod update;

pub mod github;
pub use forge::ForgeClient;
