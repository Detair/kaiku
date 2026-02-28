//! `Kaiku` Common Library
//!
//! Shared types, protocols, and utilities used by both server and client.

pub mod error;
pub mod protocol;
pub mod types;

pub use error::{Error, Result};
pub use types::*;
