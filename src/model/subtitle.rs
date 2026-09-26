//! 外挂字幕:同名字幕文件发现 + 轨道模型。纯数据层,不依赖 UI 与具体解析 crate。

use std::path::{Path, PathBuf};

/// 字幕文件格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleKind {
    /// .srt:序号 + 时间轴 + 多行文本。
    Srt,
    /// .ass / .ssa:Dialogue 行,文本是最后一个逗号字段。
    Ass,
    /// .vtt:与 srt 同构,时间用点分隔毫秒。
    Vtt,
    /// .sup:PGS 图形字幕(BD)。
    Pgs,
    /// .sub(+.idx):VobSub 图形字幕(DVD)。
    VobSub,
}

/// 一个外挂字幕轨(一个字幕文件)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleFile {
    pub path: PathBuf,
    /// 菜单里显示的短名:同名后缀(如 zh-CN),无后缀则为文件名。
    pub label: String,
    pub kind: SubtitleKind,
}

/// 是否文本字幕(自绘管线走文本解析);图形字幕走图形解码。
pub fn is_text(kind: SubtitleKind) -> bool {
    matches!(
        kind,
        SubtitleKind::Srt | SubtitleKind::Ass | SubtitleKind::Vtt
    )
}

/// 按扩展名判字幕格式;非字幕扩展名返回 None。
pub fn kind_of(name: &str) -> Option<SubtitleKind> {
    let ext = name.rsplit_once('.')?.1;
    if ext.eq_ignore_ascii_case("srt") {
        Some(SubtitleKind::Srt)
    } else if ext.eq_ignore_ascii_case("ass") || ext.eq_ignore_ascii_case("ssa") {
        Some(SubtitleKind::Ass)
    } else if ext.eq_ignore_ascii_case("vtt") {
        Some(SubtitleKind::Vtt)
    } else if ext.eq_ignore_ascii_case("sup") {
        Some(SubtitleKind::Pgs)
    } else if ext.eq_ignore_ascii_case("sub") {
        Some(SubtitleKind::VobSub)
    } else {
        None
    }
}

/// 发现与媒体同名的外挂字幕:同目录、主名相同或以「主名.」开头、扩展名受支持。
/// 结果按文件名排序,保证每次打开顺序稳定。
///
/// VobSub 只登记 .sub(同名 .idx 由解析器自己找);.idx 不单独成轨。
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

/// 同目录同名的 VobSub 索引文件(.idx)路径;.sub 的元数据(时间戳/尺寸)都在里面。
pub fn vobsub_idx(sub: &Path) -> PathBuf {
    sub.with_extension("idx")
}

/// 扩展名(含点)长度;无扩展名返回 0。
fn ext_len(name: &str) -> usize {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.len() + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 判定字幕格式() {
        assert_eq!(kind_of("a.srt"), Some(SubtitleKind::Srt));
        assert_eq!(kind_of("a.ASS"), Some(SubtitleKind::Ass));
        assert_eq!(kind_of("a.ssa"), Some(SubtitleKind::Ass));
        assert_eq!(kind_of("a.vtt"), Some(SubtitleKind::Vtt));
        assert_eq!(kind_of("a.sup"), Some(SubtitleKind::Pgs));
        assert_eq!(kind_of("a.sub"), Some(SubtitleKind::VobSub));
        assert_eq!(kind_of("a.idx"), None);
        assert_eq!(kind_of("a.mp4"), None);
    }

    #[test]
    fn 同名字幕发现() {
        let dir = std::env::temp_dir().join(format!("luminara-sub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("movie.mkv");
        for name in [
            "movie.srt",
            "movie.zh-CN.srt",
            "movie.en.ass",
            "movie.sup",
            "movie.sub",
            "other.srt",
            "movies.srt",
            "movie.idx",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let tracks = discover(&media);
        let labels: Vec<&str> = tracks.iter().map(|t| t.label.as_str()).collect();
        // 按文件名序:movie.en.ass, movie.srt, movie.sub, movie.sup, movie.zh-CN.srt;
        // 无语言后缀的轨标签=文件名(菜单里可区分);movie.idx 不单独成轨。
        assert_eq!(
            labels,
            vec!["en", "movie.srt", "movie.sub", "movie.sup", "zh-CN"]
        );
        assert_eq!(tracks[0].kind, SubtitleKind::Ass);
        assert_eq!(tracks[2].kind, SubtitleKind::VobSub);
        assert_eq!(tracks[3].kind, SubtitleKind::Pgs);
        assert_eq!(vobsub_idx(&tracks[2].path), dir.join("movie.idx"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
