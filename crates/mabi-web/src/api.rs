//! REST API implementation.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::state::AppState;
use crate::websocket::ws_handler;

/// Create the API router.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Health check
        .route("/health", get(health_check))

        // Status
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/start", post(start_simulator))
        .route("/api/v1/stop", post(stop_simulator))

        // Devices
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/:id", get(get_device))
        .route("/api/v1/devices/:id/points", get(list_device_points))
        .route("/api/v1/devices/:id/points/:point_id", get(get_point_value))
        .route("/api/v1/devices/:id/points/:point_id", put(set_point_value))

        // Protocols
        .route("/api/v1/protocols", get(list_protocols))

        // Metrics
        .route("/api/v1/metrics", get(get_metrics))
        .route("/api/v1/metrics/prometheus", get(prometheus_metrics))

        // WebSocket
        .route("/ws", get(ws_handler))

        // CORS
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// === Response types ===

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Json<Self> {
        Json(Self {
            success: true,
            data: Some(data),
            error: None,
        })
    }

    pub fn err(message: impl Into<String>) -> Json<Self> {
        Json(Self {
            success: false,
            data: None,
            error: Some(message.into()),
        })
    }
}

// === Request types ===

#[derive(Deserialize)]
pub struct ListParams {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub protocol: Option<String>,
}

#[derive(Deserialize)]
pub struct SetValueRequest {
    pub value: serde_json::Value,
}

// === Response data types ===

#[derive(Serialize)]
pub struct StatusResponse {
    pub state: String,
    pub device_count: usize,
    pub point_count: usize,
    pub uptime_secs: u64,
    pub tick_count: u64,
}

#[derive(Serialize)]
pub struct DeviceResponse {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub state: String,
    pub point_count: usize,
}

