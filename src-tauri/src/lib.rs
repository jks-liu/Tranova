mod ai;
mod files;
mod models;
mod server;
mod store;

use std::path::PathBuf;

use server::ServerState;
use store::AppStore;
use tauri::Manager;

#[derive(Clone)]
struct DesktopState {
    server_url: String,
}

#[tauri::command]
fn server_url(state: tauri::State<'_, DesktopState>) -> String {
    state.server_url.clone()
}

pub fn run() {
    let store = AppStore::load().expect("unable to initialize Tranova's user data store");
    let settings = store.settings();
    let server_address = web_url(&settings.web_host, settings.web_port);
    let server_state = ServerState {
        store,
        server_url: server_address.clone(),
    };

    tauri::Builder::default()
        .manage(DesktopState {
            server_url: server_address,
        })
        .invoke_handler(tauri::generate_handler![server_url])
        .setup(move |app| {
            let asset_dir = web_asset_dir(app.handle());
            let settings = server_state.store.settings();
            let state = server_state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    server::start_server(state, &settings.web_host, settings.web_port, asset_dir)
                        .await
                {
                    eprintln!("Tranova Web server could not start: {error}");
                }
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
