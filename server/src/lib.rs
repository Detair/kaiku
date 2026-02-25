//! `VoiceChat` Server
//!
//! Self-hosted voice and text chat platform for gaming communities.
//! Optimized for low latency (<50ms), high quality, and maximum security.

pub mod admin;
pub mod api;
pub mod auth;
pub mod chat;
pub mod config;
pub mod connectivity;
pub mod crypto;
pub mod db;
pub mod discovery;
pub mod email;
pub mod governance;
pub mod guild;
pub mod moderation;
pub mod openapi;
pub mod pages;
pub mod permissions;
pub mod presence;
pub mod ratelimit;
pub mod social;
pub mod util;
pub mod voice;
pub mod webhooks;
pub mod ws;

#[cfg(test)]
mod redis_tests;
