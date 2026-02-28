//! `Kaiku` Desktop Client - Entry Point

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vc_client::run();
}
