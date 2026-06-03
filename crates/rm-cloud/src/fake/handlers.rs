//! axum route handlers for the fake cloud.

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Path, State as AxState},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::State;

type Shared = Arc<Mutex<State>>;

/// A `429 Too Many Requests` carrying `Retry-After: 0` (so the client's retry loop is
/// exercised without test sleeps). Returns `None` if no rate-limit injection is pending.
fn rate_limit_response(state: &Shared) -> Option<axum::response::Response> {
    if state.lock().unwrap().take_rate_limit() {
        let mut resp = (StatusCode::TOO_MANY_REQUESTS, "slow down").into_response();
        resp.headers_mut()
            .insert("retry-after", "0".parse().unwrap());
        Some(resp)
    } else {
        None
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/token/json/2/device/new", post(device_new))
        .route("/token/json/2/user/new", post(user_new))
        .route("/sync/v4/root", get(root_get))
        .route("/sync/v3/root", put(root_put))
        .route("/sync/v3/files/{hash}", get(blob_get).put(blob_put))
        .with_state(state)
}

#[derive(Deserialize)]
struct DeviceReq {
    code: String,
    #[allow(dead_code)]
    #[serde(rename = "deviceDesc")]
    device_desc: String,
    #[allow(dead_code)]
    #[serde(rename = "deviceID")]
    device_id: String,
}

async fn device_new(Json(req): Json<DeviceReq>) -> impl IntoResponse {
    (StatusCode::OK, format!("device-token-for-{}", req.code))
}

async fn user_new(headers: HeaderMap) -> impl IntoResponse {
    match headers.get("authorization") {
        Some(_) => (StatusCode::OK, "user-token".to_string()),
        None => (StatusCode::UNAUTHORIZED, "no device bearer".to_string()),
    }
}

#[derive(Serialize)]
struct RootResp {
    hash: String,
    generation: i64,
    #[serde(rename = "schemaVersion")]
    schema_version: i64,
}

async fn root_get(AxState(state): AxState<Shared>) -> impl IntoResponse {
    if let Some(resp) = rate_limit_response(&state) {
        return resp;
    }
    {
        let mut s = state.lock().unwrap();
        if s.unauthorized_once {
            s.unauthorized_once = false;
            return (StatusCode::UNAUTHORIZED, "forced unauthorized").into_response();
        }
    }
    let mut s = state.lock().unwrap();
    if s.generation == 0 && s.root_hash.is_empty() {
        return (StatusCode::NOT_FOUND, "no root yet").into_response();
    }
    s.root_gets += 1;
    let hash = if s.active_lag > 0 {
        s.active_lag -= 1;
        s.lagged_hash.clone()
    } else {
        s.root_hash.clone()
    };
    Json(RootResp {
        hash,
        generation: s.generation,
        schema_version: 4,
    })
    .into_response()
}

#[derive(Deserialize)]
struct RootPutReq {
    broadcast: bool,
    hash: String,
    generation: i64,
}

async fn root_put(
    AxState(state): AxState<Shared>,
    Json(req): Json<RootPutReq>,
) -> impl IntoResponse {
    if let Some(resp) = rate_limit_response(&state) {
        return resp;
    }
    let mut s = state.lock().unwrap();
    if s.conflicts_remaining > 0 {
        s.conflicts_remaining -= 1;
        return (StatusCode::PRECONDITION_FAILED, "forced conflict").into_response();
    }
    if req.generation != s.generation {
        return (StatusCode::PRECONDITION_FAILED, "wrong generation").into_response();
    }
    let req_prev_hash = s.root_hash.clone();
    s.generation = req.generation + 1;
    s.root_hash = req.hash.clone();
    if req.broadcast {
        s.broadcast_commits += 1;
    }
    if s.arm_lag > 0 {
        s.active_lag = s.arm_lag;
        s.arm_lag = 0;
        // The index visible BEFORE this commit.
        s.lagged_hash = req_prev_hash;
    }
    let gen = s.generation;
    Json(RootResp {
        hash: req.hash,
        generation: gen,
        schema_version: 4,
    })
    .into_response()
}

async fn blob_get(AxState(state): AxState<Shared>, Path(hash): Path<String>) -> impl IntoResponse {
    if let Some(resp) = rate_limit_response(&state) {
        return resp;
    }
    let mut s = state.lock().unwrap();
    match s.blobs.get(&hash).cloned() {
        Some(b) => {
            // Count each successful blob GET for cache-effectiveness assertions.
            *s.blob_gets.entry(hash).or_insert(0) += 1;
            (StatusCode::OK, b).into_response()
        }
        None => (StatusCode::NOT_FOUND, "no blob").into_response(),
    }
}

async fn blob_put(
    AxState(state): AxState<Shared>,
    Path(hash): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(resp) = rate_limit_response(&state) {
        return resp;
    }
    // Plain key->bytes store: doc-index blobs are keyed by doc hash, not content hash.
    state.lock().unwrap().blobs.insert(hash, body.to_vec());
    StatusCode::OK.into_response()
}
