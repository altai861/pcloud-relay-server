use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayToDevice {
    RequestStart(RelayRequestStart),
    RequestBodyChunk(BodyChunk),
    RequestEnd(RequestEnd),
    RequestAbort(RequestAbort),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceToRelay {
    ResponseStart(RelayResponseStart),
    ResponseBodyChunk(BodyChunk),
    ResponseEnd(ResponseEnd),
    ResponseError(ResponseError),
    Pong,
}

#[derive(Debug, Serialize)]
pub struct RelayRequestStart {
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub headers: Vec<HeaderPair>,
}

#[derive(Debug, Deserialize)]
pub struct RelayResponseStart {
    pub request_id: String,
    pub status: u16,
    pub headers: Vec<HeaderPair>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BodyChunk {
    pub request_id: String,
    pub body_base64: String,
}

#[derive(Debug, Serialize)]
pub struct RequestEnd {
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct RequestAbort {
    pub request_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ResponseEnd {
    pub request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ResponseError {
    pub request_id: String,
    pub status: u16,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}
