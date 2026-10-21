//! Preservation contracts and bounded public-source resolution.

pub mod cli;
pub mod config;
pub mod error;
mod http;
pub mod hub;
pub mod integrity;
pub mod inventory;
pub mod licenses;
pub mod plan;
mod present;
mod runtime;
pub mod transfer;
pub mod validation;
pub mod vault;
pub(crate) mod verify;
mod writer_lock;

pub use config::Config;
pub use error::{Error, Result};
pub use inventory::Inventory;
pub use plan::{Plan, build_plan};

#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
