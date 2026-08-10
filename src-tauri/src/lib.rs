mod ai;
mod files;
mod jobs;
mod models;
mod scheduler;
mod server;
mod store;

use std::{fs, path::PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use jobs::FileJobManager;
use models::FileJobStatus;
use serde::Serialize;
use server::ServerState;
use store::AppStore;
use tauri::Manager;

#[derive(Clone)]
struct DesktopState {
    server_url: String,
    jobs: FileJobManager,
    store: AppStore,
}

#[tauri::command]
fn server_url(state: tauri::State<'_, DesktopState>) -> String {
    state.server_url.clone()
}

#[tauri::command]
fn save_file_job(
    state: tauri::State<'_, DesktopState>,
    job_id: String,
    destination: String,
) -> Result<FileJobStatus, String> {
    let destination = PathBuf::from(destination);
    let status = state
        .jobs
        .save_to_path(&job_id, &destination)
        .map_err(|error| error.to_string())?;
    if let Some(parent) = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        // The file is already saved; a preference persistence error must not turn that into a failed download.
        let _ = state.store.set_last_download_directory(parent);
    }
    Ok(status)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DroppedFile {
    name: String,
    content_base64: String,
}

#[tauri::command]
fn read_dropped_file(path: String) -> Result<DroppedFile, String> {
    let path = PathBuf::from(path);
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Dropped path is not a file".to_string());
    }
    if metadata.len() > 75 * 1024 * 1024 {
        return Err("Dropped file exceeds the 75 MB limit".to_string());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .to_string();
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(DroppedFile {
        name,
        content_base64: BASE64.encode(bytes),
    })
}

pub fn run() {
    let store = AppStore::load().expect("unable to initialize Tranova's user data store");
    let settings = store.settings();
    let server_address = web_url(&settings.web_host, settings.web_port);
    let server_state = ServerState {
        store,
        server_url: server_address.clone(),
        jobs: FileJobManager::new(),
    };
    let jobs = server_state.jobs.clone();
    let store = server_state.store.clone();

    tauri::Builder::default()
        .manage(DesktopState {
            server_url: server_address,
            jobs,
            store,
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            server_url,
            read_dropped_file,
            save_file_job
        ])
        .setup(move |app| {
            let asset_dir = web_asset_dir(app.handle());
            let settings = server_state.store.settings();
            let state = server_state.clone();
            tauri::async_runtime::spawn(async move {
                let _ =
                    server::start_server(state, &settings.web_host, settings.web_port, asset_dir)
                        .await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tranova");
}

pub fn run_server() {
    let store = AppStore::load().expect("unable to initialize Tranova's user data store");
    let settings = store.settings();
    let server_address = web_url(&settings.web_host, settings.web_port);
    let state = ServerState {
        store,
        server_url: server_address,
        jobs: FileJobManager::new(),
    };
    let assets = standalone_asset_dir();
    let runtime = tokio::runtime::Runtime::new().expect("unable to initialize the async runtime");
    runtime
        .block_on(server::start_server(
            state,
            &settings.web_host,
            settings.web_port,
            assets,
        ))
        .expect("Tranova Web server stopped unexpectedly");
}

fn web_url(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

fn web_asset_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled_assets = resource_dir.join("dist");
        if bundled_assets.is_dir() {
            return bundled_assets;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("dist")
}

fn standalone_asset_dir() -> PathBuf {
    if let Some(parent) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
    {
        for candidate in [parent.join("resources").join("dist"), parent.join("dist")] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("dist")
}
