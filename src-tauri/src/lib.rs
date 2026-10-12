// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod proxy;
mod mkv;
use serde::{Serialize};
use serde_json::{json, Value};
use std::fs;
use std::collections::{HashMap,HashSet};
use std::sync::OnceLock;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use rusqlite::{params, Connection, OptionalExtension};
use regex::Regex;
use base64::Engine;
use std::sync::atomic::{AtomicBool, Ordering};

// 文件源结构体
#[derive(Serialize)]
struct Source{
    id: i64,
    source: i32,
    name: String,
    data_a: String
}
// 文件条目结构体
#[derive(Serialize)]
struct FileEntry{
    rel_path: String,
    kind: String,      // video / subtitle
    size: i64,
    mtime: i64,
}
// 索引更新数据结构体
#[derive(Serialize)]
struct IndexStat{
    added: u32,
    updated: u32,
    removed: u32,
    works: u32,      // 刮削匹配到的作品数
    scraped: u32,    // 刮削成功的文件数
    unmatched: u32,  // 刮削未匹配的文件数
}
// 一部作品的刮削结果（网络阶段产出，落库阶段写入）
struct WorkScrape{
    tmdb_id: i64,
    kind: &'static str,               // "tv" / "movie"
    title: String,
    poster: Option<String>,
    rating: Option<f64>,
    air_date: Option<String>,
    region: Option<String>,
    certification: Option<String>,
    total_seasons: Option<i64>,
    total_episodes: Option<i64>,
    overview: Option<String>,        // 简介（搜索结果的 overview）
    backdrop: Option<String>, // 背景图链接（backdrop_path）
    genres: Vec<String>,
    cast: Value,
    episodes: Vec<(i64, u32, u32, String, Option<String>, Option<String>)>, // (file_id, 季, 集, 分集标题, 分集封面, 分集简介)
    season_meta: Vec<(u32, Option<String>, Option<String>)>, // (季, 季名, 季海报)
}
// 媒体库：作品条目
#[derive(Serialize)]
struct WorkItem{
    tmdb_id: i64,
    kind: String,             // tv / movie
    title: String,
    poster: Option<String>,   // 已缓存封面的本地绝对路径；没缓存到为 None
    backdrop: Option<String>,
    rating: Option<f64>,
    air_date: Option<String>,
    region: Option<String>,
    certification: Option<String>,
    overview: Option<String>,
    total_seasons: Option<i64>,
    total_episodes: Option<i64>,
    genres: Vec<String>,
}
// 媒体库：未刮削条目
#[derive(Serialize)]
struct UnscrapedItem{
    name: String,      // 文件名（去扩展名），用作标题
    rel_path: String,
}
// 作品详情：分集条目
#[derive(Serialize)]
struct EpisodeItem{
    file_id: i64,
    season: u32,
    episode: u32,
    title: String,
    still: Option<String>,   // 缓存绝对路径
    size: i64,
}
// 演职人员条目
#[derive(Serialize)]
struct CastItem{
    name: String,
    character: String,
    photo: Option<String>,   // 缓存绝对路径
}
// 作品详情
#[derive(Serialize)]
struct WorkDetail{
    tmdb_id: i64,
    kind: String,
    title: String,
    poster: Option<String>,
    backdrop: Option<String>,
    rating: Option<f64>,
    air_date: Option<String>,
    region: Option<String>,
    certification: Option<String>,
    overview: Option<String>,
    total_seasons: Option<i64>,
    total_episodes: Option<i64>,
    genres: Vec<String>,
    cast: Vec<CastItem>,
    total_size: i64,
    episodes: Vec<EpisodeItem>,
}
// 播放信息：字幕条目
#[derive(Serialize)]
struct SubtitleItem{
    file_id: i64,
    track: Option<u32>,   // Some = mkv 内嵌轨号；None = 外挂字幕文件
    label: String,
    format: String,   // srt / vtt / ass / ssa
}
// 播放信息
#[derive(Serialize)]
struct PlaybackInfo{
    file_id: i64,
    name: String,        // 视频文件名（含扩展名）
    src_kind: String,    // local / webdav
    src: String,         // local: 绝对路径（前端 convertFileSrc）；webdav: 本地代理地址
    size: i64,
    subtitles: Vec<SubtitleItem>,
}
// ==================== TMDB 刮削 ====================
const TMDB_API: &str = "https://api.themoviedb.org/3";
const TMDB_API_ALT: &str = "https://tmdb.skylare.me/api/3";
const TMDB_IMG: &str = "https://image.tmdb.org/t/p/original";
const TMDB_IMG_ALT: &str = "https://tmdb.skylare.me/image/t/p/original";
// 发布标签：清洗文件名时按词丢弃
const NAME_TAGS: &[&str] = &["1080p","2160p","720p","480p","576p","4k","8k","1920","2160","1280",
    "x264","x265","h264","h265","hevc","avc","av1","aac","flac","dts","truehd","eac3","ddp","atmos",
    "hdr","hdr10","dv","dovi","remux","bluray","webdl","webrip","web","hdtv","dvdrip","10bit","8bit"];
// 上次成功连上的镜像（false=主站，true=备用）；网络层失败时自动切换
static USE_ALT: AtomicBool = AtomicBool::new(false);
const TMDB_DELAY_MS: u64 = 500;   // 相邻两次请求的最小间隔，被风控就调大

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
        );",).map_err(|e|e.to_string())?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS settings (
            key      TEXT PRIMARY KEY,
            value    TEXT NOT NULL
        );",).map_err(|e|e.to_string())?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS works (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            tmdb_id        INTEGER NOT NULL,          -- TMDB 作品 id
            kind           TEXT NOT NULL,             -- tv / movie
            title          TEXT NOT NULL,             -- 作品名称
            poster         TEXT,                      -- 作品封面链接
            rating         REAL,                      -- 评分
            air_date       TEXT,                      -- 播放日期
            region         TEXT,                      -- 地区
            certification  TEXT,                      -- 分级
            total_seasons  INTEGER,                   -- 总季数（电影为 0）
            total_episodes INTEGER,                   -- 总集数（电影为 1）
            overview       TEXT,                      -- 简介
            backdrop       TEXT,
            genres         TEXT NOT NULL DEFAULT '[]',
            seasons        TEXT NOT NULL DEFAULT '[]',-- JSON:[{season, name, poster, episodes:[{file_id, episode, title, still}]}]
            cast_json      TEXT NOT NULL DEFAULT '[]',-- JSON：[{name, character, photo}]
            UNIQUE(tmdb_id, kind)
        );",).map_err(|e|e.to_string())?;
    Ok(())
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
    prune_works(db)?;
    Ok(IndexStat{added, updated, removed, works: 0, scraped: 0, unmatched: 0})
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
// rel_path 拼进 WebDAV 根 URL，逐段百分号编码（索引里的 rel 是解码过的）
fn webdav_url(root: &str, rel: &str)->String{
    let enc = |s: &str| -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    };
    let path = rel.split('/').map(enc).collect::<Vec<_>>().join("/");
    format!("{}/{}", root.trim_end_matches('/'), path)
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
// 从 settings 表读 API Key
fn get_tmdb_key(db: &Connection)->Result<String, String>{
    let key: Option<String> = db.query_row("SELECT value FROM settings WHERE key = 'tmdb_api_key'", [], |r| r.get(0))
        .optional().map_err(|e|e.to_string())?;
    let key = key.unwrap_or_default().trim().to_string();
    if key.is_empty() { Err("请先在设置中填写 TMDB API Key".into()) } else { Ok(key) }
}

