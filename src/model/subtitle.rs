//! 外挂字幕:同名字幕文件发现 + SRT/ASS/VTT 解析 + 按时间取字幕。纯数据层,不依赖 UI。
//!
//! 踩坑:WinRT 的 TimedMetadataTrack 对 .ass 支持很差(样式/特效全丢),
//! 这里干脆全部自解析、UI 层 TextBlock 自绘,行为可控且轨道切换简单。

use std::path::{Path, PathBuf};

/// 字幕文件格式。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SubtitleKind {
    /// .srt:序号 + 时间轴 + 多行文本。
    Srt,
    /// .ass / .ssa:Dialogue 行,文本是最后一个逗号字段。
    Ass,
    /// .vtt:与 srt 同构,时间用点分隔毫秒。
    Vtt,
}

/// 一个外挂字幕轨(一个字幕文件)。
#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleFile {
    pub path: PathBuf,
    /// 菜单里显示的短名:同名后缀(如 zh-CN),无后缀则为文件名。
    pub label: String,
    pub kind: SubtitleKind,
}

/// 一条字幕:起止秒 + 文本(可多行)。
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// 发现与媒体同名的外挂字幕:同目录、主名相同或以「主名.」开头、扩展名受支持。
/// 结果按文件名排序,保证每次打开顺序稳定。
pub fn discover(media: &Path) -> Vec<SubtitleFile> {
    let Some(stem) = media.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    let Some(dir) = media.parent() else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tracks = Vec::new();
    for item in read_dir.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let Some(kind) = kind_of(&name) else {
            continue;
        };
        // 去掉字幕扩展名后须与媒体主名相同,或是「主名.<语言>」形式。
        let cand = name[..name.len() - ext_len(&name)]
            .trim_end_matches('.')
            .to_string();
        if cand != stem && !cand.starts_with(&format!("{stem}.")) {
            continue;
        }
        let label = cand[stem.len()..].trim_start_matches('.').to_string();
        tracks.push(SubtitleFile {
            path: item.path(),
            label: if label.is_empty() {
                name.clone()
            } else {
                label
            },
            kind,
        });
    }
    tracks.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    tracks
}

/// 解析字幕文件内容为字幕条目(按 start 排序)。解析不出任何条目则返回空表。
pub fn parse(kind: SubtitleKind, text: &str) -> Vec<Cue> {
    let mut cues = match kind {
        SubtitleKind::Srt | SubtitleKind::Vtt => parse_timed_text(kind, text),
        SubtitleKind::Ass => parse_ass(text),
    };
    cues.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cues
}

/// 取某时刻应显示的字幕;无则 None。倒序扫描,遇早于窗口的条目即停。
pub fn cue_at(cues: &[Cue], time: f64) -> Option<&Cue> {
    let i = cues.partition_point(|c| c.start <= time);
    cues[..i].iter().rev().take(16).find(|c| time < c.end)
}

/// SRT/VTT:块之间空行分隔,块内首行序号(VTT 可无)、次行时间轴,其余为文本。
fn parse_timed_text(kind: SubtitleKind, text: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut lines = text.lines().map(str::trim_end).peekable();
    // 块级收集:时间轴行出现则开新块,空行提交。
    let mut block: Option<Cue> = None;
    let mut text_lines: Vec<String> = Vec::new();
    while let Some(line) = lines.next() {
        let line = line.trim_start_matches('\u{feff}'); // 去 BOM(只在首行,多去无害)
        let blank = line.trim().is_empty();
        if blank {
            if let Some(mut cue) = block.take() {
                cue.text = join_text(&text_lines);
                if !cue.text.is_empty() && cue.end > cue.start {
                    cues.push(cue);
                }
            }
            text_lines.clear();
            continue;
        }
        if let Some((start, end)) = parse_arrow_line(line) {
            if let Some(mut cue) = block.take() {
                cue.text = join_text(&text_lines);
                if !cue.text.is_empty() && cue.end > cue.start {
                    cues.push(cue);
                }
                text_lines.clear();
            }
            block = Some(Cue {
                start,
                end,
                text: String::new(),
            });
            continue;
        }
        if block.is_some() {
            text_lines.push(clean_markup(kind, line));
        }
        // 时间轴之前的行(序号/WEBVTT 头/NOTE 等)直接忽略。
    }
    if let Some(mut cue) = block.take() {
        cue.text = join_text(&text_lines);
        if !cue.text.is_empty() && cue.end > cue.start {
            cues.push(cue);
        }
    }
    cues
}

