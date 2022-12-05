mod controllers;
mod configuration;

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use axum::Router;
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use crate::configuration::Configuration;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Loading configuration");
    let configuration = toml::from_str::<Configuration>(&*tokio::fs::read_to_string("configuration.toml").await?)?;
    let configuration = Arc::new(configuration);

    tracing::info!("Creating registry directories");
    tokio::fs::create_dir_all(&configuration.registry_storage).await?;

    let app = Router::new()
        .route("/", get(controllers::base::root))
        .route("/v2/", get(controllers::base::registry_base))
        .with_state(configuration)
        /*
        Routes remaining
        Get an image
        GET /v2/<name>/manifests/<reference>
        GET /v2/<name>/blobs/<digest>

        Push an image
        POST        /v2/<name>/blobs/uploads/
        PUT | PATCH /v2/<name>/blobs/uploads/<uuid>
        HEAD        /v2/<name>/blobs/<digest>
        PUT         /v2/<name>/manifests/<reference>
         */
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from_str("127.0.0.1:8000").unwrap();
    println!("Listen port 8000");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

