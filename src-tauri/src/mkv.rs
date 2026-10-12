// MKV/WebM 内嵌字幕提取：只读遍历 EBML
// 阶段1 full=false：读到 Tracks 就停（列字幕菜单）；阶段2 full=true：通读收字幕块
// 本地 seek 跳过音视频块 payload，网络流读丢；中途读错「能拿多少算多少」，不拦播放

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub type Extracted = HashMap<u32, String>;

// 字幕轨信息
pub struct TrackInfo {
    pub number: u32,
    pub codec: String,      // CodecID 原样，如 S_TEXT/ASS
    pub lang: String,
    pub name: String,
    pub private: Vec<u8>,   // ASS 的 Script Info + Styles 头
    pub comp: u8,           // ContentCompAlgo：0=zlib 3=去头 0xFF=无压缩
    pub comp_settings: Vec<u8>,   // 去头压缩时被剥掉的字节
}
impl TrackInfo {
    pub fn is_text(&self) -> bool {
        matches!(self.codec.as_str(), "S_TEXT/UTF8" | "S_TEXT/ASS" | "S_TEXT/SSA")
    }
    pub fn format(&self) -> &'static str {
        match self.codec.as_str() {
            "S_TEXT/ASS" => "ass",
            "S_TEXT/SSA" => "ssa",
            "S_HDMV/PGS" => "pgs",
            _ => "srt",
        }
    }
    pub fn is_pgs(&self) -> bool { self.codec == "S_HDMV/PGS" }
}

// 解析结果：轨列表 + 字幕块（轨号, 起ms, 止ms（None=容器没给）, 包内容）
#[derive(Default)]
pub struct Parse {
    pub tracks: Vec<TrackInfo>,
    pub events: Vec<(u32, i64, Option<u64>, Vec<u8>)>,
}

