mod commands;
mod resources;
mod session;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            let runtime = state.runtime.clone();
            app.manage(state);
            // Free the warm model's ~800 MB of pages after a period of
            // inactivity so an idle Veda does not pin them indefinitely.
            tauri::async_runtime::spawn(crate::session::session_janitor(runtime));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_preflight,
            commands::resource_catalog,
            commands::prepare_resources,
            commands::list_docsets,
            commands::list_downloads,
            commands::install_docset,
            commands::remove_docset,
            commands::ask_veda,
            commands::read_source,
            commands::open_source,
            commands::reveal_data_folder,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Veda");
}
