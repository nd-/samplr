// samplr's graphical interface: a window showing the web page of ui/, which
// calls the Rust functions marked `#[tauri::command]` below.

// On Windows, a release build starts without opening a console window
// next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use samplr::db;

/// Number of samples in the database.
/// Called from JavaScript with `invoke("sample_count")`. The error travels to
/// the page as text: `Box<dyn Error>` cannot be turned into JSON, a `String` can.
#[tauri::command]
fn sample_count() -> Result<usize, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let files = db::all_files(&db).map_err(|e| e.to_string())?;
    Ok(files.len())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![sample_count])
        .run(tauri::generate_context!())
        .expect("could not start the interface");
}