#[derive(Serialize)]
pub struct PointResponse {
    pub id: String,
    pub device_id: String,
    pub point_id: String,
    pub value: serde_json::Value,
    pub quality: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct MetricsResponse {
    pub uptime_secs: u64,
    pub devices_active: u64,
    pub points_total: u64,
    pub memory_bytes: u64,
    pub cpu_percent: f64,
    pub ticks_total: u64,
    pub tick_rate: f64,
}

// === Handlers ===

async fn get_status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<StatusResponse>> {
    let engine = state.engine.read();
    ApiResponse::ok(StatusResponse {
        state: format!("{:?}", engine.state()),
        device_count: engine.device_count(),
        point_count: engine.point_count(),
        uptime_secs: state.uptime().as_secs(),
        tick_count: engine.tick_count(),
    })
}

async fn start_simulator(State(state): State<Arc<AppState>>) -> Json<ApiResponse<String>> {
    let _engine = state.engine.read();
    // Note: In real implementation, you'd need to handle the async start properly
    ApiResponse::ok("Simulator starting".to_string())
}

async fn stop_simulator(State(state): State<Arc<AppState>>) -> Json<ApiResponse<String>> {
    let engine = state.engine.read();
    engine.request_shutdown();
    ApiResponse::ok("Simulator stopping".to_string())
}

async fn list_devices(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Json<ApiResponse<Vec<DeviceResponse>>> {
    let engine = state.engine.read();
    let devices: Vec<DeviceResponse> = engine
        .list_devices()
        .into_iter()
        .skip(params.offset.unwrap_or(0))
        .take(params.limit.unwrap_or(100))
        .map(|d| DeviceResponse {
            id: d.id.clone(),
            name: d.name.clone(),
            protocol: format!("{:?}", d.protocol),
            state: format!("{:?}", d.state),
            point_count: d.point_count,
        })
        .collect();

    ApiResponse::ok(devices)
}

async fn get_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<DeviceResponse>>, StatusCode> {
    let engine = state.engine.read();

    match engine.device(&id) {
        Some(handle) => {
            let info = handle.info();
            Ok(ApiResponse::ok(DeviceResponse {
                id: info.id,
                name: info.name,
                protocol: format!("{:?}", info.protocol),
                state: format!("{:?}", info.state),
                point_count: info.point_count,
            }))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn list_device_points(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, StatusCode> {
    let engine = state.engine.read();

    if engine.device(&id).is_some() {
        // In real implementation, would get actual point definitions
        Ok(ApiResponse::ok(vec![]))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_point_value(
    State(state): State<Arc<AppState>>,
    Path((device_id, point_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // Get device handle and drop the engine guard before awaiting
    let device = {
        let engine = state.engine.read();
        engine.device(&device_id)
    };

    let Some(device) = device else {
        return (
            StatusCode::NOT_FOUND,
            ApiResponse::<PointResponse>::err("Device not found"),
        );
    };

    match device.read(&point_id).await {
        Ok(dp) => (
            StatusCode::OK,
            ApiResponse::ok(PointResponse {
                id: dp.id.to_string(),
                device_id,
                point_id,
                value: serde_json::to_value(&dp.value).unwrap_or(serde_json::Value::Null),
                quality: dp.quality.to_string(),
                timestamp: dp.timestamp.to_rfc3339(),
            }),
        ),
        Err(e) => (
            StatusCode::NOT_FOUND,
            ApiResponse::<PointResponse>::err(format!("{}", e)),
        ),
    }
}

async fn set_point_value(
    State(state): State<Arc<AppState>>,
    Path((device_id, point_id)): Path<(String, String)>,
    Json(request): Json<SetValueRequest>,
) -> impl IntoResponse {
    // Convert JSON value to mabi_core::Value
    let value = match request.value {
        serde_json::Value::Bool(b) => mabi_core::Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                mabi_core::Value::I64(i)
            } else if let Some(f) = n.as_f64() {
                mabi_core::Value::F64(f)
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    ApiResponse::<()>::err("Invalid number"),
                );
            }
        }
        serde_json::Value::String(s) => mabi_core::Value::String(s),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                ApiResponse::<()>::err("Invalid value type"),
            )
        }
    };

    // Get device handle and drop the engine guard before awaiting
    let device = {
        let engine = state.engine.read();
        engine.device(&device_id)
    };

    let Some(device) = device else {
        return (
            StatusCode::NOT_FOUND,
            ApiResponse::<()>::err("Device not found"),
        );
    };

    match device.write(&point_id, value).await {
        Ok(_) => (StatusCode::OK, ApiResponse::ok(())),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<()>::err(format!("{}", e)),
        ),
    }
}

async fn get_metrics(State(state): State<Arc<AppState>>) -> Json<ApiResponse<MetricsResponse>> {
    let engine = state.engine.read();
    let snapshot = engine.metrics().snapshot();

    ApiResponse::ok(MetricsResponse {
        uptime_secs: snapshot.uptime_secs,
        devices_active: snapshot.devices_active,
        points_total: snapshot.points_total,
        memory_bytes: snapshot.memory_bytes,
        cpu_percent: snapshot.cpu_percent,
        ticks_total: snapshot.ticks_total,
        tick_rate: snapshot.tick_rate,
    })
}

async fn prometheus_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let engine = state.engine.read();
    let metrics = engine.metrics().export_prometheus();
    (StatusCode::OK, [("content-type", "text/plain")], metrics)
}

// === Health check ===

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// === Protocols ===

#[derive(Serialize)]
pub struct ProtocolResponse {
    pub id: String,
    pub name: String,
    pub default_port: u16,
    pub transport: String,
}

async fn list_protocols() -> Json<ApiResponse<Vec<ProtocolResponse>>> {
    use strum::IntoEnumIterator;
    use mabi_core::Protocol;

    let protocols: Vec<ProtocolResponse> = Protocol::iter()
        .map(|p| ProtocolResponse {
            id: format!("{:?}", p).to_lowercase(),
            name: p.display_name().to_string(),
            default_port: p.default_port(),
            transport: if p.is_tcp() {
                "TCP".to_string()
            } else if p.is_udp() {
                "UDP".to_string()
            } else {
                "Serial".to_string()
            },
        })
        .collect();

    ApiResponse::ok(protocols)
}
