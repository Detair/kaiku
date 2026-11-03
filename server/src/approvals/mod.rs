//! Device signatures and approvals (spec B.2, contract C26).
//!
//! **SECURITY CRITICAL.** See `AGENTS.md` in this directory.

pub mod context;
pub mod secrets;
pub mod signing;

#[cfg(test)]
mod golden_tests;