// ---------- IO 抽象：本地跳读 / 流式读丢 ----------
trait MkvIo {
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()>;
    fn skip(&mut self, n: u64) -> std::io::Result<()>;
}
struct LocalIo(File);
impl MkvIo for LocalIo {
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> { Read::read_exact(&mut self.0, buf) }
    fn skip(&mut self, n: u64) -> std::io::Result<()> { self.0.seek(SeekFrom::Current(n as i64)).map(|_| ()) }
}
struct StreamIo<R: Read> { r: R }
impl<R: Read> MkvIo for StreamIo<R> {
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> { Read::read_exact(&mut self.r, buf) }
    fn skip(&mut self, n: u64) -> std::io::Result<()> {
        let copied = std::io::copy(&mut self.r.by_ref().take(n), &mut std::io::sink())?;
        if copied < n { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof")); }
        Ok(())
    }
}

// ---------- EBML 基元 ----------
// 元素 ID 常量（含长度标记的原始值）
const ID_EBML: u64 = 0x1A45DFA3;
const ID_SEGMENT: u64 = 0x18538067;
const ID_INFO: u64 = 0x1549A966;
const ID_TIMECODE_SCALE: u64 = 0x2AD7B1;
const ID_TRACKS: u64 = 0x1654AE6B;
const ID_TRACK_ENTRY: u64 = 0xAE;
const ID_TRACK_NUMBER: u64 = 0xD7;
const ID_TRACK_TYPE: u64 = 0x83;
const ID_CODEC_ID: u64 = 0x86;
const ID_CODEC_PRIVATE: u64 = 0x63A2;
const ID_LANGUAGE: u64 = 0x22B59C;
const ID_NAME: u64 = 0x536E;
const ID_CONTENT_ENCODINGS: u64 = 0x6D80;
const ID_CONTENT_ENCODING: u64 = 0x6240;
const ID_CONTENT_COMPRESSION: u64 = 0x5034;
const ID_CONTENT_COMP_ALGO: u64 = 0x4254;
const ID_CONTENT_COMP_SETTINGS: u64 = 0x4255;
const ID_CLUSTER: u64 = 0x1F43B675;
const ID_TIMECODE: u64 = 0xE7;
const ID_SIMPLE_BLOCK: u64 = 0xA3;
const ID_BLOCK_GROUP: u64 = 0xA0;
const ID_BLOCK: u64 = 0xA1;
const ID_BLOCK_DURATION: u64 = 0x9B;

fn vint_len(first: u8) -> usize {
    let mut len = 1; let mut mask = 0x80u8;
    while len <= 8 && first & mask == 0 { mask >>= 1; len += 1; }
    len
}
// 变长整数：keep_marker=true 给元素 ID（保留标记位），false 给 size/轨号
fn read_vint(r: &mut dyn MkvIo, keep_marker: bool) -> std::io::Result<(u64, usize)> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let len = vint_len(b[0]);
    if len > 8 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad vint")); }
    let mut buf = [0u8; 8];
    buf[0] = b[0];
    if len > 1 { r.read_exact(&mut buf[1..len])?; }
    let marker = 0x80u8 >> (len - 1);
    let mut val = if keep_marker { buf[0] as u64 } else { (buf[0] & (marker - 1)) as u64 };
    for i in 1..len { val = (val << 8) | buf[i] as u64; }
    Ok((val, len))
}
// 内存版变长整数（Block 头预读用）
fn vint_mem(buf: &[u8]) -> Option<(u64, usize)> {
    let first = *buf.first()?;
    let len = vint_len(first);
    if len > 8 || buf.len() < len { return None; }
    let marker = 0x80u8 >> (len - 1);
    let mut val = (first & (marker - 1)) as u64;
    for i in 1..len { val = (val << 8) | buf[i] as u64; }
    Some((val, len))
}
// 元素头：(id, size（未知=None）, 头字节数)
fn read_elem(r: &mut dyn MkvIo) -> std::io::Result<(u64, Option<u64>, usize)> {
    let (id, il) = read_vint(r, true)?;
    let (sz, sl) = read_vint(r, false)?;
    let unknown = sz == (1u64 << (7 * sl)) - 1;
    Ok((id, if unknown { None } else { Some(sz) }, il + sl))
}
fn read_uint(r: &mut dyn MkvIo, n: u64) -> std::io::Result<u64> {
    let n = n.min(8) as usize;
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf[..n])?;
    let mut v = 0u64;
    for i in 0..n { v = (v << 8) | buf[i] as u64; }
    Ok(v)
}
fn read_bytes(r: &mut dyn MkvIo, n: u64) -> std::io::Result<Vec<u8>> {
    let n = n.min(8 * 1024 * 1024) as usize;   // 只有 CodecPrivate 会读，上限防坏文件
    let mut v = vec![0u8; n];
    r.read_exact(&mut v)?;
    Ok(v)
}
fn read_str(r: &mut dyn MkvIo, n: u64) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&read_bytes(r, n)?).trim_end_matches('\0').to_string())
}
fn skip(r: &mut dyn MkvIo, size: Option<u64>) -> std::io::Result<()> {
    match size { Some(n) => r.skip(n), None => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unknown-size leaf")) }
}
// 时间单位 → 毫秒（TimecodeScale 默认 1ms）
fn units_ms(units: i64, scale: u64) -> i64 {
    ((units as i128 * scale as i128) / 1_000_000) as i64
}

struct State { scale: u64, full: bool, done: bool, out: Parse }
impl State {
    fn keep(&self, n: u32) -> bool {
        self.out.tracks.iter().any(|t| t.number == n && (t.is_text() || t.is_pgs()))
    }
    // 按 ContentCompAlgo 还原块数据：zlib 每块独立解压 / 去头压缩把被剥的字节补回块首
    fn decode_payload(&self, n: u32, payload: Vec<u8>) -> Vec<u8> {
        let Some(t) = self.out.tracks.iter().find(|t| t.number == n) else { return payload };
        match t.comp {
            0 => {
                let mut out = Vec::new();
                match flate2::read::ZlibDecoder::new(&payload[..]).read_to_end(&mut out) {
                    Ok(_) => out,
                    Err(_) => payload,   // 坏数据原样返回，后面的解析会跳过
                }
            }
            3 => {
                let mut out = t.comp_settings.clone();
                out.extend_from_slice(&payload);
                out
            }
            _ => payload,
        }
    }
}

// ---------- 对外入口 ----------
pub fn parse_local(path: &str, full: bool) -> Result<Parse, String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    Ok(parse(&mut LocalIo(f), full))
}
pub fn parse_stream<R: Read>(r: R, full: bool) -> Parse {
    parse(&mut StreamIo { r }, full)
}

