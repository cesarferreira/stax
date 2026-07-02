//! stax library interface
//!
//! This module exposes internal functionality for integration testing.
//! The main binary is in main.rs.

// Internal modules used by the CLI and tests
mod cache;
mod ci;
pub mod cli;
mod commands;
mod config;
mod engine;
pub mod errors;
mod forge;
mod git;
mod notifications;
mod ops;
mod progress;
mod remote;
mod tui;
mod update;

pub mod github;
pub use forge::ForgeClient;
