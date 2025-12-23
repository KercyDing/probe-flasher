#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
use commands::AppState;

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_ports,
            commands::identify_port,
            commands::flash_firmware,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
