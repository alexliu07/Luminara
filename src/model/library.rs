use std::fs;
use std::path::PathBuf;

/// 一个已添加的媒体库(本地文件夹)。纯数据层,不依赖 UI。
#[derive(Clone, Debug, PartialEq)]
pub struct MediaLibrary {
    pub id: u64,
    /// 文件夹名(路径最后一段)。
    pub name: String,
    /// 文件夹完整路径;也用于去重。
    pub path: String,
    /// 占位封面底色,由路径哈希稳定派生。
    pub cover_rgb: (u8, u8, u8),
}

impl MediaLibrary {
    /// 从完整路径构建;`id` 由调用方分配。
    pub fn from_path(id: u64, path: String) -> Self {
        let name = path
            .rsplit(['\\', '/'])
            .find(|segment| !segment.is_empty())
            .unwrap_or(&path)
            .to_string();
        let cover_rgb = cover_rgb_for(&path);
        Self {
            id,
            name,
            path,
            cover_rgb,
        }
    }
}

/// 由路径 FNV-1a 哈希派生暗色占位封面:同路径每次一致,不同路径尽量不同。
fn cover_rgb_for(path: &str) -> (u8, u8, u8) {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // 各通道取哈希不同字节段,映射到与示例封面同风格的暗色区间 (30–130)
    let channel = |shift: u32| 30 + ((hash >> shift) as u8 % 101);
    (channel(0), channel(8), channel(16))
}

/// 加载已保存的媒体库列表;文件缺失或行解析失败跳过该行。
/// 行格式:`id;path`(名称/封面色由 path 派生,不持久化)。
pub fn load_libraries() -> Vec<MediaLibrary> {
    let Ok(text) = fs::read_to_string(libraries_path()) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (id, path) = line.split_once(';')?;
            let id: u64 = id.parse().ok()?;
            Some(MediaLibrary::from_path(id, path.to_string()))
        })
        .collect()
}

/// 写入媒体库列表;失败静默(本地缓存不阻断交互)。
pub fn save_libraries(libraries: &[MediaLibrary]) {
    let path = libraries_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text: String = libraries
        .iter()
        .map(|lib| format!("{};{}\n", lib.id, lib.path))
        .collect();
    let _ = fs::write(path, text);
}

/// 媒体库列表文件路径:%APPDATA%\Luminara\libraries.txt。
fn libraries_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Luminara")
        .join("libraries.txt")
}
