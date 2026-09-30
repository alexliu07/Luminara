// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use serde::{Serialize};
use std::fs;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use rusqlite::{params, Connection};

#[derive(Serialize)]
struct Source{
    id: i64,
    source: i32,
    name: String,
    data_a: String
}

fn init_db(db: &Connection)->Result<(), String>{
    db.execute_batch("CREATE TABLE IF NOT EXISTS sources (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            source   INTEGER NOT NULL,
            name     TEXT NOT NULL,
            data_a   TEXT NOT NULL UNIQUE,
            data_b   TEXT,
            data_c   TEXT,
            data_d   TEXT,
            data_e   TEXT,
            data_f   TEXT,
            data_g   TEXT,
            data_h   TEXT,
            data_i   TEXT,
            data_j   TEXT,
            added_at TEXT DEFAULT (datetime('now'))
        );",).map_err(|e|e.to_string())
}
fn open_db(app: &tauri::AppHandle)->Result<Connection, String>{
    let dir = app.path().app_data_dir().expect("配置目录不可用");
    let _ = fs::create_dir_all(&dir);
    let db = Connection::open(dir.join("luminara.db")).map_err(|e|e.to_string())?;
    init_db(&db)?;
    Ok(db)
}

#[tauri::command]
async fn add_local_source(app: tauri::AppHandle)->Result<bool, String>{
    let picked = app.dialog().file().set_title("选择媒体文件夹").blocking_pick_folder();
    let Some(folder) = picked else {
        return Ok(false);
    };
    let path = folder.simplified().into_path().map_err(|e|e.to_string())?;
    let path_str = path.to_string_lossy().to_string();
    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path_str.clone());
    let db = open_db(&app)?;
    db.execute("INSERT OR IGNORE INTO sources (source,name,data_a) VALUES (0,?1,?2)",params![name,path_str]).map_err(|e|e.to_string())?;
    Ok(true)
}
#[tauri::command]
fn list_sources(app: tauri::AppHandle)->Result<Vec<Source>, String>{
    let db = open_db(&app)?;
    let mut stmt = db.prepare("SELECT * FROM sources ORDER BY id").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([],|row|Ok(Source{id: row.get(0)?,source: row.get(1)?,name: row.get(2)?,data_a: row.get(3)?})).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
#[tauri::command]
fn delete_source(app: tauri::AppHandle,id: i64)->Result<(), String>{
    let db = open_db(&app)?;
    db.execute("DELETE FROM sources WHERE id = ?1",params![id]).map_err(|e|e.to_string())?;
    Ok(())
}



#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![add_local_source, list_sources, delete_source])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
