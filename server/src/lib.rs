//! `VoiceChat` Server
//!
//! Self-hosted voice and text chat platform for gaming communities.
//! Optimized for low latency (<50ms), high quality, and maximum security.

pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod db;
pub mod guild;
pub mod permissions;
pub mod social;
pub mod voice;
pub mod ws;

#[cfg(test)]
mod redis_tests;