fn parse(io: &mut dyn MkvIo, full: bool) -> Parse {
    let mut st = State { scale: 1_000_000, full, done: false, out: Parse::default() };
    loop {
        if st.done { break }
        let Ok((id, size, _)) = read_elem(io) else { break };
        match id {
            ID_EBML => { if skip(io, size).is_err() { break } }
            ID_SEGMENT => walk_segment(io, &mut st),   // Segment 常为未知长度，直接读到 EOF
            _ => { if skip(io, size).is_err() { break } }
        }
    }
    st.out
}

fn walk_segment(io: &mut dyn MkvIo, st: &mut State) {
    loop {
        if st.done { return }
        let Ok((id, size, _)) = read_elem(io) else { return };
        let r = match id {
            ID_INFO => parse_info(io, size, st),
            ID_TRACKS => {
                let r = parse_tracks(io, size, st);
                if !st.full { st.done = true; }   // 阶段1：列完轨就停
                r
            }
            ID_CLUSTER if st.full => parse_cluster(io, size, st),
            _ => skip(io, size),
        };
        if r.is_err() { st.done = true; return }   // 读错（含 Range 截断）：收摊保结果
    }
}

fn parse_info(io: &mut dyn MkvIo, size: Option<u64>, st: &mut State) -> std::io::Result<()> {
    let Some(mut left) = size else { return Ok(()) };
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        if id == ID_TIMECODE_SCALE { st.scale = read_uint(io, sz)?.max(1) } else { io.skip(sz)?; }
        left = left.saturating_sub(hl as u64 + sz);
    }
    Ok(())
}

fn parse_tracks(io: &mut dyn MkvIo, size: Option<u64>, st: &mut State) -> std::io::Result<()> {
    let Some(mut left) = size else { return Ok(()) };
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        if id == ID_TRACK_ENTRY {
            let (ttype, t) = parse_track_entry(io, sz)?;
            if ttype == 0x11 { st.out.tracks.push(t) }   // 只收字幕轨
        } else { io.skip(sz)?; }
        left = left.saturating_sub(hl as u64 + sz);
    }
    Ok(())
}

fn parse_track_entry(io: &mut dyn MkvIo, size: u64) -> std::io::Result<(u64, TrackInfo)> {
    let mut left = size;
    let (mut number, mut ttype) = (0u32, 0u64);
    let (mut codec, mut lang, mut name) = (String::new(), String::new(), String::new());
    let mut private = Vec::new();
    let (mut comp, mut comp_settings) = (0xFFu8, Vec::new());
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        match id {
            ID_TRACK_NUMBER => number = read_uint(io, sz)? as u32,
            ID_TRACK_TYPE => ttype = read_uint(io, sz)?,
            ID_CODEC_ID => codec = read_str(io, sz)?,
            ID_CODEC_PRIVATE => private = read_bytes(io, sz)?,
            ID_LANGUAGE => lang = read_str(io, sz)?,
            ID_NAME => name = read_str(io, sz)?,
            ID_CONTENT_ENCODINGS => { let (a, s) = parse_content_encodings(io, sz)?; comp = a; comp_settings = s; }
            _ => io.skip(sz)?,
        }
        left = left.saturating_sub(hl as u64 + sz);
    }
    Ok((ttype, TrackInfo { number, codec, lang, name, private, comp, comp_settings }))
}

