// WebDAV 播放代理：GET /dav/{file_id} 转发到 WebDAV 服务器，透传 Range 让播放器能拖进度
// 只监听 127.0.0.1，认证信息不出后端
use rusqlite::{params, OptionalExtension};
use std::sync::OnceLock;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static PORT: OnceLock<u16> = OnceLock::new();
static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();   // ← 新增

// 上游客户端全局复用：连接池 keep-alive，Range 小请求不再每次重新握手
fn client() -> &'static reqwest::blocking::Client {
    CLIENT.get_or_init(|| reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build().expect("构建代理客户端失败"))
}
// 代理端口（启动后可取，未启动为 0）
pub fn port() -> u16 { PORT.get().copied().unwrap_or(0) }

// 起代理线程，返回监听端口
pub fn start(app: tauri::AppHandle) -> Result<u16, String> {
    let _ = APP.set(app);
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = server.server_addr().to_ip().ok_or("无法获取代理端口")?.port();
    let _ = PORT.set(port);
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            std::thread::spawn(move || handle(req));   // ← 每请求一线程，让前端并发真正并发
        }
    });
    Ok(port)
}

// 文件 id → (WebDAV 文件 URL, 用户名, 密码, 扩展名)；只有 WebDAV 源才返回
fn target(file_id: i64) -> Result<(String, String, String, String), String> {
    let app = APP.get().ok_or("播放代理未初始化")?;
    let db = super::open_db(app)?;
    let row: Option<(String, i32, String, Option<String>, Option<String>)> = db.query_row(
        "SELECT f.rel_path, s.source, s.data_a, s.data_e, s.data_f FROM files f JOIN sources s ON s.id = f.lib_id WHERE f.id = ?1",
        params![file_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).optional().map_err(|e| e.to_string())?;
    let Some((rel, source, root, user, pass)) = row else { return Err("文件不存在".into()) };
    if source != 1 { return Err("不是 WebDAV 文件".into()) }
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    Ok((super::webdav_url(&root, &rel), user.unwrap_or_default(), pass.unwrap_or_default(), ext))
}

// 扩展名兜底 Content-Type（上游没给时才用）
fn guess_type(ext: &str) -> &'static str {
    match ext {
        "mp4" | "m4v" | "mov" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ts" => "video/mp2t",
        "avi" => "video/x-msvideo",
        "srt" => "application/x-subrip",
        "vtt" => "text/vtt",
        _ => "application/octet-stream",
    }
}

fn handle(req: tiny_http::Request) {
    let reply = |req: tiny_http::Request, code: u16, msg: &str| {
        let _ = req.respond(tiny_http::Response::from_string(msg).with_status_code(code));
    };
    // CORS 预检：前端 fetch 带 Range 头需要它
    if req.url().starts_with("/dav/") && req.method().eq(&tiny_http::Method::Options) {
        let mut resp = tiny_http::Response::empty(204);
        for (name, value) in [
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"),
            ("Access-Control-Allow-Headers", "Range, Accept, Accept-Encoding"),
            ("Access-Control-Max-Age", "86400"),
        ] {
            if let Ok(h) = tiny_http::Header::from_bytes(name, value) { resp.add_header(h); }
        }
        let _ = req.respond(resp);
        return;
    }
    if req.url().starts_with("/dav/") && req.method().eq(&tiny_http::Method::Get) {
        let id = &req.url()["/dav/".len()..];
        let id = id.split('/').next().unwrap_or("");
        if let Ok(file_id) = id.parse::<i64>() {
            let (url, user, pass, ext) = match target(file_id) {
                Ok(t) => t,
                Err(e) => return reply(req, 404, &e),
            };
            // 上游 GET：只透传 Range，其余请求头不带
            // 上游 GET：只透传 Range，其余请求头不带
            let mut rb = client().get(&url).header("Accept-Encoding", "identity");
            if !user.is_empty() { rb = rb.basic_auth(&user, Some(&pass)); }
            if let Some(range) = req.headers().iter().find(|h| h.field.equiv("Range")) {
                rb = rb.header("Range", range.value.as_str().to_owned());
            }
            let resp = match rb.send() {
                Ok(r) => r,
                Err(e) => return reply(req, 502, &format!("无法连接 WebDAV：{}", e)),
            };
            // 回传上游状态码和关键头，body 流式转发
            let status = tiny_http::StatusCode(resp.status().as_u16());
            let len = resp.content_length().map(|l| l as usize);
            let mut headers = Vec::new();
            let ctype = resp.headers().get("Content-Type")
                .map(|v| v.as_bytes().to_vec())
                .unwrap_or_else(|| guess_type(&ext).as_bytes().to_vec());
            for (name, value) in [
                ("Access-Control-Allow-Origin", b"*".to_vec()),
                ("Access-Control-Expose-Headers", b"Content-Range, Accept-Ranges".to_vec()),   // ← 新增：不暴露 JS 就读不到 Content-Range
                ("Content-Type", ctype),
                ("Content-Range", resp.headers().get("Content-Range").map(|v| v.as_bytes().to_vec()).unwrap_or_default()),
                ("Accept-Ranges", resp.headers().get("Accept-Ranges").map(|v| v.as_bytes().to_vec()).unwrap_or_default()),
            ] {
                if !value.is_empty() {
                    if let Ok(h) = tiny_http::Header::from_bytes(name, value) { headers.push(h); }
                }
            }
            let _ = req.respond(tiny_http::Response::new(status, headers, resp, len, None));
            return;
        }
    }
    reply(req, 404, "not found");
}
