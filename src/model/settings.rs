//! 应用设置及持久化。纯数据层,不依赖 UI。

use std::fs;
use std::path::PathBuf;

/// 应用设置;持久化到 %APPDATA%/Luminara/settings.txt(key=value 按行)。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 刮削模式:是否扫描媒体库内容补全元数据(扫描本身后续阶段实现)。
    pub scrape_mode: bool,
    /// TMDB API Key;刮削模式开启时显示并使用。
    pub tmdb_api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scrape_mode: false,
            tmdb_api_key: String::new(),
        }
    }
}

impl Settings {
    /// 读取设置;文件缺失或解析失败的字段回退默认值。
    pub fn load() -> Self {
        let Ok(text) = fs::read_to_string(settings_path()) else {
            return Self::default();
        };
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "scrape_mode" => settings.scrape_mode = value == "true",
                "tmdb_api_key" => settings.tmdb_api_key = value.to_string(),
                _ => {}
            }
        }
        settings
    }

    /// 写入设置;失败静默(本地缓存不阻断交互)。
    pub fn save(&self) {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(
            path,
            format!(
                "scrape_mode={}\ntmdb_api_key={}\n",
                if self.scrape_mode { "true" } else { "false" },
                self.tmdb_api_key,
            ),
        );
    }
}

/// 设置文件路径:%APPDATA%/Luminara/settings.txt;APPDATA 缺失退到 ./Luminara/settings.txt。
fn settings_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Luminara")
        .join("settings.txt")
}
