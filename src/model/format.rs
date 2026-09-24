//! 媒体文件格式:按扩展名判定可播放性。纯数据层,不依赖 UI。
//! 只做扩展名判定——真正的解码能力由 Media Foundation 决定,这里不猜编解码器。

/// 视频扩展名(小写)。
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "ts", "m2ts", "vob",
    "3gp", "ogv",
];

/// 音频扩展名(小写)。
const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma", "aiff", "ape",
];

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
        assert!(!is_supported("a.srt"));
        assert!(!is_supported(".hidden"));
        assert!(!is_supported("noext"));
    }
}
