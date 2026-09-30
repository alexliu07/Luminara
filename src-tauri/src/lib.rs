// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use serde::{Serialize};
use std::fs;
use std::collections::{HashMap,HashSet};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use rusqlite::{params, Connection};

// 文件源结构体
#[derive(Serialize)]
struct Source{
    id: i64,
    source: i32,
    name: String,
    data_a: String
}
// 索引更新数据结构体
#[derive(Serialize)]
struct IndexStat{
    added: u32,
    updated: u32,
    removed: u32,
}
// 文件条目结构体
#[derive(Serialize)]
struct FileEntry{
    rel_path: String,
    kind: String,      // video / subtitle
    size: i64,
    mtime: i64,
}

// 数据库初始化
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

// 打开数据库
fn open_db(app: &tauri::AppHandle)->Result<Connection, String>{
    let dir = app.path().app_data_dir().expect("配置目录不可用");
    let _ = fs::create_dir_all(&dir);
    let db = Connection::open(dir.join("luminara.db")).map_err(|e|e.to_string())?;
    init_db(&db)?;
    Ok(db)
}

// 文件类型定义和判断
const VIDEO_EXT: [&str; 14] = ["mp4","mkv","avi","mov","webm","flv","wmv","m4v","ts","rm","rmvb","mpg","mpeg","3gp"];
const SUB_EXT: [&str; 6] = ["srt","ass","ssa","sub","vtt","idx"];
fn file_type(ext: &str) -> Option<&'static str> {
    if VIDEO_EXT.contains(&ext) {Some("video")}
    else if SUB_EXT.contains(&ext) {Some("subtitle")}
    else {None}
}