// ContentEncodings → (ContentCompAlgo, ContentCompSettings)；algo 缺省按规范为 zlib(0)
fn parse_content_encodings(io: &mut dyn MkvIo, size: u64) -> std::io::Result<(u8, Vec<u8>)> {
    let (mut out, mut settings) = (0xFFu8, Vec::new());
    let mut left = size;
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        if id == ID_CONTENT_ENCODING {
            let mut l2 = sz;
            while l2 > 0 {
                let (id2, sz2, hl2) = read_elem(io)?;
                let sz2 = sz2.unwrap_or(0);
                if id2 == ID_CONTENT_COMPRESSION {
                    let mut algo = 0u8;
                    let mut l3 = sz2;
                    while l3 > 0 {
                        let (id3, sz3, hl3) = read_elem(io)?;
                        let sz3 = sz3.unwrap_or(0);
                        if id3 == ID_CONTENT_COMP_ALGO { algo = read_uint(io, sz3)? as u8 }
                        else if id3 == ID_CONTENT_COMP_SETTINGS { settings = read_bytes(io, sz3)? }
                        else { io.skip(sz3)?; }
                        l3 = l3.saturating_sub(hl3 as u64 + sz3);
                    }
                    out = algo;
                } else { io.skip(sz2)?; }
                l2 = l2.saturating_sub(hl2 as u64 + sz2);
            }
        } else { io.skip(sz)?; }
        left = left.saturating_sub(hl as u64 + sz);
    }
    Ok((out, settings))
}

fn parse_cluster(io: &mut dyn MkvIo, size: Option<u64>, st: &mut State) -> std::io::Result<()> {
    let mut left = size.unwrap_or(u64::MAX);
    let mut cts = 0i64;
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        match id {
            ID_TIMECODE => cts = read_uint(io, sz)? as i64,
            ID_SIMPLE_BLOCK => {
                let body = read_block_body(io, sz, st)?;
                if let Some((tn, rel, payload)) = body {
                    st.out.events.push((tn, units_ms(cts + rel, st.scale), None, payload));
                }
            }
            ID_BLOCK_GROUP => parse_block_group(io, sz, cts, st)?,
            _ => io.skip(sz)?,
        }
        left = left.saturating_sub(hl as u64 + sz);
    }
    Ok(())
}

fn parse_block_group(io: &mut dyn MkvIo, size: u64, cts: i64, st: &mut State) -> std::io::Result<()> {
    let mut left = size;
    let mut blk: Option<(u32, i64, Vec<u8>)> = None;
    let mut dur: Option<u64> = None;
    while left > 0 {
        let (id, sz, hl) = read_elem(io)?;
        let sz = sz.unwrap_or(0);
        match id {
            ID_BLOCK => blk = read_block_body(io, sz, st)?,
            ID_BLOCK_DURATION => dur = Some(units_ms(read_uint(io, sz)? as i64, st.scale).max(0) as u64),
            _ => io.skip(sz)?,
        }
        left = left.saturating_sub(hl as u64 + sz);
    }
    if let Some((tn, rel, payload)) = blk {
        st.out.events.push((tn, units_ms(cts + rel, st.scale), dur, payload));
    }
    Ok(())
}

// Block 头：轨号(VINT) + int16 相对时间 + 1 字节 flags；非目标文本轨/带 lacing 的直接跳过 payload
fn read_block_body(io: &mut dyn MkvIo, size: u64, st: &State) -> std::io::Result<Option<(u32, i64, Vec<u8>)>> {
    if size < 4 { io.skip(size)?; return Ok(None) }
    let peek_n = size.min(16) as usize;
    let mut peek = [0u8; 16];
    io.read_exact(&mut peek[..peek_n])?;
    let rest = size - peek_n as u64;
    let Some((tn, tl)) = vint_mem(&peek[..peek_n]) else { io.skip(rest)?; return Ok(None) };
    if tl + 3 > peek_n { io.skip(rest)?; return Ok(None) }
    let rel = i16::from_be_bytes([peek[tl], peek[tl + 1]]) as i64;
    let flags = peek[tl + 2];
    let good = st.keep(tn as u32) && flags & 0x06 == 0;
    if !good { io.skip(rest)?; return Ok(None) }
    let mut payload = peek[tl + 3..peek_n].to_vec();
    if rest > 0 {
        let base = payload.len();
        payload.resize(base + rest as usize, 0);
        io.read_exact(&mut payload[base..])?;
    }
    Ok(Some((tn as u32, rel, st.decode_payload(tn as u32, payload))))
}

