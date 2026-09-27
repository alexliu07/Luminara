//! 链接辅助:libmpv2-sys 负责链接 mpv 本身,但 MSVC 还需能找到导入库 mpv.lib。
//! 约定环境变量 MPV_LIB_DIR 指向含 mpv.lib 的目录,自动加入链接搜索路径。

fn main() {
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    let dir = std::env::var("MPV_LIB_DIR").unwrap_or_default();
    if dir.is_empty() {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!(
                "cargo:warning=未设置 MPV_LIB_DIR,链接 mpv.lib 需其位于 LIB 路径内(见 README)"
            );
        }
        return;
    }
    let lib = std::path::Path::new(&dir).join("mpv.lib");
    if !lib.is_file() {
        println!("cargo:warning=MPV_LIB_DIR 下未找到 mpv.lib: {}", lib.display());
    }
    println!("cargo:rustc-link-search=native={dir}");
}
