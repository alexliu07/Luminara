//! 媒体库文件索引:后台扫描目录树并持久化。纯数据层,不依赖 UI。

use std::fs;
use std::path::Path;

/// 一个索引条目(文件或目录)。
#[derive(Clone, Debug, PartialEq)]
pub struct IndexEntry {
    /// 相对媒体库根的路径(正斜杠分隔)。
    pub rel_path: String,
    /// 文件/目录名(路径最后一段)。
    pub name: String,
    pub is_dir: bool,
    /// 文件字节数;目录为 0。
    pub size: u64,
}

/// 递归扫描目录树,收集全部文件/目录条目。失败的子目录静默跳过。
pub fn build(root: &Path) -> Vec<IndexEntry> {
    let mut entries = Vec::new();
    walk(root, "", &mut entries);
    entries
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<IndexEntry>) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return
    };
    for item in read_dir.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let rel_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(meta) = item.metadata() else {
            continue
        };
        let is_dir = meta.is_dir();
        out.push(IndexEntry {
            rel_path: rel_path.clone(),
            name,
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
        });
        if is_dir {
            walk(&item.path(), &rel_path, out);
        }
    }
}

/// 写入索引文件;失败静默(后台任务不阻断交互)。
/// 行格式:`d;rel_path;name` 或 `f;rel_path;name;size`。
pub fn save(library_id: u64, entries: &[IndexEntry]) {
    let path = index_path(library_id);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut text = String::new();
    for e in entries {
        if e.is_dir {
            text.push_str(&format!("d;{};{}\n", e.rel_path, e.name));
        } else {
            text.push_str(&format!("f;{};{};{}\n", e.rel_path, e.name, e.size));
        }
    }
    let _ = fs::write(path, text);
}

/// 删除某媒体库的索引文件;失败静默。仅清理 Luminara 自己的缓存,不动媒体文件。
pub fn remove(library_id: u64) {
    let _ = fs::remove_file(index_path(library_id));
}

/// 索引文件路径:%APPDATA%\Luminara\index\<library_id>.txt。
fn index_path(library_id: u64) -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Luminara")
        .join("index")
        .join(format!("{library_id}.txt"))
}
