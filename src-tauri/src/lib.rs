// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use serde::{Serialize};
use std::fs;
use std::collections::{HashMap,HashSet};
use std::ops::Index;
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

#[derive(Serialize)]
struct IndexStat{
    added: u32,
    updated: u32,
    removed: u32,
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
        );",).map_err(|e|e.to_string())?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS files (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            lib_id   INTEGER NOT NULL,
            rel_path TEXT NOT NULL,
            type     TEXT NOT NULL,
            size     INTEGER NOT NULL,      -- 字节数
            mtime    INTEGER NOT NULL,      -- 修改时间（Unix 秒）
            UNIQUE(lib_id, rel_path)
        );",).map_err(|e|e.to_string())
}
fn open_db(app: &tauri::AppHandle)->Result<Connection, String>{
    let dir = app.path().app_data_dir().expect("配置目录不可用");
    let _ = fs::create_dir_all(&dir);
    let db = Connection::open(dir.join("luminara.db")).map_err(|e|e.to_string())?;
    init_db(&db)?;
    Ok(db)
}

const VIDEO_EXT: [&str; 14] = ["mp4","mkv","avi","mov","webm","flv","wmv","m4v","ts","rm","rmvb","mpg","mpeg","3gp"];
const SUB_EXT: [&str; 6] = ["srt","ass","ssa","sub","vtt","idx"];
fn file_type(ext: &str) -> Option<&'static str> {
    if VIDEO_EXT.contains(&ext) {Some("video")}
    else if SUB_EXT.contains(&ext) {Some("subtitle")}
    else {None}
}
fn scan_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, &'static str, i64, i64)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_files(root, &path, out);              // 递归子目录
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        let Some(ty) = file_type(&ext) else {continue};
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let rel_str = rel.to_string_lossy().replace('\\', "/"); // Windows 反斜杠归一化
        let size = meta.len() as i64;
        let mtime = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push((rel_str, ty, size, mtime));
    }
}
#[tauri::command]
async fn add_local_source(app: tauri::AppHandle)->Result<Option<i64>, String>{
    let picked = app.dialog().file().set_title("选择媒体文件夹").blocking_pick_folder();
    let Some(folder) = picked else {
        return Ok(None);
    };
    let path = folder.simplified().into_path().map_err(|e|e.to_string())?;
    let path_str = path.to_string_lossy().to_string();
    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path_str.clone());
    let db = open_db(&app)?;
    db.execute("INSERT OR IGNORE INTO sources (source,name,data_a) VALUES (0,?1,?2)",params![name,path_str]).map_err(|e|e.to_string())?;
    let id: i64 = db.query_row("SELECT id FROM sources WHERE data_a = ?1", params![path_str], |r| r.get(0)).map_err(|e|e.to_string())?;
    Ok(Some(id))
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
#[tauri::command]
async fn index_source(app: tauri::AppHandle, id: i64)->Result<IndexStat, String>{
    let mut db = open_db(&app)?;
    let root_str: String = db.query_row("SELECT data_a FROM sources WHERE id = ?1", params![id], |r| r.get(0)).map_err(|e|e.to_string())?;
    let root = std::path::Path::new(&root_str);
    let mut files = Vec::new();
    scan_files(root, root, &mut files);
    let mut old: HashMap<String, (String, i64, i64)> = HashMap::new();
    {
        let mut stmt = db.prepare("SELECT rel_path, type, size, mtime FROM files WHERE lib_id = ?1").map_err(|e|e.to_string())?;
        let rows = stmt.query_map(params![id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,i64>(2)?, r.get::<_,i64>(3)?))).map_err(|e|e.to_string())?;
        for row in rows {
            let (p, t, s, m) = row.map_err(|e|e.to_string())?;
            old.insert(p, (t, s, m));
        }
    }
    let mut seen = HashSet::new();
    let (mut added, mut updated, mut removed) = (0u32, 0u32, 0u32);
    let tx = db.transaction().map_err(|e|e.to_string())?;
    {
        let mut ins = tx.prepare("INSERT INTO files (lib_id,rel_path,type,size,mtime) VALUES (?1,?2,?3,?4,?5)").map_err(|e|e.to_string())?;
        let mut upd = tx.prepare("UPDATE files SET type=?1, size=?2, mtime=?3 WHERE lib_id=?4 AND rel_path=?5").map_err(|e|e.to_string())?;
        for (rel, ty, size, mtime) in &files {
            seen.insert(rel.clone());
            match old.get(rel) {
                None => {                                    // 新文件
                    ins.execute(params![id, rel, ty, size, mtime]).map_err(|e|e.to_string())?;
                    added += 1;
                }
                Some((ot, os, om)) if ot != ty || os != size || om != mtime => {  // 有变化
                    upd.execute(params![ty, size, mtime, id, rel]).map_err(|e|e.to_string())?;
                    updated += 1;
                }
                _ => {}                                      // 没变：不写库
            }
        }
    }
    {
        let mut del = tx.prepare("DELETE FROM files WHERE lib_id=?1 AND rel_path=?2").map_err(|e|e.to_string())?;
        for p in old.keys() {
            if !seen.contains(p) {                           // 磁盘上已删除
                del.execute(params![id, p]).map_err(|e|e.to_string())?;
                removed += 1;
            }
        }
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(IndexStat{added, updated, removed})

}



#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![add_local_source, list_sources, delete_source, index_source])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
