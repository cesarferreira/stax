//! stax library interface
//!
//! This module exposes internal functionality for integration testing.
//! The main binary is in main.rs.

#![allow(dead_code)]
#![allow(unused_imports)]

// Internal modules used by the CLI and tests
mod cache;
mod ci;
pub mod cli;
mod commands;
mod config;
mod engine;
mod forge;
mod git;
mod ops;
mod progress;
mod remote;
mod tui;
mod update;

pub mod github;
pub use forge::ForgeClient;
