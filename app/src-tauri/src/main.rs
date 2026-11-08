//! Hermes desktop app. Reads the encrypted archive through `hermes-viewer` after the two-key unlock,
//! controls the visible tray recorder, and records nothing itself.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod collector;
mod commands;
mod prefs;
mod queries;
mod segments;
mod state;

fn main() {
    let data_dir = state::resolve_data_dir();
    let prefs = prefs::Prefs::load(&state::prefs_path(&data_dir));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::new(data_dir, prefs))
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::save_prefs,
            commands::lock_info,
            commands::detect_keys,
            commands::init_archive,
            commands::unlock,
            commands::lock_archive,
            commands::key_present,
            commands::integrity_info,
            commands::recheck,
            commands::summary,
            commands::timeline,
            commands::focus_list,
            commands::app_detail,
            commands::heatmap,
            commands::explore,
            commands::export_events,
            commands::collector_info,
            commands::collector_control,
            commands::collection_settings_get,
            commands::collection_settings_set,
            commands::autostart_set,
            commands::open_data_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hermes");
}
