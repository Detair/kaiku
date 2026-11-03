//! `kaiku-admin`: the host CLI for key phones and sessions.
//!
//! Exit codes: 0 done, 2 usage error, 1 failure. See `vc_server::cli`.

use std::process::ExitCode;

use vc_server::cli::{run, CliCtx, Command, StdIo};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match Command::parse(&args) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // As the server: the TLS provider before any client is built.
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());
    let ctx = match CliCtx::from_env().await {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("kaiku-admin: cannot reach the server's services: {e:#}");
            return ExitCode::from(1);
        }
    };
    match run(&ctx, cmd, &mut StdIo).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kaiku-admin: {e:#}");
            ExitCode::from(1)
        }
    }
}
