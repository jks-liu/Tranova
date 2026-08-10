use std::{io, path::PathBuf, sync::Arc};

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderValue, Method, StatusCode},
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
    jobs::FileJobManager,
    models::{
        AppData, AppSettings, FileJobStatus, Glossary, HistoryEntry, PromptTemplate, Provider,
        TranslateOptions, TranslateRequest, TranslationResult,
    },
    scheduler::{AiScheduler, Priority},
    store::AppStore,
};

#[derive(Clone)]
pub struct ServerState {
    pub store: AppStore,
    pub server_url: String,
    pub jobs: FileJobManager,
}

#[derive(Clone)]
struct ApiState {
    store: AppStore,
    server_url: String,
    scheduler: AiScheduler,
    jobs: FileJobManager,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapResponse {
    version: String,
    providers: Vec<Provider>,
    glossaries: Vec<Glossary>,
    prompts: Vec<PromptTemplate>,
    settings: AppSettings,
    history: Vec<HistoryEntry>,
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
    let store = state.store.clone();
    let api_state = ApiState {
        store: store.clone(),
        server_url: state.server_url,
        scheduler: AiScheduler::new(store),
        jobs: state.jobs,
    };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/translate", post(translate))
        .route("/api/translate-file", post(translate_file))
        .route("/api/file-jobs", get(file_jobs))
        .route("/api/file-jobs/{id}", get(file_job))
        .route("/api/file-jobs/{id}/download", get(download_file_job))
        .route("/api/file-jobs/{id}/retry", post(retry_file_job))
        .route("/api/history", get(history).delete(clear_history))
        .route("/api/history/{id}", delete(delete_history))
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
        .with_state(api_state)
}

async fn health() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "ok".to_string(),
    })
}

async fn bootstrap(State(state): State<ApiState>) -> Json<BootstrapResponse> {
    let AppData {
        providers,
        glossaries,
        prompts,
        settings,
        history,
    } = state.store.snapshot();
    Json(BootstrapResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        providers,
        glossaries,
        prompts,
        settings,
        history,
        server_url: state.server_url,
    })
}

async fn translate(
    State(state): State<ApiState>,
    Json(request): Json<TranslateRequest>,
) -> Result<Json<TranslationResult>, ApiError> {
    let result = state
        .scheduler
        .translate(request.clone(), Priority::High)
        .await
        .map_err(ApiError::from_ai)?;
    state
        .store
        .add_history(history_for_text(&request, &result))
        .map_err(ApiError::from_io)?;
    Ok(Json(result))
}

async fn translate_file(
    State(state): State<ApiState>,
    mut multipart: Multipart,
) -> Result<Json<FileJobStatus>, ApiError> {
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut options: Option<TranslateOptions> = None;
    let mut source_path: Option<String> = None;
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
            Some("sourcePath") => {
                source_path = Some(field.text().await.map_err(ApiError::bad_request)?);
            }
            _ => {}
        }
    }
    let (filename, bytes) = file.ok_or_else(|| ApiError::bad_request("A file is required"))?;
    let options =
        options.ok_or_else(|| ApiError::bad_request("Translation options are required"))?;
    let output_mode = options.output_mode;
    let request = options.into_request();
    let status = state.jobs.create(&filename, output_mode, source_path);
    let job_id = status.id.clone();
    let jobs = state.jobs.clone();
    let store = state.store.clone();
    let scheduler = state.scheduler.clone();
    tokio::spawn(async move {
        jobs.mark_processing(&job_id);
        let progress_jobs = jobs.clone();
        let progress_job_id = job_id.clone();
        let progress: files::ProgressCallback = Arc::new(move |value| {
            progress_jobs.update_progress(&progress_job_id, value);
        });
        match files::translate_file(
            &store,
            &scheduler,
            &filename,
            bytes,
            request.clone(),
            output_mode,
            progress,
        )
        .await
        {
            Ok(result) => {
                let history_result = history_for_file(&request, &result.filename);
                if let Err(error) = store.add_history(history_result) {
                    jobs.fail(&job_id, error.to_string());
                } else {
                    jobs.complete(&job_id, result);
                }
            }
            Err(error) => jobs.fail(&job_id, error.to_string()),
        }
    });
    Ok(Json(status))
}

async fn file_jobs(State(state): State<ApiState>) -> Json<Vec<FileJobStatus>> {
    Json(state.jobs.list())
}