// GET https://api.themoviedb.org/3{path}，用 Read Access Token 走 Bearer 鉴权
// GET {base}{path}，用 Read Access Token 走 Bearer 鉴权；主站连不上自动换 TMDB_API_ALT
async fn tmdb_get(client: &reqwest::Client, key: &str, path: &str, query: &[(&str, String)])->Result<Value, String>{
    let use_alt = USE_ALT.load(Ordering::Relaxed);
    let first = if use_alt { TMDB_API_ALT } else { TMDB_API };
    let second = if use_alt { TMDB_API } else { TMDB_API_ALT };
    let mut err = String::new();
    for base in [first, second] {
        println!("Using {}",base);
        tokio::time::sleep(std::time::Duration::from_millis(TMDB_DELAY_MS)).await;
        match client.get(format!("{}{}", base, path)).bearer_auth(key).query(query).send().await {
            Ok(resp) => {
                USE_ALT.store(base == TMDB_API_ALT, Ordering::Relaxed);   // 记住这次连通的
                return if resp.status().is_success() {
                    resp.json::<Value>().await.map_err(|e| e.to_string())
                } else {
                    Err(format!("TMDB 返回 {}", resp.status().as_u16()))
                };
            }
            Err(e) => {
                err = if e.is_timeout() { "TMDB 请求超时".to_string() } else { format!("无法连接 TMDB：{}", e) };
            }
        }
    }
    Err(format!("{}（主站和镜像均不可达）", err))
}

// TMDB 图片相对路径（如 /abc.jpg）：库里只存这个，图床主机下载时再拼
fn img_path(v: &Value)->Option<String>{
    let p = v.as_str()?;
    if p.is_empty() { None } else { Some(p.to_string()) }
}

// 去掉 (...)、[...]、{...} 段（一般是发布组/编码信息）
fn strip_brackets(s: &str)->String{
    let mut out = String::with_capacity(s.len());
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => { depth += 1; out.push(' '); }
            ')' | ']' | '}' => { depth = depth.saturating_sub(1); }
            _ => out.push(if depth == 0 { c } else { ' ' }),
        }
    }
    out
}

// 文件名 → (标题猜测, 年份, (季,集))；无集号视为电影，裸集数默认第 1 季
fn parse_media_name(rel: &str)->(String, Option<i64>, Option<(u32,u32)>){
    static RE_SE: OnceLock<Regex> = OnceLock::new();     // S01E02
    static RE_X: OnceLock<Regex> = OnceLock::new();      // 1x02
    static RE_CN: OnceLock<Regex> = OnceLock::new();     // 第3季…第12集
    static RE_EP: OnceLock<Regex> = OnceLock::new();     // EP12 / E12 / 第12集
    static RE_BR: OnceLock<Regex> = OnceLock::new();     // [03] 方括号裸集数
    static RE_DASH: OnceLock<Regex> = OnceLock::new();   // " - 12"
    static RE_SEASON: OnceLock<Regex> = OnceLock::new(); // 裸季号 S2 / 第2季
    static RE_YEAR: OnceLock<Regex> = OnceLock::new();   // 年份
    let re_se = RE_SE.get_or_init(|| Regex::new(r"(?i)S(\d{1,2})[\s._-]*E(\d{1,3})").unwrap());
    let re_x  = RE_X.get_or_init(|| Regex::new(r"(?i)(?:^|[^0-9])(\d{1,2})x(\d{2,3})(?:$|[^0-9])").unwrap());
    let re_cn = RE_CN.get_or_init(|| Regex::new(r"第\s*(\d{1,2})\s*季.*?第\s*(\d{1,3})\s*[集话話]").unwrap());
    let re_ep = RE_EP.get_or_init(|| Regex::new(r"(?i)(?:^|[^0-9A-Za-z])(?:EP?|第)[\s._-]*(\d{1,3})[\s._-]*(?:[集话話])?").unwrap());
    let re_br = RE_BR.get_or_init(|| Regex::new(r"\[(\d{2,3})\]").unwrap());
    let re_dash = RE_DASH.get_or_init(|| Regex::new(r"-[\s._]*(\d{1,3})(?:$|[\s._\[(])").unwrap());
    let re_season = RE_SEASON.get_or_init(|| Regex::new(r"(?i)S(\d{1,2})|第\s*(\d{1,2})\s*季").unwrap());
    let re_year = RE_YEAR.get_or_init(|| Regex::new(r"(?:^|[^0-9])((?:19|20)\d{2})(?:$|[^0-9xp])").unwrap());

    let file = rel.rsplit('/').next().unwrap_or(rel);
    let stem = file.rsplit_once('.').map(|(s,_)| s).unwrap_or(file);
    // 季集号：标题取季集号之前的部分
    let (mut head, mut se) = if let Some(c) = re_se.captures(stem) {
        let m = c.get(0).unwrap();
        (stem[..m.start()].to_string(), Some((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0))))
    } else if let Some(c) = re_x.captures(stem) {
        let m = c.get(0).unwrap();
        (stem[..m.start()].to_string(), Some((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0))))
    } else if let Some(c) = re_cn.captures(stem) {
        let m = c.get(0).unwrap();
        (stem[..m.start()].to_string(), Some((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0))))
    } else {
        (stem.to_string(), None)
    };
    // 裸集数（EP12 / E12 / 第12集 / " - 12"）：季号单独找（S2 / 第2季），没有就默认第 1 季
    if se.is_none() {
        let ep = re_ep.captures(stem).map(|c| (c.get(0).unwrap().start(), c[1].parse::<u32>().unwrap_or(0)))
            .or_else(|| re_dash.captures(stem).map(|c| (c.get(0).unwrap().start(), c[1].parse::<u32>().unwrap_or(0))))
            .or_else(|| re_br.captures(stem).map(|c| (c.get(0).unwrap().start(), c[1].parse::<u32>().unwrap_or(0))));
        if let Some((pos, e)) = ep {
            let mut h = stem[..pos].to_string();
            let mut s = 1u32;
            if let Some(c) = re_season.captures(&h) {
                s = c.get(1).or(c.get(2)).map(|m| m.as_str().parse().unwrap_or(1)).unwrap_or(1);
                let m = c.get(0).unwrap();
                h.replace_range(m.start()..m.end(), " ");
            }
            se = Some((s, e));
            head = h;
        }
    }
    // 抽出年份作为搜索过滤
    let mut head = head;
    let hit = re_year.captures(&head).and_then(|c| c.get(1).map(|m| (m.start(), m.end())));
    let year = match hit {
        Some((s, e)) => {
            let y: Option<i64> = head[s..e].parse().ok();
            head.replace_range(s..e, " ");
            y
        }
        None => None,
    };
    // 去括号段、丢发布标签、合并空格
    let title = strip_brackets(&head)
        .split(|c: char| c == '.' || c == '_' || c == '-' || c.is_whitespace())
        .filter(|w| !w.is_empty() && !NAME_TAGS.iter().any(|t| t.eq_ignore_ascii_case(w)))
        .collect::<Vec<_>>().join(" ");
    (title, year, se)
}

// 把一集合并进 seasons JSON：按季分组、按集数排序，同一 file_id 的旧条目先移除
fn seasons_put(seasons: &mut Value, season: u32, file_id: i64, episode: u32, title: String, still: Option<String>, overview: Option<String>){
    let Some(arr) = seasons.as_array_mut() else { return };
    for g in arr.iter_mut() {
        if let Some(eps) = g["episodes"].as_array_mut() {
            eps.retain(|e| e["file_id"].as_i64() != Some(file_id));
        }
    }
    let idx = match arr.iter().position(|g| g["season"].as_i64() == Some(season as i64)) {
        Some(i) => i,
        None => { arr.push(json!({ "season": season, "episodes": [] })); arr.len() - 1 }
    };
    let Some(eps) = arr[idx]["episodes"].as_array_mut() else { return };
    eps.push(json!({ "file_id": file_id, "episode": episode, "title": title, "still": still, "overview": overview }));
    eps.sort_by_key(|e| e["episode"].as_i64().unwrap_or(0));
}

