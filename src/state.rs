use crate::{config::Config, protocol::RelayResponseStart};
use axum::{body::Bytes, extract::ws::Message};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

#[derive(Clone)]
pub struct AppState {
    pub devices: Arc<RwLock<HashMap<String, Arc<DeviceConnection>>>>,
    pub shared_token: Arc<String>,
    request_counter: Arc<AtomicU64>,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
}

pub struct DeviceConnection {
    pub sender: mpsc::Sender<Message>,
    pub pending: Mutex<HashMap<String, PendingResponse>>,
}

pub struct PendingResponse {
    pub start_sender: Option<oneshot::Sender<RelayResponseStart>>,
    pub body_sender: mpsc::Sender<Result<Bytes, io::Error>>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            shared_token: Arc::new(config.shared_token),
            request_counter: Arc::new(AtomicU64::new(1)),
            request_timeout: config.request_timeout,
            max_body_bytes: config.max_body_bytes,
        }
    }

    pub fn next_request_id(&self) -> String {
        self.request_counter
            .fetch_add(1, Ordering::Relaxed)
            .to_string()
    }
}