async fn file_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<FileJobStatus>, ApiError> {
    state
        .jobs
        .get(&id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("File job was not found"))
}

async fn retry_file_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<FileJobStatus>, ApiError> {
    let (status, context) = state.jobs.begin_retry(&id).map_err(ApiError::bad_request)?;
    let jobs = state.jobs.clone();
    let store = state.store.clone();
    let scheduler = state.scheduler.clone();
    let restore_context = context.clone();
    let job_id = id.clone();
    tokio::spawn(async move {
        let progress_jobs = jobs.clone();
        let progress_job_id = job_id.clone();
        let progress: files::ProgressCallback = Arc::new(move |value| {
            progress_jobs.update_progress(&progress_job_id, value);
        });
        match files::retry_failed_batches(&store, &scheduler, context, progress).await {
            Ok(result) => jobs.complete(&job_id, result),
            Err(error) => jobs.restore_retry(&job_id, restore_context, error.to_string()),
        }
    });
    Ok(Json(status))
}

async fn download_file_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (filename, media_type, content) = state
        .jobs
        .download(&id)
        .ok_or_else(|| ApiError::bad_request("File translation is not complete"))?;
    let mut response = content.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&media_type).map_err(ApiError::internal)?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(&filename)).map_err(ApiError::internal)?,
    );
    Ok(response)
}

fn content_disposition(filename: &str) -> String {
    let fallback = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        if fallback.is_empty() {
            "translation"
        } else {
            &fallback
        },
        percent_encode(filename.as_bytes())
    )
}

fn percent_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

async fn delete_history(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_history(&id).map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_history(State(state): State<ApiState>) -> Result<StatusCode, ApiError> {
    state.store.clear_history().map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn history(State(state): State<ApiState>) -> Json<Vec<HistoryEntry>> {
    Json(state.store.snapshot().history)
}

fn history_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{}", timestamp, std::process::id())
}

fn history_for_text(request: &TranslateRequest, result: &TranslationResult) -> HistoryEntry {
    HistoryEntry {
        id: history_id(),
        kind: "text".to_string(),
        source_language: request.source_language.clone(),
        target_language: request.target_language.clone(),
        source_text: request.text.clone(),
        translated_text: result.translated_text.clone(),
        filename: None,
        provider: result.provider.clone(),
        model: result.model.clone(),
        created_at: unix_timestamp(),
    }
}

fn history_for_file(request: &TranslateRequest, filename: &str) -> HistoryEntry {
    HistoryEntry {
        id: history_id(),
        kind: "file".to_string(),
        source_language: request.source_language.clone(),
        target_language: request.target_language.clone(),
        source_text: String::new(),
        translated_text: String::new(),
        filename: Some(filename.to_string()),
        provider: request.provider_id.clone(),
        model: String::new(),
        created_at: unix_timestamp(),
    }
}

fn unix_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

async fn save_provider(
    State(state): State<ApiState>,
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
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_provider(&id)
        .map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_provider(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let provider = state
        .store
        .snapshot()
        .providers
        .into_iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| ApiError::bad_request("Provider was not found"))?;
    let request = TranslateRequest {
        text: "Reply with OK only.".to_string(),
        source_language: "English".to_string(),
        target_language: "English".to_string(),
        provider_id: id,
        prompt_id: None,
        glossary_ids: Vec::new(),
    };
    state
        .scheduler
        .translate(request, Priority::High)
        .await
        .map_err(ApiError::from_ai)?;
    Ok(Json(MessageResponse {
        message: format!("{} responded successfully", provider.name),
    }))
}

async fn save_glossary(
    State(state): State<ApiState>,
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
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_glossary(&id)
        .map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_prompt(
    State(state): State<ApiState>,
    Json(prompt): Json<PromptTemplate>,
) -> Result<Json<PromptTemplate>, ApiError> {
    Ok(Json(
        state.store.save_prompt(prompt).map_err(ApiError::from_io)?,
    ))
}

async fn delete_prompt(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_prompt(&id).map_err(ApiError::from_io)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_settings(
    State(state): State<ApiState>,
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
    State(state): State<ApiState>,
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
        history,
    } = state.store.snapshot();
    Ok(Json(BootstrapResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        providers,
        glossaries,
        prompts,
        settings,
        history,
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
    fn not_found(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: error.to_string(),
        }
    }
    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
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