// 剧集分级：CN 优先，其次 US，否则任一非空
fn tv_certification(cr: &Value)->Option<String>{
    let arr = cr["results"].as_array()?;
    for cc in ["CN", "US"] {
        if let Some(s) = arr.iter().find(|r| r["iso_3166_1"].as_str() == Some(cc)).and_then(|r| r["rating"].as_str()) {
            return Some(s.to_string())
        }
    }
    arr.iter().find_map(|r| r["rating"].as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string())
}
// 电影分级：release_dates 里优先 CN/US 的 certification
fn movie_certification(rd: &Value)->Option<String>{
    let arr = rd["results"].as_array()?;
    for cc in ["CN", "US"] {
        let rds = arr.iter().find(|r| r["iso_3166_1"].as_str() == Some(cc))
            .and_then(|r| r["release_dates"].as_array()).map(|a| a.as_slice()).unwrap_or(&[]);
        if let Some(s) = rds.iter().find_map(|d| d["certification"].as_str()).filter(|s| !s.is_empty()) {
            return Some(s.to_string())
        }
    }
    arr.iter().flat_map(|r| r["release_dates"].as_array().map(|a| a.iter()).into_iter().flatten())
        .find_map(|d| d["certification"].as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string())
}
// 刮削前清理：删掉本文件源所有文件的旧刮削条目（改过名的文件换了 id，旧条目不会再被覆盖到），
//条目清空的作品行一并删除；其他文件源的条目不受影响
fn clear_scrape_of_source(db: &Connection, id: i64)->Result<(), String>{
    let src: HashSet<i64> = {
        let mut stmt = db.prepare("SELECT id FROM files WHERE lib_id = ?1").map_err(|e|e.to_string())?;
        let rows = stmt.query_map(params![id], |r| r.get(0)).map_err(|e|e.to_string())?;
        rows.collect::<Result<_,_>>().map_err(|e|e.to_string())?
    };
    let rows: Vec<(i64, String)> = {
        let mut stmt = db.prepare("SELECT id, seasons FROM works").map_err(|e|e.to_string())?;
        let rs = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e|e.to_string())?;
        rs.collect::<Result<_,_>>().map_err(|e|e.to_string())?
    };
    for (wid, old) in rows {
        let mut seasons: Value = serde_json::from_str(&old).unwrap_or_else(|_| json!([]));
        if let Some(arr) = seasons.as_array_mut() {
            for g in arr.iter_mut() {
                if let Some(eps) = g["episodes"].as_array_mut() {
                    eps.retain(|e| e["file_id"].as_i64().map(|f| !src.contains(&f)).unwrap_or(true));
                }
            }
            arr.retain(|g| g["episodes"].as_array().map(|a| !a.is_empty()).unwrap_or(true));
        }
        if seasons.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            db.execute("DELETE FROM works WHERE id = ?1", params![wid]).map_err(|e|e.to_string())?;  // 空壳作品删行
        } else {
            db.execute("UPDATE works SET seasons = ?1 WHERE id = ?2", params![seasons.to_string(), wid]).map_err(|e|e.to_string())?;
        }
    }
    Ok(())
}
// 清掉 works.seasons 里指向已删文件的分集
fn prune_works(db: &Connection)->Result<(), String>{
    let live: HashSet<i64> = {
        let mut stmt = db.prepare("SELECT id FROM files").map_err(|e|e.to_string())?;
        let rows = stmt.query_map([], |r| r.get(0)).map_err(|e|e.to_string())?;
        rows.collect::<Result<_,_>>().map_err(|e|e.to_string())?
    };
    let rows: Vec<(i64, String)> = {
        let mut stmt = db.prepare("SELECT id, seasons FROM works").map_err(|e|e.to_string())?;
        let rs = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e|e.to_string())?;
        rs.collect::<Result<_,_>>().map_err(|e|e.to_string())?
    };
    for (wid, old) in rows {
        let mut seasons: Value = serde_json::from_str(&old).unwrap_or_else(|_| json!([]));
        let mut changed = false;
        if let Some(arr) = seasons.as_array_mut() {
            for g in arr.iter_mut() {
                if let Some(eps) = g["episodes"].as_array_mut() {
                    let before = eps.len();
                    eps.retain(|e| e["file_id"].as_i64().map(|f| live.contains(&f)).unwrap_or(false));
                    if eps.len() != before { changed = true }
                }
            }
            let before = arr.len();
            arr.retain(|g| g["episodes"].as_array().map(|a| !a.is_empty()).unwrap_or(true));
            if arr.len() != before { changed = true }
        }
        if changed {
            if seasons.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                db.execute("DELETE FROM works WHERE id = ?1", params![wid]).map_err(|e|e.to_string())?;
            } else {
                db.execute("UPDATE works SET seasons = ?1 WHERE id = ?2", params![seasons.to_string(), wid]).map_err(|e|e.to_string())?;
            }
        }
    }
    Ok(())
}
// 匹配一部剧，拉作品/分集详情，返回 None 表示没搜到
async fn fetch_tv(client: &reqwest::Client, key: &str, title: &str, year: Option<i64>, files: &[(i64, Option<(u32,u32)>)])->Result<Option<WorkScrape>, String>{
    // 搜索（query 只有标题，季集号不进 query；带年份搜不到就去掉年份重搜）
    let mut q = vec![("query", title.to_string()), ("language", "zh-CN".to_string())];
    if let Some(y) = year { q.push(("first_air_date_year", y.to_string())); }
    let mut results = tmdb_get(client, key, "/search/tv", &q).await?.get("results").cloned().unwrap_or(Value::Null);
    if results.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        let q2 = vec![("query", title.to_string()), ("language", "zh-CN".to_string())];
        results = tmdb_get(client, key, "/search/tv", &q2).await?.get("results").cloned().unwrap_or(Value::Null);
    }
    let Some(hit) = results.get(0) else { return Ok(None) };
    let tv_id = hit["id"].as_i64().ok_or("TMDB 搜索结果缺少 id")?;

    // 电视剧详情
    let detail = tmdb_get(client, key, &format!("/tv/{}", tv_id), &[("language", "zh-CN".to_string())]).await?;
    let ratings = tmdb_get(client, key, &format!("/tv/{}/content_ratings", tv_id), &[]).await?;
    let name = detail["name"].as_str().unwrap_or(title).to_string();
    let poster = img_path(&detail["poster_path"]);
    let rating = detail["vote_average"].as_f64();
    let air_date = detail["first_air_date"].as_str().map(|s| s.to_string());
    let region = detail["origin_country"].get(0).and_then(|v| v.as_str()).map(|s| s.to_string());
    let total_seasons = detail["number_of_seasons"].as_i64();
    let total_episodes = detail["number_of_episodes"].as_i64();
    let certification = tv_certification(&ratings);
    // 演职人员（取前 14 位，credits 本身按 order 排好）
    let credits = tmdb_get(client, key, &format!("/tv/{}/credits", tv_id), &[("language", "zh-CN".to_string())]).await?;
    let cast: Value = credits["cast"].as_array()
        .map(|a| Value::Array(a.iter().filter_map(|c| {
            Some(json!({
                "name": c["name"].as_str()?,
                "character": c["character"].as_str().unwrap_or(""),
                "photo": img_path(&c["profile_path"]),
            }))
        }).collect()))
        .unwrap_or_else(|| json!([]));
    let overview = hit["overview"].as_str().map(|s| s.to_string());
    let backdrop = img_path(&hit["backdrop_path"]);
    let genres: Vec<String> = detail["genres"].as_array()
        .map(|a| a.iter().filter_map(|g| g["name"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // 当季详情：每个出现过的季拉一次，拿季名/季海报
    let mut season_map: HashMap<u32, (Option<String>, Option<String>)> = HashMap::new();
    for &(_, se) in files {
        let Some((season, _)) = se else { continue };
        if season_map.contains_key(&season) { continue }
        let meta = if let Ok(s) = tmdb_get(client, key, &format!("/tv/{}/season/{}", tv_id, season),
                                           &[("language", "zh-CN".to_string())]).await {
            (s["name"].as_str().map(|x| x.to_string()), img_path(&s["poster_path"]))
        } else { (None, None) };
        season_map.insert(season, meta);
    }
    // 单集详情：逐文件按 parse 的季/集拉，拿分集标题/封面
    let mut episodes = Vec::new();
    for &(fid, se) in files {
        let Some((season, ep)) = se else { continue };
        if let Ok(e) = tmdb_get(client, key, &format!("/tv/{}/season/{}/episode/{}", tv_id, season, ep),
                                &[("language", "zh-CN".to_string())]).await {
            episodes.push((fid, season, ep, e["name"].as_str().unwrap_or("").to_string(), img_path(&e["still_path"]),
                           e["overview"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())));
        }
    }
    if episodes.is_empty() { return Ok(None) }
    let season_meta: Vec<(u32, Option<String>, Option<String>)> =
        season_map.into_iter().map(|(sn, (name, poster))| (sn, name, poster)).collect();
    Ok(Some(WorkScrape{ tmdb_id: tv_id, kind: "tv", title: name, poster, rating, air_date, region,
        certification, total_seasons, total_episodes, overview, backdrop, genres, cast, episodes, season_meta }))
}

// 匹配一部电影；一文件一条分集，单集封面按需求留空
async fn fetch_movie(client: &reqwest::Client, key: &str, title: &str, year: Option<i64>, files: &[i64])->Result<Option<WorkScrape>, String>{
    let mut q = vec![("query", title.to_string()), ("language", "zh-CN".to_string())];
    if let Some(y) = year { q.push(("year", y.to_string())); }
    let mut results = tmdb_get(client, key, "/search/movie", &q).await?.get("results").cloned().unwrap_or(Value::Null);
    if results.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        let q2 = vec![("query", title.to_string()), ("language", "zh-CN".to_string())];
        results = tmdb_get(client, key, "/search/movie", &q2).await?.get("results").cloned().unwrap_or(Value::Null);
    }
    let Some(hit) = results.get(0) else { return Ok(None) };
    let movie_id = hit["id"].as_i64().ok_or("TMDB 搜索结果缺少 id")?;

    let detail = tmdb_get(client, key, &format!("/movie/{}", movie_id), &[("language", "zh-CN".to_string())]).await?;
    let dates = tmdb_get(client, key, &format!("/movie/{}/release_dates", movie_id), &[]).await?;
    let name = detail["title"].as_str().unwrap_or(title).to_string();
    let poster = img_path(&detail["poster_path"]);
    let rating = detail["vote_average"].as_f64();
    let air_date = detail["release_date"].as_str().map(|s| s.to_string());
    let region = detail["production_countries"].get(0).and_then(|c| c["iso_3166_1"].as_str()).map(|s| s.to_string());
    let certification = movie_certification(&dates);
    // 演职人员（取前 14 位，credits 本身按 order 排好）
    let credits = tmdb_get(client, key, &format!("/movie/{}/credits", movie_id), &[("language", "zh-CN".to_string())]).await?;
    let cast: Value = credits["cast"].as_array()
        .map(|a| Value::Array(a.iter().take(14).filter_map(|c| {
            Some(json!({
                "name": c["name"].as_str()?,
                "character": c["character"].as_str().unwrap_or(""),
                "photo": img_path(&c["profile_path"]),
            }))
        }).collect()))
        .unwrap_or_else(|| json!([]));
    let overview = hit["overview"].as_str().map(|s| s.to_string());
    let backdrop = img_path(&hit["backdrop_path"]);
    let genres: Vec<String> = detail["genres"].as_array()
        .map(|a| a.iter().filter_map(|g| g["name"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let episodes: Vec<(i64, u32, u32, String, Option<String>, Option<String>)> =
        files.iter().map(|&fid| (fid, 0u32, 0u32, name.clone(), None, None)).collect();   // 封面/简介留空
    Ok(Some(WorkScrape{ tmdb_id: movie_id, kind: "movie", title: name, poster, rating, air_date, region,
        certification, total_seasons: Some(0), total_episodes: Some(1), overview, backdrop, genres, cast,
        episodes, season_meta: Vec::new() }))
}
// 把一部作品合并进 works 表：读旧 seasons → 按 file_id 合并 → 写回
fn apply_work(db: &Connection, w: &WorkScrape)->Result<(), String>{
    let old: Option<String> = db
        .query_row("SELECT seasons FROM works WHERE tmdb_id = ?1 AND kind = ?2", params![w.tmdb_id, w.kind], |r| r.get(0))
        .optional().map_err(|e|e.to_string())?;
    let mut seasons: Value = old.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!([]));
    for (fid, season, ep, title, still, overview) in &w.episodes {
        seasons_put(&mut seasons, *season, *fid, *ep, title.clone(), still.clone(), overview.clone());
    }
    // 当季详情写进对应季分组
    if let Some(arr) = seasons.as_array_mut() {
        for (sn, name, poster) in &w.season_meta {
            if let Some(g) = arr.iter_mut().find(|g| g["season"].as_i64() == Some(*sn as i64)) {
                g["name"] = json!(name);
                g["poster"] = json!(poster);
            }
        }
    }
    if let Some(arr) = seasons.as_array_mut() {
        arr.retain(|g| g["episodes"].as_array().map(|a| !a.is_empty()).unwrap_or(true));
        arr.sort_by_key(|g| g["season"].as_i64().unwrap_or(0));
    }
    db.execute("INSERT INTO works
            (tmdb_id,kind,title,poster,rating,air_date,region,certification,total_seasons,total_episodes,overview,backdrop,genres,cast_json,seasons)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
            ON CONFLICT(tmdb_id, kind) DO UPDATE SET title=excluded.title, poster=excluded.poster, rating=excluded.rating,
                air_date=excluded.air_date, region=excluded.region, certification=excluded.certification,
                total_seasons=excluded.total_seasons, total_episodes=excluded.total_episodes,
                overview=excluded.overview, backdrop=excluded.backdrop, cast_json=excluded.cast_json, seasons=excluded.seasons",
               params![w.tmdb_id, w.kind, w.title, w.poster, w.rating, w.air_date, w.region, w.certification,
                w.total_seasons, w.total_episodes, w.overview, w.backdrop,json!(w.genres).to_string(), w.cast.to_string(), seasons.to_string()])
        .map_err(|e|e.to_string())?;
    Ok(())
}

// 同步：读源下视频文件，按 (标题小写, 是否剧集) 分组，减少 TMDB 搜索次数
fn load_scrape_groups(db: &Connection, id: i64)->Result<HashMap<(String, bool), Vec<(i64, Option<i64>, Option<(u32,u32)>)>>, String>{
    let mut stmt = db.prepare("SELECT id, rel_path FROM files WHERE lib_id = ?1 AND type = 'video'").map_err(|e|e.to_string())?;
    let rows = stmt.query_map(params![id], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
    let files: Vec<(i64, String)> = rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let mut groups = HashMap::new();
    for (fid, rel) in files {
        let (title, year, se) = parse_media_name(&rel);
        groups.entry((title.to_lowercase(), se.is_some())).or_insert_with(Vec::new).push((fid, year, se));
    }
    Ok(groups)
}

// 扫描完自动刮削整个文件源，返回 (作品数, 刮削成功文件数, 未匹配文件数)
// 分三段：同步读库 → 异步请求 TMDB → 同步写库（连接不跨 await）
// 没配 API Key 时直接跳过；刮削失败不影响扫描结果
async fn scrape_source_files(app: tauri::AppHandle, id: i64)->Result<(u32, u32, u32), String>{
    let (key, groups) = {
        let db = open_db(&app)?;
        let Ok(key) = get_tmdb_key(&db) else { return Ok((0, 0, 0)) };
        (key, load_scrape_groups(&db, id)?)
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().map_err(|e|e.to_string())?;
    let mut works = Vec::new();
    let (mut done, mut unmatched) = (0u32, 0u32);
    for ((title, is_tv), items) in groups {
        let year = items.iter().find_map(|(_, y, _)| *y);
        let fids: Vec<i64> = items.iter().map(|(f, _, _)| *f).collect();
        let r = if is_tv {
            let fs: Vec<(i64, Option<(u32,u32)>)> = items.iter().map(|(f, _, se)| (*f, *se)).collect();
            match fetch_tv(&client, &key, &title, year, &fs).await {
                Ok(None) => fetch_movie(&client, &key, &title, year, &fids).await,
                other => other,
            }
        } else {
            fetch_movie(&client, &key, &title, year, &fids).await
        };
        match r {
            Ok(Some(w)) => {
                let n = w.episodes.len() as u32;
                done += n;
                unmatched += items.len() as u32 - n;
                works.push(w);
            }
            Ok(None) => { unmatched += items.len() as u32; }   // 没搜到
            Err(e) => {                                        // 请求失败：打出来才好排查
                println!("[刮削失败] {}：{}", title, e);
                unmatched += items.len() as u32;
            }
        }
    }
    {
        let db = open_db(&app)?;
        clear_scrape_of_source(&db, id)?;
        for w in &works { apply_work(&db, w)?; }
    }
    // 刮削完构建图片缓存（失败不影响刮削结果）
    cache_images(app).await.unwrap_or_else(|e| { println!("[图片缓存] {}", e); 0 });
    Ok((works.len() as u32, done, unmatched))
}
// ==================== 图片缓存 ====================
// 上次成功连上的图床（false=TMDB_IMG，true=TMDB_IMG_ALT）
static IMG_USE_ALT: AtomicBool = AtomicBool::new(false);

// 缓存目录：和数据库同级的 img_cache/
fn img_cache_dir(app: &tauri::AppHandle)->std::path::PathBuf {
    app.path().app_data_dir().expect("配置目录不可用").join("img_cache")
}

// 收集 works 表里所有图片相对路径：poster / backdrop / 各季海报 / 各集封面
fn collect_image_paths(db: &Connection)->Result<HashSet<String>, String>{
    let mut paths = HashSet::new();
    let mut stmt = db.prepare("SELECT poster, backdrop, cast_json, seasons FROM works").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_,Option<String>>(0)?, r.get::<_,Option<String>>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?))).map_err(|e|e.to_string())?;
    for row in rows {
        let (poster, backdrop, cast, seasons) = row.map_err(|e|e.to_string())?;
        for p in poster.into_iter().chain(backdrop) { paths.insert(p); }
        let v: Value = serde_json::from_str(&seasons).unwrap_or_else(|_| json!([]));
        for g in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if let Some(p) = g["poster"].as_str() { paths.insert(p.to_string()); }
            for e in g["episodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                if let Some(p) = e["still"].as_str() { paths.insert(p.to_string()); }
            }
            let cv: Value = serde_json::from_str(&cast).unwrap_or_else(|_| json!([]));
            for c in cv.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                if let Some(p) = c["photo"].as_str() { paths.insert(p.to_string()); }
            }
        }
    }
    Ok(paths)
}

