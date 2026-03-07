use crate::config::AppConfig;
use crate::db::{self, AppState};
use crate::models::{CreateWorkflowRequest, TriggerWorkflowRequest, UpdateWorkflowDraftRequest};
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

pub async fn run_api(config: AppConfig) -> Result<()> {
    let state = db::connect(&config).await?;
    let app = router(state.clone(), &config);
    let listener = tokio::net::TcpListener::bind(&config.server_addr).await?;
    info!("api listening on {}", config.server_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState, config: &AppConfig) -> Router {
    let cors = if let Ok(origin) = config.dashboard_origin.parse::<axum::http::HeaderValue>() {
        CorsLayer::new()
            .allow_origin(origin)
            .allow_headers(Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT])
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_headers(Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT])
    };

    Router::new()
        .route("/health", get(health))
        .route("/v1/workflows", get(list_workflows).post(create_workflow))
        .route("/v1/workflows/:id", get(get_workflow_history))
        .route("/v1/workflows/:id/draft", put(update_workflow_draft))
        .route("/v1/workflows/:id/publish", post(publish_workflow))
        .route("/v1/workflows/:slug/run", post(trigger_workflow))
        .route("/v1/workflows/:id/history", get(get_workflow_history))
        .route("/v1/webhooks/:token", post(trigger_webhook))
        .route("/v1/runs", get(list_runs))
        .route("/v1/runs/:id", get(get_run_detail))
        .route("/v1/usage", get(get_usage))
        .route("/v1/demo", get(get_demo_info))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn get_demo_info(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let usage = db::usage_summary(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({
        "mode": state.config.mode,
        "api_key": "demo_api_key",
        "usage": usage
    })))
}

async fn list_workflows(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let workflows = db::list_workflows(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(workflows)))
}

async fn create_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkflowRequest>,
) -> Result<Json<Value>, ApiError> {
    require_api_key(&state, &headers).await?;
    let workflow = db::create_workflow(&state.pool, request)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(json!(workflow)))
}

async fn update_workflow_draft(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<UpdateWorkflowDraftRequest>,
) -> Result<Json<Value>, ApiError> {
    require_api_key(&state, &headers).await?;
    let workflow = db::update_workflow_draft(&state.pool, id, request)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(json!(workflow)))
}

async fn publish_workflow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_api_key(&state, &headers).await?;
    let published = db::publish_workflow(&state.pool, id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(published)))
}

async fn get_workflow_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let workflows = db::list_workflows(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let history = db::workflow_history(&state.pool, id)
        .await
        .map_err(ApiError::internal)?;
    let workflow = workflows.into_iter().find(|workflow| workflow.id == id);
    Ok(Json(json!({
        "workflow": workflow,
        "history": history
    })))
}

async fn trigger_workflow(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<TriggerWorkflowRequest>,
) -> Result<Json<Value>, ApiError> {
    require_api_key(&state, &headers).await?;
    let idempotency_key = request.idempotency_key.or_else(|| {
        headers
            .get("Idempotency-Key")
            .and_then(|value| value.to_str().ok().map(str::to_string))
    });
    let response = db::trigger_workflow(
        &state.pool,
        &slug,
        request.payload,
        idempotency_key,
        "api".to_string(),
    )
    .await
    .map_err(ApiError::bad_request)?;
    Ok(Json(json!(response)))
}

async fn trigger_webhook(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let response = db::trigger_webhook(&state.pool, &token, payload)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(json!(response)))
}

async fn list_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let runs = db::list_runs(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(runs)))
}

async fn get_run_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let detail = db::get_run_detail(&state.pool, id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(detail)))
}

async fn get_usage(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let usage = db::usage_summary(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!(usage)))
}

async fn require_api_key(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(key) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing x-api-key header",
        ));
    };
    let valid = db::validate_api_key(&state.pool, key)
        .await
        .map_err(ApiError::internal)?;
    if valid {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid API key"))
    }
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.to_string())
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
