//! 媒体/字幕文件格式:扩展名判定 + 同级可播文件收集。纯数据层,不依赖 UI。
//!
//! 只做扩展名判定——真正的解码能力由 mpv/ffmpeg 决定,这里不猜编解码器。

/// 视频扩展名(小写)。
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "ts", "m2ts", "vob",
    "3gp", "ogv",
];

/// 音频扩展名(小写)。
const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma", "aiff", "ape",
];

/// 字幕扩展名(小写)。浏览页显示但点击不进播放页。
const SUBTITLE_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sup", "sub", "idx"];

/// 是否为可播放的媒体文件(视频或音频)。按文件名扩展名判定。
pub fn is_supported(name: &str) -> bool {
    is_video(name) || is_audio(name)
}

/// 是否为视频文件。扩展名大小写不敏感。
pub fn is_video(name: &str) -> bool {
    contains(VIDEO_EXTS, name)
}

/// 是否为音频文件。扩展名大小写不敏感。
pub fn is_audio(name: &str) -> bool {
    contains(AUDIO_EXTS, name)
}

/// 是否为字幕文件。
pub fn is_subtitle(name: &str) -> bool {
    contains(SUBTITLE_EXTS, name)
}

/// 同级可播文件(同目录、受支持的媒体文件),按文件名升序。
/// 上一个/下一个按此序切换;当前文件不在其中(扩展名不支持)时返回空表。
pub fn collect_siblings(media: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Some(dir) = media.parent() else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<std::path::PathBuf> = read_dir
        .flatten()
        .map(|item| item.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| is_supported(&name.to_string_lossy()))
        })
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
}

fn contains(list: &[&str], name: &str) -> bool {
    ext(name).is_some_and(|e| list.iter().any(|item| item.eq_ignore_ascii_case(e)))
}

/// 取扩展名(不含点)。无扩展名或纯点名返回 None。
fn ext(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 判定扩展名() {
        assert!(is_supported("a.MP4"));
        assert!(is_supported("a.mkv"));
        assert!(is_supported("a.flac"));
        assert!(is_video("a.webm"));
        assert!(is_audio("a.opus"));
        assert!(is_subtitle("a.SRT"));
        assert!(is_subtitle("a.sup"));
        assert!(is_subtitle("a.idx"));
        assert!(!is_supported("a.srt"));
        assert!(!is_supported(".hidden"));
        assert!(!is_supported("noext"));
    }

    #[test]
    fn 同级文件收集() {
        let dir = std::env::temp_dir().join(format!("luminara-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.mkv"), b"").unwrap();
        std::fs::write(dir.join("a.mp3"), b"").unwrap();
        std::fs::write(dir.join("c.txt"), b"").unwrap();
        std::fs::write(dir.join("d.srt"), b"").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        let files = collect_siblings(&dir.join("b.mkv"));
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.mp3".to_string(), "b.mkv".to_string()]);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