// 下载一张图：主图床失败自动换 TMDB_IMG_ALT，记住能连上的
async fn fetch_img(client: &reqwest::Client, path: &str)->Result<Vec<u8>, String>{
    let use_alt = IMG_USE_ALT.load(Ordering::Relaxed);
    let first = if use_alt { TMDB_IMG_ALT } else { TMDB_IMG };
    let second = if use_alt { TMDB_IMG } else { TMDB_IMG_ALT };
    let mut err = String::new();
    for base in [first, second] {
        println!("Using {}",base);
        tokio::time::sleep(std::time::Duration::from_millis(TMDB_DELAY_MS)).await;
        match client.get(format!("{}{}", base, path)).send().await {
            Ok(resp) if resp.status().is_success() => {
                IMG_USE_ALT.store(base == TMDB_IMG_ALT, Ordering::Relaxed);
                return resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string());
            }
            Ok(resp) => { err = format!("图床返回 {}", resp.status().as_u16()); }   // 失败也换个图床试试
            Err(e) => {
                err = if e.is_timeout() { "图片下载超时".to_string() } else { format!("无法连接图床：{}", e) };
            }
        }
    }
    Err(err)
}

// 刮削完把图片下到本地缓存；已缓存的跳过，单张失败不影响整体，返回成功张数
async fn cache_images(app: tauri::AppHandle)->Result<u32, String>{
    let (dir, paths) = {
        let db = open_db(&app)?;
        (img_cache_dir(&app), collect_image_paths(&db)?)
    };
    let _ = fs::create_dir_all(&dir);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build().map_err(|e|e.to_string())?;
    let mut ok = 0u32;
    for path in paths {
        // 相对路径形如 /abc.jpg，缓存文件名直接用 abc.jpg
        let Some(name) = path.rsplit('/').next().filter(|s| !s.is_empty()) else { continue };
        let file = dir.join(name);
        if file.exists() { ok += 1; continue }
        tokio::time::sleep(std::time::Duration::from_millis(TMDB_DELAY_MS)).await;
        if let Ok(bytes) = fetch_img(&client, &path).await {
            if fs::write(&file, &bytes).is_ok() { ok += 1 }
        }
    }
    Ok(ok)
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
    let mut stat = write_index(&mut db,id,&files)?;
    drop(db);   // 让连接在 await 前释放
    let (works, scraped, unmatched) = scrape_source_files(app, id).await.unwrap_or_else(|e| { println!("[刮削] {}", e); (0, 0, 0) });
    stat.works = works;
    stat.scraped = scraped;
    stat.unmatched = unmatched;
    Ok(stat)
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
        .timeout(std::time::Duration::from_secs(10))
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
    println!("index_start");
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
    let mut stat = write_index(&mut db, id, &out)?;
    println!("index_complete");
    drop(db);
    println!("scrape_start");
    let (works, scraped, unmatched) = scrape_source_files(app, id).await.unwrap_or((0, 0, 0));
    println!("scrape_complete");
    stat.works = works;
    stat.scraped = scraped;
    stat.unmatched = unmatched;
    Ok(stat)
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
// 读取设置
#[tauri::command]
fn get_setting(app: tauri::AppHandle, key: String)->Result<Option<String>, String>{
    let db = open_db(&app)?;
    let v:Option<String> = db.query_row("SELECT value FROM settings WHERE key = ?1",params![key],|r| r.get(0)).optional().map_err(|e|e.to_string())?;
    Ok(v)
}
// 写入设置
#[tauri::command]
fn set_setting(app: tauri::AppHandle, key: String, value: String)->Result<(), String>{
    let db = open_db(&app)?;
    db.execute("INSERT INTO settings (key,value) VALUES (?1,?2)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value",
               params![key, value]).map_err(|e|e.to_string())?;
    Ok(())
}
// 媒体库：已刮削作品（每部一行）
#[tauri::command]
fn list_works(app: tauri::AppHandle)->Result<Vec<WorkItem>, String>{
    let db = open_db(&app)?;
    let dir = img_cache_dir(&app);
    // 相对路径 → img_cache 绝对路径；没下到图返回 None
    let cached = |p: &Option<String>| -> Option<String> {
        p.as_ref().map(|p| dir.join(p.rsplit('/').next().unwrap_or("")))
            .filter(|f| f.exists() && f.is_file())
            .map(|f| f.to_string_lossy().to_string())
    };
    let mut stmt = db.prepare("SELECT tmdb_id, kind, title, poster, backdrop, rating, air_date, region, certification, overview, total_seasons, total_episodes, genres FROM works ORDER BY title").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([], |r| {
        let poster: Option<String> = r.get(3)?;
        let backdrop: Option<String> = r.get(4)?;
        Ok(WorkItem{
            tmdb_id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            poster: cached(&poster),
            backdrop: cached(&backdrop),
            rating: r.get(5)?,
            air_date: r.get(6)?,
            region: r.get(7)?,
            certification: r.get(8)?,
            overview: r.get(9)?,
            total_seasons: r.get(10)?,
            total_episodes: r.get(11)?,
            genres: serde_json::from_str(&r.get::<_,String>(12)?).unwrap_or_default(),
        })
    }).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
// 媒体库：未刮削的视频文件（file_id 不在 works.seasons 里）
#[tauri::command]
fn list_unscraped(app: tauri::AppHandle)->Result<Vec<UnscrapedItem>, String>{
    let db = open_db(&app)?;
    let mut scraped: HashSet<i64> = HashSet::new();
    {
        let mut stmt = db.prepare("SELECT seasons FROM works").map_err(|e|e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_,String>(0)).map_err(|e|e.to_string())?;
        for row in rows {
            let v: Value = serde_json::from_str(&row.map_err(|e|e.to_string())?).unwrap_or_else(|_| json!([]));
            for g in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                for e in g["episodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
                    if let Some(fid) = e["file_id"].as_i64() { scraped.insert(fid); }
                }
            }
        }
    }
    let mut stmt = db.prepare("SELECT id, rel_path FROM files WHERE type = 'video' ORDER BY rel_path").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (fid, rel) = row.map_err(|e|e.to_string())?;
        if scraped.contains(&fid) { continue }
        let file = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        let name = file.rsplit_once('.').map(|(s,_)| s.to_string()).unwrap_or(file);
        out.push(UnscrapedItem{ name, rel_path: rel });
    }
    Ok(out)
}
// 作品详情：作品字段 + 分集（seasons JSON 扁平化，补文件大小）+ 演职人员
#[tauri::command]
fn get_work(app: tauri::AppHandle, tmdb: i64, kind: String)->Result<WorkDetail, String>{
    let db = open_db(&app)?;
    let dir = img_cache_dir(&app);
    let cached = |p: &Option<String>| -> Option<String> {
        p.as_ref().map(|p| dir.join(p.rsplit('/').next().unwrap_or("")))
            .filter(|f| f.exists() && f.is_file())
            .map(|f| f.to_string_lossy().to_string())
    };
    let row: (String, Option<String>, Option<String>, Option<f64>, Option<String>, Option<String>,
              Option<String>, Option<String>, Option<i64>, Option<i64>, String, String, String) = db.query_row(
        "SELECT title, poster, backdrop, rating, air_date, region, certification, overview, total_seasons, total_episodes, genres, cast_json, seasons FROM works WHERE tmdb_id = ?1 AND kind = ?2",
        params![tmdb, kind],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?)),
    ).map_err(|e|e.to_string())?;
    let (title, poster, backdrop, rating, air_date, region, certification, overview,
        total_seasons, total_episodes, genres, cast, seasons) = row;

    // 文件大小表（算总量/分集大小）
    let mut sizes: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = db.prepare("SELECT id, size FROM files").map_err(|e|e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?))).map_err(|e|e.to_string())?;
        for row in rows { let (i, s) = row.map_err(|e|e.to_string())?; sizes.insert(i, s); }
    }
    // seasons JSON → 扁平分集列表
    let v: Value = serde_json::from_str(&seasons).unwrap_or_else(|_| json!([]));
    let mut episodes = Vec::new();
    let mut total_size = 0i64;
    for g in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        let season = g["season"].as_i64().unwrap_or(0) as u32;
        for e in g["episodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let file_id = e["file_id"].as_i64().unwrap_or(0);
            let size = sizes.get(&file_id).copied().unwrap_or(0);
            total_size += size;
            episodes.push(EpisodeItem{
                file_id,
                season,
                episode: e["episode"].as_i64().unwrap_or(0) as u32,
                title: e["title"].as_str().unwrap_or("").to_string(),
                still: cached(&e["still"].as_str().map(|s| s.to_string())),
                size,
            });
        }
    }
    episodes.sort_by_key(|e| (e.season, e.episode));
    // 演职人员
    let cv: Value = serde_json::from_str(&cast).unwrap_or_else(|_| json!([]));
    let cast: Vec<CastItem> = cv.as_array().map(|a| a.iter().map(|c| CastItem{
        name: c["name"].as_str().unwrap_or("").to_string(),
        character: c["character"].as_str().unwrap_or("").to_string(),
        photo: cached(&c["photo"].as_str().map(|s| s.to_string())),
    }).collect()).unwrap_or_default();
    Ok(WorkDetail{
        tmdb_id: tmdb, kind, title,
        poster: cached(&poster), backdrop: cached(&backdrop),
        rating, air_date, region, certification, overview,
        total_seasons, total_episodes,
        genres: serde_json::from_str(&genres).unwrap_or_default(),
        cast, total_size, episodes,
    })
}