/// 找到时间轴行返回 (start, end);不是则 None。
/// 兼容 `00:00:01,000 --> 00:00:04,000` / `0:00:01.00 --> ...`(尾部可带对齐参数)。
fn parse_arrow_line(line: &str) -> Option<(f64, f64)> {
    let (left, right) = line.split_once("-->")?;
    let start = parse_timestamp(left.trim())?;
    let end = parse_timestamp(right.trim().split_whitespace().next()?)?;
    Some((start, end))
}

/// 时间戳 → 秒。支持 `HH:MM:SS,mmm` / `H:MM:SS.cs` / `MM:SS.mmm`,分隔符逗号点号皆可。
fn parse_timestamp(text: &str) -> Option<f64> {
    let mut parts = text.split(':').collect::<Vec<_>>();
    if parts.len() == 2 {
        parts.insert(0, "0");
    }
    if parts.len() != 3 {
        return None;
    }
    let hours: f64 = parts[0].parse().ok()?;
    let minutes: f64 = parts[1].parse().ok()?;
    let seconds: f64 = parts[2].replace(',', ".").parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// ASS/SSA:`[Events]` 段的 `Dialogue:` 行,前 9 个逗号是元数据,第 10 段起是文本。
fn parse_ass(text: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut in_events = false;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_events = trimmed.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("Dialogue:") else {
            continue;
        };
        // splitn(10):前 9 段元数据 + 剩余全部为文本(文本内可能有逗号)。
        let fields = rest.trim_start().splitn(10, ',').collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let (Some(start), Some(end)) = (
            parse_timestamp(fields[1].trim()),
            parse_timestamp(fields[2].trim()),
        ) else {
            continue;
        };
        let text = clean_markup(SubtitleKind::Ass, fields[9]);
        if !text.is_empty() && end > start {
            cues.push(Cue { start, end, text });
        }
    }
    cues
}

/// 清掉标记:HTML 尖括号标签、ASS 花括号特效;ASS 的 \N \n 换行、\h 空格。
fn clean_markup(kind: SubtitleKind, line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut skip = 0usize; // >0 表示在跳过的配对符号内
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if kind == SubtitleKind::Ass => skip += 1,
            '}' if kind == SubtitleKind::Ass && skip > 0 => skip -= 1,
            '<' if kind != SubtitleKind::Ass => skip += 1,
            '>' if kind != SubtitleKind::Ass && skip > 0 => skip -= 1,
            '\\' if kind == SubtitleKind::Ass && skip == 0 => match chars.peek() {
                Some('N') | Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('h') => {
                    chars.next();
                    out.push(' ');
                }
                _ => {}
            },
            _ if skip == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 多行文本拼接(去空行首尾)。
fn join_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 文件名的字幕扩展名判定。
fn kind_of(name: &str) -> Option<SubtitleKind> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "srt" => Some(SubtitleKind::Srt),
        "ass" | "ssa" => Some(SubtitleKind::Ass),
        "vtt" => Some(SubtitleKind::Vtt),
        _ => None,
    }
}

/// 文件名字幕扩展名长度(含点)。
fn ext_len(name: &str) -> usize {
    name.rsplit_once('.').map(|(_, e)| e.len() + 1).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析srt() {
        let cues = parse(
            SubtitleKind::Srt,
            "1\n00:00:01,000 --> 00:00:04,000\n你好\n<i>世界</i>\n\n2\n00:00:05.500 --> 00:00:06,000\n第二条\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "你好\n世界");
        assert_eq!(cues[1].start, 5.5);
        assert_eq!(
            cue_at(&cues, 2.0).map(|c| c.text.as_str()),
            Some("你好\n世界")
        );
        assert_eq!(cue_at(&cues, 4.5), None);
    }

    #[test]
    fn 解析ass() {
        let cues = parse(
            SubtitleKind::Ass,
            "[Script Info]\nTitle: x\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,{\\an8}第一行\\N第二行, 带逗号\n",
        );
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "第一行\n第二行, 带逗号");
    }
}
