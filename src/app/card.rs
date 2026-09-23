use windows_reactor::*;

/// 卡片封面渲染方式(视图层概念,不进 model)。
#[derive(Clone, Debug)]
pub enum Cover {
    /// 纯色占位块。
    Solid { rgb: (u8, u8, u8) },
    /// 纯色背景 + 居中白色图标(媒体库文件夹卡片)。
    SolidIcon {
        rgb: (u8, u8, u8),
        icon: &'static [u8],
    },
    /// 真实封面图 + 纯色回退底;path 须为绝对路径(source_file 限制)。
    // 二期 MediaItem.cover_path 接入后启用。
    #[allow(dead_code)]
    Image {
        path: String,
        fallback_rgb: (u8, u8, u8),
    },
}

impl Cover {
    /// 纯色占位封面。
    pub fn solid(rgb: (u8, u8, u8)) -> Self {
        Self::Solid { rgb }
    }

    /// 纯色 + 内嵌白色文件夹图标;icon 字节由本函数注入,调用方零感知资产。
    pub fn folder(rgb: (u8, u8, u8)) -> Self {
        Self::SolidIcon {
            rgb,
            icon: include_bytes!("../../assets/folder-white.png"),
        }
    }

    /// 真实封面图;解码/加载失败回退纯色。path 须为绝对路径。
    // 二期 MediaItem.cover_path 接入后启用。
    #[allow(dead_code)]
    pub fn image(path: impl Into<String>, fallback_rgb: (u8, u8, u8)) -> Self {
        Self::Image {
            path: path.into(),
            fallback_rgb,
        }
    }
}

/// 封面内图标边长(DIP)。
const ICON_SIZE: f64 = 72.0;

/// 图标透明度(不烘进 PNG,资产保持纯白可复用)。
const ICON_OPACITY: f64 = 0.85;

/// 作品/媒体库通用卡片:封面 + 标题 + 副标题。
pub fn media_card(title: &str, subtitle: &str, cover: Cover) -> View {
    Border::new()
        .padding(Thickness::uniform(6.0))
        .content(
            Grid::new()
                .columns([GridLength::Pixel(149.0)])
                .rows([
                    GridLength::Pixel(223.0),
                    GridLength::Pixel(20.0),
                    GridLength::Pixel(16.0),
                ])
                .children((
                    cover_block(cover),
                    TextBlock::new()
                        .text(title)
                        .font_size(15.0)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .foreground(ThemeBrush::PrimaryText)
                        .max_lines(1)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .grid_row(1),
                    TextBlock::new()
                        .text(subtitle)
                        .font_size(12.0)
                        .foreground(Color::rgb(150, 150, 150))
                        .max_lines(1)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .grid_row(2),
                )),
        )
}

/// 封面块:圆角 8 的 Border(回退底色) + 按 Cover 分支填充 content。已带 grid_row(0)。
/// 图层用 Border content 居中叠加(不用 Grid 兄弟叠层):图片须为圆角 Border 的后代才能吃到裁切。
fn cover_block(cover: Cover) -> View {
    match cover {
        Cover::Solid { rgb } => cover_base(rgb).into(),
        Cover::SolidIcon { rgb, icon } => cover_base(rgb).content(icon_layer(icon)),
        // source_file 构建期失败 → 回退纯色;运行期解码失败不接 on_failed(纯 view 函数接不了组件
        // Message 通道),靠露出 Border 回退底色兜底。
        Cover::Image { path, fallback_rgb } => match Image::new().source_file(&path) {
            Ok(image) => cover_base(fallback_rgb).content(image.stretch(Stretch::UniformToFill)),
            Err(_) => cover_base(fallback_rgb).into(),
        },
    }
}

/// 封面底座:圆角 8 + 纯色背景 + grid_row(0)。
/// 必须返回 Border(不是 View),供各臂在 .content() 前继续构建。
fn cover_base(rgb: (u8, u8, u8)) -> Border {
    let (r, g, b) = rgb;
    Border::new()
        .corner_radius(CornerRadius::uniform(8.0))
        .background(Color::rgb(r, g, b))
        .grid_row(0)
}

/// 居中白色图标层:72 DIP 见方,透明度 0.85。png 为内嵌白色透明底 PNG。
fn icon_layer(png: &'static [u8]) -> Image {
    Image::new()
        .source_data(EncodedImage::from_static(png))
        .stretch(Stretch::Uniform)
        .width(ICON_SIZE)
        .height(ICON_SIZE)
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .opacity(ICON_OPACITY)
}