// 扫描文件
fn scan_local_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, &'static str, i64, i64)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_local_files(root, &path, out);              // 递归子目录
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
// 扫描结果与数据库旧记录做差异并写入（本地/WebDAV 共用）
fn write_index(db: &mut Connection, id: i64, files: &[(String, &'static str, i64, i64)])->Result<IndexStat, String>{
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
        for (rel, ty, size, mtime) in files {
            seen.insert(rel.clone());
            match old.get(rel) {
                None => {
                    ins.execute(params![id, rel, ty, size, mtime]).map_err(|e|e.to_string())?;
                    added += 1;
                }
                Some((ot, os, om)) if ot != ty || os != size || om != mtime => {
                    upd.execute(params![ty, size, mtime, id, rel]).map_err(|e|e.to_string())?;
                    updated += 1;
                }
                _ => {}
            }
        }
    }
    {
        let mut del = tx.prepare("DELETE FROM files WHERE lib_id=?1 AND rel_path=?2").map_err(|e|e.to_string())?;
        for p in old.keys() {
            if !seen.contains(p) {
                del.execute(params![id, p]).map_err(|e|e.to_string())?;
                removed += 1;
            }
        }
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(IndexStat{added, updated, removed})
}
// URL百分号解码
fn percent_decode(s: &str)->String{
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i+1..i+3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
// href → 解码后的路径部分（去掉协议主机、query和末尾斜杠）
fn href_path(href: &str)->String{
    let rest = match href.find("://") {
        Some(i) => {
            let r = &href[i+3..];
            &r[r.find('/').unwrap_or(r.len())..]
        }
        None => href
    };
    percent_decode(rest.split('?').next().unwrap_or("")).trim_end_matches('/').to_string()
}
// href → 最后一段名字（文件名/目录名）
fn href_name(href: &str)->String{
    href_path(href).rsplit('/').next().unwrap_or("").to_string()
}
// 相对href拼成完整URL
fn resolve_href(base: &str, href: &str)->String{
    if href.contains("://") { return href.to_string() }
    let i = base.find("://").map(|i| i + 3).unwrap_or(0);
    let rest = &base[i..];
    let origin_len = rest.find('/').map(|j| i + j).unwrap_or(base.len());
    format!("{}{}", &base[..origin_len], href)
}
// 解析PROPFIND的207 Multi-Status，返回 (href, 是否目录, size, mtime)
fn parse_multistatus(xml: &str)->Result<Vec<(String, bool, i64, i64)>, String>{
    let doc = roxmltree::Document::parse(xml).map_err(|e|e.to_string())?;
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| n.tag_name().name() == "response") {
        let (mut href, mut is_dir, mut size, mut mtime) = (String::new(), false, 0i64, 0i64);
        for n in resp.descendants() {
            match n.tag_name().name() {
                "href" if href.is_empty() => href = n.text().unwrap_or("").trim().to_string(),
                "collection" => is_dir = true,
                "getcontentlength" => size = n.text().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
                "getlastmodified" => mtime = n.text().and_then(|t| httpdate::parse_http_date(t.trim()).ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64).unwrap_or(0),
                _ => {}
            }
        }
        if !href.is_empty() { out.push((href, is_dir, size, mtime)) }
    }
    Ok(out)
}
// 对一个目录发 Depth:1 的 PROPFIND，返回它的子项
async fn webdav_propfind(client: &reqwest::Client, url: &str, user: &str, pass: &str)->Result<Vec<(String, bool, i64, i64)>, String>{
    let method = reqwest::Method::from_bytes(b"PROPFIND").map_err(|e|e.to_string())?;
    let mut req = client.request(method, url)
        .header("Depth", "1")
        .header("Content-Type", "application/xml")
        .body(r#"<?xml version="1.0" encoding="utf-8"?><propfind xmlns="DAV:"><allprop/></propfind>"#);
    if !user.is_empty() {
        req = req.basic_auth(user, Some(pass));
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("服务器返回 {}", resp.status().as_u16()));
    }
    let xml = resp.text().await.map_err(|e|e.to_string())?;
    parse_multistatus(&xml)
}
// 添加本地文件源
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
// 展示文件源列表
#[tauri::command]
fn list_sources(app: tauri::AppHandle)->Result<Vec<Source>, String>{
    let db = open_db(&app)?;
    let mut stmt = db.prepare("SELECT * FROM sources ORDER BY id").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([],|row|Ok(Source{id: row.get(0)?,source: row.get(1)?,name: row.get(2)?,data_a: row.get(3)?})).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
// 删除文件源
#[tauri::command]
fn delete_source(app: tauri::AppHandle,id: i64)->Result<(), String>{
    let db = open_db(&app)?;
    db.execute("DELETE FROM sources WHERE id = ?1",params![id]).map_err(|e|e.to_string())?;
    Ok(())
}
//扫描文件源
#[tauri::command]
async fn index_local_source(app: tauri::AppHandle, id: i64)->Result<IndexStat, String>{
    let mut db = open_db(&app)?;
    let root_str: String = db.query_row("SELECT data_a FROM sources WHERE id = ?1", params![id], |r| r.get(0)).map_err(|e|e.to_string())?;
    let root = std::path::Path::new(&root_str);
    let mut files = Vec::new();
    scan_local_files(root, root, &mut files);
    write_index(&mut db,id,&files)
}
// 添加WebDAV文件源
#[tauri::command]
fn add_webdav_source(app: tauri::AppHandle, name: String, host: String, port: u16, https: bool, username: String, password: String, path: String)->Result<i64, String>{
    let path = if path.starts_with('/') { path } else { format!("/{}", path) };
    let url = format!("{}://{}:{}{}", if https {"https"} else {"http"}, host, port, path);
    let db = open_db(&app)?;
    db.execute("INSERT OR IGNORE INTO sources (source,name,data_a,data_b,data_c,data_d,data_e,data_f,data_g) VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8)",
               params![name, url, host, port.to_string(), if https {"1"} else {"0"}, username, password, path]).map_err(|e|e.to_string())?;
    let id: i64 = db.query_row("SELECT id FROM sources WHERE data_a = ?1", params![url], |r| r.get(0)).map_err(|e|e.to_string())?;
    Ok(id)
}
// 测试WebDAV连接
#[tauri::command]
async fn test_webdav_source(host: String, port: u16, https: bool, username: String, password: String, path: String)->Result<(), String>{
    let path = if path.starts_with('/') { path } else { format!("/{}", path) };
    let url = format!("{}://{}:{}{}", if https {"https"} else {"http"}, host, port, path);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build().map_err(|e|e.to_string())?;
    let method = reqwest::Method::from_bytes(b"PROPFIND").map_err(|e|e.to_string())?;
    let mut req = client.request(method, &url)
        .header("Depth", "0")
        .header("Content-Type", "application/xml")
        .body(r#"<?xml version="1.0" encoding="utf-8"?><propfind xmlns="DAV:"><allprop/></propfind>"#);
    if !username.is_empty() {
        req = req.basic_auth(&username, Some(&password));
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() { "连接超时".to_string() } else { format!("无法连接：{}", e) }
    })?;
    let code = resp.status().as_u16();
    if (200..300).contains(&code) { Ok(()) }                    // 207 Multi-Status 也是成功
    else if code == 401 { Err("用户名或密码错误".into()) }
    else if code == 403 { Err("没有访问权限".into()) }
    else if code == 404 { Err("路径不存在".into()) }
    else { Err(format!("服务器返回 {}", code)) }
}
// 扫描WebDAV文件源
#[tauri::command]
async fn index_webdav_source(app: tauri::AppHandle, id: i64)->Result<IndexStat, String>{
    let mut db = open_db(&app)?;
    let (root_url, user, pass): (String, String, String) = db
        .query_row("SELECT data_a, data_e, data_f FROM sources WHERE id = ?1", params![id],
                   |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e|e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().map_err(|e|e.to_string())?;
    let mut out: Vec<(String, &'static str, i64, i64)> = Vec::new();
    let mut queue = vec![(format!("{}/", root_url.trim_end_matches('/')), String::new())]; // (目录URL, rel前缀)
    let mut is_root = true;
    while let Some((dir_url, rel)) = queue.pop() {
        let entries = match webdav_propfind(&client, &dir_url, &user, &pass).await {
            Ok(e) => e,
            Err(e) => {
                if is_root { return Err(e) }   // 根目录失败才报错，坏子目录跳过
                continue
            }
        };
        is_root = false;
        for (href, is_dir, size, mtime) in entries {
            if href_path(&href) == href_path(&dir_url) { continue }  // 跳过目录自身条目
            let name = href_name(&href);
            if name.is_empty() { continue }
            if is_dir {
                let mut child = resolve_href(&dir_url, &href);
                if !child.ends_with('/') { child.push('/') }        // 目录URL必须带斜杠
                queue.push((child, format!("{}{}/", rel, name)));
            } else {
                let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                let Some(ty) = file_type(&ext) else { continue };   // 只收视频/字幕
                out.push((format!("{}{}", rel, name), ty, size, mtime));
            }
        }
    }
    write_index(&mut db, id, &out)
}
// 列出文件源下已索引的文件
#[tauri::command]
fn list_files(app: tauri::AppHandle, id: i64)->Result<Vec<FileEntry>, String>{
    let db = open_db(&app)?;
    let mut stmt = db.prepare("SELECT rel_path, type, size, mtime FROM files WHERE lib_id = ?1 ORDER BY rel_path").map_err(|e|e.to_string())?;
    let rows = stmt.query_map(params![id], |r| Ok(FileEntry{
        rel_path: r.get(0)?,
        kind: r.get(1)?,
        size: r.get(2)?,
        mtime: r.get(3)?,
    })).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![add_local_source, list_sources, delete_source, index_local_source, add_webdav_source, test_webdav_source, index_webdav_source, list_files])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
