mod config;
mod device;
mod health;
mod protocol;
mod proxy;
mod routes;
mod state;

use crate::{config::Config, routes::build_router, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let bind = config.bind;
    let state = AppState::new(config);
    let app = build_router(state);

    println!("pCloud relay server listening on http://{bind}");
    println!("Device tunnel endpoint: ws://{bind}/api/relay/device/connect");
    println!("Remote web/API path: http://{bind}/d/{{device_id}}/...");

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
