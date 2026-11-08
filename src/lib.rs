//! Preservation contracts and bounded public-source resolution.

pub mod budget;
mod buffers;
pub mod cli;
pub mod config;
pub mod error;
mod http;
pub mod hub;
pub mod integrity;
pub mod inventory;
pub mod kit;
pub mod licenses;
pub mod plan;
mod present;
mod readiness;
mod runtime;
pub mod transfer;
pub mod validation;
pub mod vault;
pub(crate) mod verify;
mod writer_lock;

pub use config::Config;
pub use error::{Error, Result};
pub use inventory::Inventory;
pub use plan::{Plan, build_plan, build_plan_selecting};

#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
