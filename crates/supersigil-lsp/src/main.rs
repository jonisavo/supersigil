//! Supersigil Language Server binary entry point.

use serde::Serialize;
use supersigil_lsp::state::SupersigilLsp;

// Bump this only when editor-visible protocol behavior changes in a way that
// breaks compatibility. Package-version bumps alone do not require changing it.
const COMPATIBILITY_VERSION: u32 = 1;

#[derive(Serialize)]
struct CompatibilityInfo<'a> {
    compatibility_version: u32,
    server_version: &'a str,
}

fn compatibility_info_json() -> String {
    serde_json::to_string(&CompatibilityInfo {
        compatibility_version: COMPATIBILITY_VERSION,
        server_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("compatibility info should serialize")
}

fn compatibility_info_requested() -> bool {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--compatibility-info")
    {
        return true;
    }

    false
}

#[tokio::main(flavor = "current_thread")]
async fn run_lsp() {
    let (server, _) = async_lsp::MainLoop::new_server(|client| {
        tower::ServiceBuilder::new()
            .layer(async_lsp::tracing::TracingLayer::default())
            .layer(async_lsp::server::LifecycleLayer::default())
            .layer(async_lsp::panic::CatchUnwindLayer::default())
            .layer(async_lsp::concurrency::ConcurrencyLayer::default())
            .layer(async_lsp::client_monitor::ClientProcessMonitorLayer::new(
                client.clone(),
            ))
            .service(SupersigilLsp::new_router(client))
    });

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    let (stdin, stdout) = (
        async_lsp::stdio::PipeStdin::lock_tokio().unwrap(),
        async_lsp::stdio::PipeStdout::lock_tokio().unwrap(),
    );

    server.run_buffered(stdin, stdout).await.unwrap();
}

fn main() {
    if compatibility_info_requested() {
        println!("{}", compatibility_info_json());
        return;
    }

    run_lsp();
}