// ---------- 重建字幕文本 ----------
impl Parse {
    pub fn build_texts(&self) -> Extracted {
        let mut out = Extracted::new();
        for t in self.tracks.iter().filter(|t| t.is_text()) {
            let mut mine: Vec<(i64, Option<u64>, String)> = self.events.iter()
                .filter(|e| e.0 == t.number)
                .map(|e| (e.1, e.2, String::from_utf8_lossy(&e.3).into_owned()))
                .collect();
            mine.sort_by_key(|e| e.0);
            // 缺结束时间：取同轨下一条起点，末条 +3 秒
            for i in 0..mine.len() {
                if mine[i].1.is_none() {
                    mine[i].1 = Some(if i + 1 < mine.len() {
                        mine[i + 1].0.max(mine[i].0) as u64
                    } else {
                        (mine[i].0 + 3000).max(0) as u64
                    });
                }
            }
            let ev: Vec<(i64, i64, &str)> = mine.iter().map(|(s, e, p)| (*s, e.unwrap_or(*s as u64 + 3000) as i64, p.as_str())).collect();
            let text = if t.format() == "srt" { build_srt(&ev) } else { build_ass(t, &ev) };
            out.insert(t.number, text);
        }
        out
    }
    // PGS → .sup：块 payload 拼成裸段流后按段切（段可能跨块、一块也可能多段），
    // 每段加 PG + PTS/DTS 头（libpgs 要求每段都有 10 字节头）；时间取段首所在块的时间。
    // 显示集分组不用管：libpgs 自己按 END(0x80) 段收拢。
    pub fn build_sup(&self, track: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut seg: Vec<u8> = Vec::new();   // 当前段：type(1) + size(2) + data(size)
        let mut seg_ms = 0i64;
        for (tn, ms, _dur, payload) in &self.events {
            if *tn != track { continue }
            let mut p = payload.as_slice();
            while !p.is_empty() {
                if seg.is_empty() { seg_ms = *ms; }
                let need = if seg.len() < 3 { 3 }
                    else { 3 + u16::from_be_bytes([seg[1], seg[2]]) as usize };
                let take = need.saturating_sub(seg.len()).min(p.len());
                seg.extend_from_slice(&p[..take]);
                p = &p[take..];
                if seg.len() >= 3 && seg.len() == 3 + u16::from_be_bytes([seg[1], seg[2]]) as usize {
                    out.extend_from_slice(b"PG");
                    let pts = (seg_ms.max(0) as u64 * 90).min(u32::MAX as u64) as u32;
                    out.extend_from_slice(&pts.to_be_bytes());
                    out.extend_from_slice(&pts.to_be_bytes());   // DTS = PTS
                    out.extend_from_slice(&seg);
                    seg.clear();
                }
            }
        }
        out   // 末尾不成段的残字节丢弃
    }
}

fn fmt_ass(ms: i64) -> String {
    let ms = ms.max(0);
    format!("{}:{:02}:{:02}.{:02}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000 / 10)
}
fn fmt_srt(ms: i64) -> String {
    let ms = ms.max(0);
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

// MKV 的 ASS 包格式：ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text
// 还原成 Dialogue 行：丢 ReadOrder，起止时间来自块时间
fn build_ass(t: &TrackInfo, ev: &[(i64, i64, &str)]) -> String {
    let mut out = String::from_utf8_lossy(&t.private).trim_start_matches('\u{feff}').to_string();
    if out.trim().is_empty() {
        out.push_str("[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Microsoft YaHei,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n");
    }
    if !out.contains("[Events]") {
        out.push_str("\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    }
    for (s, e, pkt) in ev {
        let (_, rest) = pkt.split_once(',').unwrap_or(("", *pkt));
        let (layer, tail) = rest.split_once(',').unwrap_or(("0", rest));
        out.push_str(&format!("Dialogue: {},{},{},{}\n", layer, fmt_ass(*s), fmt_ass(*e), tail.trim_end_matches('\n')));
    }
    out
}

fn build_srt(ev: &[(i64, i64, &str)]) -> String {
    let mut out = String::new();
    for (i, (s, e, txt)) in ev.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, fmt_srt(*s), fmt_srt(*e), txt.trim_end_matches('\n')));
    }
    out
}