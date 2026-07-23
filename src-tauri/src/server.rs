use std::{io, path::PathBuf};

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Serialize;
use serde_json::Value;
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    ai, files,
    models::{
        AppData, AppSettings, FileJobResult, Glossary, PromptTemplate, Provider, TranslateRequest,
        TranslationResult,
    },
    store::AppStore,
};

#[derive(Clone)]
pub struct ServerState {
    pub store: AppStore,
    pub server_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapResponse {
    providers: Vec<Provider>,
    glossaries: Vec<Glossary>,
    prompts: Vec<PromptTemplate>,
    settings: AppSettings,
    server_url: String,
}

#[derive(Serialize)]
struct MessageResponse {
    message: String,
}

pub async fn start_server(
    state: ServerState,
    host: &str,
    port: u16,
    asset_dir: PathBuf,
) -> io::Result<()> {
    let app = api_router(state).fallback_service(
        ServeDir::new(&asset_dir).not_found_service(ServeFile::new(asset_dir.join("index.html"))),
    );
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("Tranova Web is listening at http://{host}:{port}");
    axum::serve(listener, app).await.map_err(io::Error::other)
}

fn api_router(state: ServerState) -> Router {
    let origins = [
        HeaderValue::from_static("http://localhost:1420"),
        HeaderValue::from_static("http://127.0.0.1:1420"),
        HeaderValue::from_static("http://tauri.localhost"),
        HeaderValue::from_static("https://tauri.localhost"),
        HeaderValue::from_static("tauri://localhost"),
    ];
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/translate", post(translate))
        .route("/api/translate-file", post(translate_file))
        .route("/api/providers", put(save_provider))
        .route("/api/providers/{id}", delete(delete_provider))
        .route("/api/providers/{id}/test", post(test_provider))
        .route("/api/glossaries", put(save_glossary))
        .route("/api/glossaries/{id}", delete(delete_glossary))
        .route("/api/prompts", put(save_prompt))
        .route("/api/prompts/{id}", delete(delete_prompt))
        .route("/api/settings", put(save_settings))
        .route("/api/import/{kind}", post(import_data))
        .layer(DefaultBodyLimit::max(75 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn health() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "ok".to_string(),
    })
}

async fn bootstrap(State(state): State<ServerState>) -> Json<BootstrapResponse> {
    let AppData {
        providers,
        glossaries,
        prompts,
        settings,
    } = state.store.snapshot();
    Json(BootstrapResponse {
        providers,
        glossaries,
        prompts,
        settings,
        server_url: state.server_url,
    })
}

async fn translate(
    State(state): State<ServerState>,
    Json(request): Json<TranslateRequest>,
) -> Result<Json<TranslationResult>, ApiError> {
    Ok(Json(
        ai::translate(&state.store, &request)
            .await
            .map_err(ApiError::from_ai)?,
    ))
}

async fn translate_file(
    State(state): State<ServerState>,
    mut multipart: Multipart,
) -> Result<Json<FileJobResult>, ApiError> {
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut options: Option<TranslateRequest> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(ApiError::bad_request)?
    {
        match field.name() {
            Some("file") => {
                let name = field
                    .file_name()
                    .unwrap_or("document")
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("document")
                    .to_string();
                let bytes = field.bytes().await.map_err(ApiError::bad_request)?.to_vec();
                file = Some((name, bytes));
            }
            Some("options") => {
                let text = field.text().await.map_err(ApiError::bad_request)?;
                options = Some(serde_json::from_str(&text).map_err(ApiError::bad_request)?);
            }
            _ => {}
        }
    }
    let (filename, bytes) = file.ok_or_else(|| ApiError::bad_request("A file is required"))?;
    let options =
        options.ok_or_else(|| ApiError::bad_request("Translation options are required"))?;
    Ok(Json(
        files::translate_file(&state.store, &filename, bytes, options)
            .await
            .map_err(ApiError::from_file)?,
    ))
}

async fn save_provider(
    State(state): State<ServerState>,
    Json(provider): Json<Provider>,
) -> Result<Json<Provider>, ApiError> {
    Ok(Json(
        state
            .store
            .save_provider(provider)
            .map_err(ApiError::from_io)?,
    ))
}

async fn delete_provider(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_provider(&id)
        .map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_provider(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    Ok(Json(MessageResponse {
        message: ai::test_provider(&state.store, &id)
            .await
            .map_err(ApiError::from_ai)?,
    }))
}

async fn save_glossary(
    State(state): State<ServerState>,
    Json(glossary): Json<Glossary>,
) -> Result<Json<Glossary>, ApiError> {
    Ok(Json(
        state
            .store
            .save_glossary(glossary)
            .map_err(ApiError::from_io)?,
    ))
}

async fn delete_glossary(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_glossary(&id)
        .map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_prompt(
    State(state): State<ServerState>,
    Json(prompt): Json<PromptTemplate>,
) -> Result<Json<PromptTemplate>, ApiError> {
    Ok(Json(
        state.store.save_prompt(prompt).map_err(ApiError::from_io)?,
    ))
}

async fn delete_prompt(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_prompt(&id).map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_settings(
    State(state): State<ServerState>,
    Json(settings): Json<AppSettings>,
) -> Result<Json<AppSettings>, ApiError> {
    Ok(Json(
        state
            .store
            .save_settings(settings)
            .map_err(ApiError::from_io)?,
    ))
}

async fn import_data(
    State(state): State<ServerState>,
    Path(kind): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<BootstrapResponse>, ApiError> {
    match kind.as_str() {
        "glossaries" => state
            .store
            .replace_glossaries(read_import(body, "glossaries")?)
            .map_err(ApiError::from_io)?,
        "prompts" => state
            .store
            .replace_prompts(read_import(body, "prompts")?)
            .map_err(ApiError::from_io)?,
        _ => return Err(ApiError::bad_request("Unknown import type")),
    }
    let AppData {
        providers,
        glossaries,
        prompts,
        settings,
    } = state.store.snapshot();
    Ok(Json(BootstrapResponse {
        providers,
        glossaries,
        prompts,
        settings,
        server_url: state.server_url,
    }))
}

fn read_import<T: serde::de::DeserializeOwned>(
    body: Value,
    field: &str,
) -> Result<Vec<T>, ApiError> {
    let value = match body {
        Value::Array(_) => body,
        Value::Object(mut object) => object
            .remove(field)
            .ok_or_else(|| ApiError::bad_request(format!("Expected a '{field}' array")))?,
        _ => {
            return Err(ApiError::bad_request(
                "Expected a JSON array or export object",
            ))
        }
    };
    serde_json::from_value(value).map_err(ApiError::bad_request)
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
    fn from_io(error: std::io::Error) -> Self {
        let status = if error.kind() == std::io::ErrorKind::InvalidInput {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
    fn from_ai(error: ai::AiError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        }
    }
    fn from_file(error: files::FileError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