// ==================== 播放 ====================
// 字幕语言后缀 → 显示名（chs&eng 这类多标记也认）；认不出用文件名
fn subtitle_label(sfx: &str, file: &str)->String{
    let mut parts: Vec<&str> = Vec::new();
    for t in sfx.trim_start_matches('.').split(['.', '&', '+', '_', ' ']).filter(|t| !t.is_empty()) {
        let s = match t.to_lowercase().as_str() {
            "zh" | "chs" | "sc" | "gb" | "zh-cn" | "zh-hans" | "简" | "简体" | "简中" | "中字" => "简体中文",
            "cht" | "tc" | "big5" | "zh-tw" | "zh-hant" | "繁" | "繁体" | "繁中" => "繁體中文",
            "chi" | "zho" => "中文",
            "en" | "eng" | "英文" => "English",
            "ja" | "jp" | "jpn" | "日" | "日文" => "日本語",
            "ko" | "kr" | "kor" | "韩" | "韩文" => "한국어",
            "fr" | "fre" | "fra" | "法" | "法文" => "Français",
            "de" | "ger" | "deu" | "德" | "德文" => "Deutsch",
            "ru" | "rus" | "俄" | "俄文" => "Русский",
            "default" | "forced" | "sdh" | "cc" | "comment" => continue,
            _ => t,
        };
        if !parts.contains(&s) { parts.push(s); }
    }
    if parts.is_empty() { file.to_string() } else { parts.join("&") }
}
// 内嵌轨标签：语言码（ISO 639-2）+ 轨名
fn mkv_subtitle_label(lang: &str, name: &str) -> String {
    let mut sfx = String::new();
    if !lang.is_empty() && lang != "und" { sfx.push('.'); sfx.push_str(lang); }
    if !name.is_empty() { sfx.push('.'); sfx.push_str(&name.replace([' ', '/'], ".")); }
    subtitle_label(&sfx, if name.is_empty() { "内嵌字幕" } else { name })
}
// 同目录同名词幕：ep01.mkv 配 ep01.srt / ep01.zh.ass / ep01.chs&eng.srt；sub/idx 位图不收
fn find_subtitles(db: &Connection, lib_id: i64, video_rel: &str)->Result<Vec<SubtitleItem>, String>{
    let (dir, file) = video_rel.rsplit_once('/').unwrap_or(("", video_rel));
    let stem = file.rsplit_once('.').map(|(s,_)| s).unwrap_or(file);
    let mut stmt = db.prepare("SELECT id, rel_path FROM files WHERE lib_id = ?1 AND type = 'subtitle' ORDER BY rel_path").map_err(|e|e.to_string())?;
    let rows = stmt.query_map(params![lib_id], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (fid, rel) = row.map_err(|e|e.to_string())?;
        let (sdir, sfile) = rel.rsplit_once('/').unwrap_or(("", &rel));
        if sdir != dir { continue }
        let Some((sname, sext)) = sfile.rsplit_once('.') else { continue };
        let ext = sext.to_lowercase();
        if !["srt","vtt","ass","ssa"].contains(&ext.as_str()) { continue }
        let Some(sfx) = sname.strip_prefix(stem) else { continue };
        if !sfx.is_empty() && !sfx.starts_with('.') { continue }
        out.push(SubtitleItem{ file_id: fid, track: None, label: subtitle_label(sfx, sfile), format: ext });
    }
    Ok(out)
}
// 字幕文本解码：UTF-8 优先（去 BOM），失败按 GBK 兼容（老 SRT 常见）
fn decode_text(bytes: &[u8])->String{
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim_start_matches('\u{feff}').to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}
// 内嵌字幕文本缓存：{file_id}_{size}.json → {轨号: 文本}，提取过就不再通读容器
fn sub_cache_path(app: &tauri::AppHandle, file_id: i64, size: i64) -> std::path::PathBuf {
    app.path().app_data_dir().expect("配置目录不可用").join("sub_cache").join(format!("{}_{}.json", file_id, size))
}
fn cache_get(app: &tauri::AppHandle, file_id: i64, size: i64, track: u32) -> Option<String> {
    let raw = fs::read_to_string(sub_cache_path(app, file_id, size)).ok()?;
    serde_json::from_str::<HashMap<u32, String>>(&raw).ok()?.get(&track).cloned()
}
fn cache_put(app: &tauri::AppHandle, file_id: i64, size: i64, texts: &HashMap<u32, String>) {
    let p = sub_cache_path(app, file_id, size);
    if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
    // 合并写：单轨缓存别把其他轨盖掉
    let mut map = fs::read_to_string(&p).ok()
        .and_then(|s| serde_json::from_str::<HashMap<u32, String>>(&s).ok())
        .unwrap_or_default();
    for (k, v) in texts { map.insert(*k, v.clone()); }
    if let Ok(s) = serde_json::to_string(&map) { let _ = fs::write(p, s); }
}
// 按文件 id 查 size（缓存键要用）
fn file_size(app: &tauri::AppHandle, file_id: i64) -> Result<i64, String> {
    let db = open_db(app)?;
    db.query_row("SELECT size FROM files WHERE id = ?1", params![file_id], |r| r.get(0))
        .optional().map_err(|e| e.to_string())?
        .ok_or_else(|| "文件不存在".to_string())
}
// PGS .sup 缓存：{file_id}_{size}_{track}.sup 原始字节
fn bin_cache_path(app: &tauri::AppHandle, file_id: i64, size: i64, track: u32) -> std::path::PathBuf {
    app.path().app_data_dir().expect("配置目录不可用").join("sub_cache").join(format!("{}_{}_{}.sup", file_id, size, track))
}
fn bin_cache_get(app: &tauri::AppHandle, file_id: i64, size: i64, track: u32) -> Option<Vec<u8>> {
    fs::read(bin_cache_path(app, file_id, size, track)).ok()
}
fn bin_cache_put(app: &tauri::AppHandle, file_id: i64, size: i64, track: u32, data: &[u8]) {
    let p = bin_cache_path(app, file_id, size, track);
    if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
    let _ = fs::write(p, data);
}
// 系统中文字体文件（给 ASS 渲染用）
#[derive(Serialize)]
struct FontFile{
    path: String,
    family: String,   // 字体族名（libass 默认字体）
}
// 系统中文字体候选：只返回路径，字节由前端经 asset protocol 读取
#[tauri::command]
fn get_cjk_fonts()->Result<Vec<FontFile>, String>{
    let windir = std::env::var("windir").unwrap_or_else(|_| "C:\\Windows".into());
    let candidates: Vec<(String, &str)> = vec![
        // Windows：雅黑（简）→ 黑体（兜底单文件 TTF）→ 正黑（繁）→ 宋体
        (format!("{}\\Fonts\\msyh.ttc", windir), "Microsoft YaHei"),
        (format!("{}\\Fonts\\simhei.ttf", windir), "SimHei"),
        (format!("{}\\Fonts\\msjh.ttc", windir), "Microsoft JhengHei"),
        (format!("{}\\Fonts\\simsun.ttc", windir), "SimSun"),
        // macOS
        ("/System/Library/Fonts/PingFang.ttc".into(), "PingFang SC"),
        ("/System/Library/Fonts/STHeiti Light.ttc".into(), "Heiti SC"),
        // Linux
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(), "Noto Sans CJK SC"),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".into(), "WenQuanYi Micro Hei"),
    ];
    let out: Vec<FontFile> = candidates.into_iter()
        .filter(|(p, _)| std::path::Path::new(p).is_file())
        .take(2)   // 一个主力 + 一个兜底足够（TTF 兜底防 .ttc 解析失败）
        .map(|(path, family)| FontFile{ path, family: family.to_string() })
        .collect();
    if out.is_empty() { Err("未找到系统中文字体".into()) } else { Ok(out) }
}
// 播放信息：定位播放源 + 同名词幕列表（外挂 + mkv 内嵌）
#[tauri::command]
fn get_playback_info(app: tauri::AppHandle, file_id: i64)->Result<PlaybackInfo, String>{
    let db = open_db(&app)?;
    let row: Option<(String, i64, i64, i32, String, Option<String>, Option<String>)> = db.query_row(
        "SELECT f.rel_path, f.lib_id, f.size, s.source, s.data_a, s.data_e, s.data_f FROM files f JOIN sources s ON s.id = f.lib_id WHERE f.id = ?1",
        params![file_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    ).optional().map_err(|e|e.to_string())?;
    let Some((rel, lib_id, size, source, data_a, user, pass)) = row else { return Err("文件不存在或已被删除".into()) };
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let mut subtitles = find_subtitles(&db, lib_id, &rel)?;
    // mkv 内嵌字幕轨：只读到 Tracks 就停，不扫全文件
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "mkv" | "mka" | "webm") {
        let res = match source {
            0 => mkv::parse_local(&std::path::Path::new(&data_a).join(&rel).to_string_lossy(), false),
            1 => {
                let client = reqwest::blocking::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(10))
                    .build().map_err(|e|e.to_string())?;
                let mut req = client.get(webdav_url(&data_a, &rel)).header("Range", "bytes=0-1048575");
                if !user.as_deref().unwrap_or("").is_empty() {
                    req = req.basic_auth(user.unwrap_or_default(), Some(pass.unwrap_or_default()));
                }
                let resp = req.send().map_err(|e|e.to_string())?;
                // 206 截断或 200 全流都行：读到 Tracks 即停，然后 drop 响应
                if !resp.status().is_success() { Err(format!("列轨失败（HTTP {}）", resp.status().as_u16())) }
                else { Ok(mkv::parse_stream(resp, false)) }
            }
            _ => Err("不支持的文件源类型".into()),
        };
        match res {
            Ok(p) => for t in p.tracks.into_iter().filter(|t| t.is_text() || t.is_pgs()) {
                subtitles.push(SubtitleItem{
                    file_id,
                    track: Some(t.number),
                    label: mkv_subtitle_label(&t.lang, &t.name),
                    format: t.format().to_string(),
                });
            },
            Err(e) => println!("内嵌字幕列轨失败 {}: {}", rel, e),   // 失败不拦播放
        }
    }
    let (src_kind, src) = match source {
        0 => ("local", std::path::Path::new(&data_a).join(&rel).to_string_lossy().to_string()),
        1 => ("webdav", format!("http://127.0.0.1:{}/dav/{}", proxy::port(), file_id)),
        _ => return Err("不支持的文件源类型".into()),
    };
    Ok(PlaybackInfo{ file_id, name, src_kind: src_kind.to_string(), src, size, subtitles })
}
// 读取字幕原文：本地直接读文件，WebDAV 走 GET 整读
#[tauri::command]
async fn get_subtitle_text(app: tauri::AppHandle, file_id: i64, track: Option<u32>)->Result<String, String>{
    let row: Option<(String, i64, i32, String, Option<String>, Option<String>)> = {
        let db = open_db(&app)?;
        db.query_row(
            "SELECT f.rel_path, f.size, s.source, s.data_a, s.data_e, s.data_f FROM files f JOIN sources s ON s.id = f.lib_id WHERE f.id = ?1",
            params![file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).optional().map_err(|e|e.to_string())?
    };
    let Some((rel, size, source, data_a, user, pass)) = row else { return Err("文件不存在或已被删除".into()) };
    // 内嵌轨：提取（WebDAV 要通读容器，放 blocking 线程里跑）
    if let Some(n) = track {
        if let Some(t) = cache_get(&app, file_id, size, n) { return Ok(t) }
        let parsed: Result<mkv::Parse, String> = match source {
            0 => {
                let abs = std::path::Path::new(&data_a).join(&rel);
                tokio::task::spawn_blocking(move || mkv::parse_local(&abs.to_string_lossy(), true))
                    .await.map_err(|e| e.to_string())?
            }
            1 => {
                let (url, user, pass) = (webdav_url(&data_a, &rel), user.unwrap_or_default(), pass.unwrap_or_default());
                tokio::task::spawn_blocking(move || {
                    let client = reqwest::blocking::Client::builder()
                        .connect_timeout(std::time::Duration::from_secs(10))
                        .timeout(std::time::Duration::from_secs(600))
                        .build().map_err(|e| e.to_string())?;
                    let mut req = client.get(url).header("Accept-Encoding", "identity");
                    if !user.is_empty() { req = req.basic_auth(user, Some(pass)); }
                    let resp = req.send().map_err(|e| e.to_string())?;
                    if !resp.status().is_success() {
                        return Err(format!("下载失败（HTTP {}）", resp.status().as_u16()));
                    }
                    Ok(mkv::parse_stream(resp, true))
                }).await.map_err(|e| e.to_string())?
            }
            _ => Err("不支持的文件源类型".into()),
        };
        let texts = parsed?.build_texts();   // 一次提取存下全部文本轨
        cache_put(&app, file_id, size, &texts);
        return texts.get(&n).cloned().ok_or_else(|| "内嵌轨不存在或不是文本字幕".to_string());
    }
    match source {
        0 => {
            if let Some(t) = cache_get(&app, file_id, size, 0) { return Ok(t) }   // 外挂字幕约定 track=0
            let abs = std::path::Path::new(&data_a).join(&rel);
            let text = decode_text(&fs::read(&abs).map_err(|e|e.to_string())?);
            cache_put(&app, file_id, size, &HashMap::from([(0, text.clone())]));
            Ok(text)
        }
        1 => {
            if let Some(t) = cache_get(&app, file_id, size, 0) { return Ok(t) }
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build().map_err(|e|e.to_string())?;
            let mut req = client.get(webdav_url(&data_a, &rel));
            if !user.as_deref().unwrap_or("").is_empty() {
                req = req.basic_auth(user.unwrap_or_default(), Some(pass.unwrap_or_default()));
            }
            let resp = req.send().await.map_err(|e|e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("字幕下载失败（HTTP {}）", resp.status().as_u16()));
            }
            let text = decode_text(&resp.bytes().await.map_err(|e|e.to_string())?);
            cache_put(&app, file_id, size, &HashMap::from([(0, text.clone())]));
            Ok(text)
        }
        _ => Err("不支持的文件源类型".into()),
    }
}
// PGS 位图字幕：整轨提取拼成 .sup 流，base64 返回（前端 libpgs.loadFromBuffer 直接用）
#[tauri::command]
async fn get_subtitle_bin(app: tauri::AppHandle, file_id: i64, track: u32) -> Result<String, String> {
    let row: Option<(String, i64, i32, String, Option<String>, Option<String>)> = {
        let db = open_db(&app)?;
        db.query_row(
            "SELECT f.rel_path, f.size, s.source, s.data_a, s.data_e, s.data_f FROM files f JOIN sources s ON s.id = f.lib_id WHERE f.id = ?1",
            params![file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).optional().map_err(|e|e.to_string())?
    };
    let Some((rel, size, source, data_a, user, pass)) = row else { return Err("文件不存在或已被删除".into()) };
    if let Some(raw) = bin_cache_get(&app, file_id, size, track) {
        return Ok(base64::engine::general_purpose::STANDARD.encode(&raw));
    }
    // 整轨提取（WebDAV 要通读容器，放 blocking 线程里跑）
    let parsed: Result<mkv::Parse, String> = match source {
        0 => {
            let abs = std::path::Path::new(&data_a).join(&rel);
            tokio::task::spawn_blocking(move || mkv::parse_local(&abs.to_string_lossy(), true))
                .await.map_err(|e| e.to_string())?
        }
        1 => {
            let (url, user, pass) = (webdav_url(&data_a, &rel), user.unwrap_or_default(), pass.unwrap_or_default());
            tokio::task::spawn_blocking(move || {
                let client = reqwest::blocking::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(10))
                    .timeout(std::time::Duration::from_secs(600))
                    .build().map_err(|e| e.to_string())?;
                let mut req = client.get(url).header("Accept-Encoding", "identity");
                if !user.is_empty() { req = req.basic_auth(user, Some(pass)); }
                let resp = req.send().map_err(|e| e.to_string())?;
                if !resp.status().is_success() {
                    return Err(format!("下载失败（HTTP {}）", resp.status().as_u16()));
                }
                Ok(mkv::parse_stream(resp, true))
            }).await.map_err(|e| e.to_string())?
        }
        _ => Err("不支持的文件源类型".into()),
    };
    let sup = parsed?.build_sup(track);
    if sup.is_empty() { return Err("内嵌轨不存在或没有 PGS 数据".into()) }
    bin_cache_put(&app, file_id, size, track, &sup);
    Ok(base64::engine::general_purpose::STANDARD.encode(&sup))
}
// 字幕缓存读写：前端 Range 提取的结果也落盘，重复播放秒开
#[tauri::command]
fn peek_sub_cache(app: tauri::AppHandle, file_id: i64, track: u32) -> Result<Option<String>, String> {
    let size = file_size(&app, file_id)?;
    Ok(cache_get(&app, file_id, size, track))
}
#[tauri::command]
fn save_sub_cache(app: tauri::AppHandle, file_id: i64, texts: HashMap<u32, String>) -> Result<(), String> {
    let size = file_size(&app, file_id)?;
    cache_put(&app, file_id, size, &texts);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // WebDAV 播放代理：失败不影响其他功能，在线播放会不可用
            match proxy::start(app.handle().clone()) {
                Ok(port) => println!("播放代理监听 127.0.0.1:{}", port),
                Err(e) => println!("[播放代理] 启动失败：{}", e),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![add_local_source, list_sources, delete_source, index_local_source, add_webdav_source, test_webdav_source, index_webdav_source, list_files, get_setting, set_setting, list_works, list_unscraped, get_work, get_playback_info, get_subtitle_text, get_subtitle_bin, peek_sub_cache, save_sub_cache, get_cjk_fonts])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
