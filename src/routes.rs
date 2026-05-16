use crate::{
    device::{device_connect, device_status},
    health::health,
    proxy::{proxy_path, proxy_root, redirect_device_root},
    state::AppState,
};
use axum::{
    Router,
    routing::{any, get},
};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/devices/:device_id/status", get(device_status))
        .route("/api/relay/device/connect", get(device_connect))
        .route("/d/:device_id", any(redirect_device_root))
        .route("/d/:device_id/", any(proxy_root))
        .route("/d/:device_id/*path", any(proxy_path))
        .with_state(state)
}
