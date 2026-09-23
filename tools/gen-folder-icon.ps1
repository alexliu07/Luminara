# 一次性生成 assets/folder-white.png:256x256 纯白文件夹剪影 + alpha 透明底。
# 再生: powershell -NoProfile -ExecutionPolicy Bypass -File tools/gen-folder-icon.ps1
# 要点:
#   1. 位图必须 Format32bppArgb(24bpp 无 alpha 会存成不透明方块);
#   2. CompositingMode.SourceCopy(SourceOver 在透明黑上画白会混出灰黑边,SourceCopy 才能得到「白 RGB + 半透明 A」软边);
#   3. 标签页 + 主体放进同一 GraphicsPath(FillMode.Winding)一次填充成并集,只在外轮廓抗锯齿;
#   4. 纯白 FromArgb(255,255,255,255);透明度由 Image.opacity 控制,不烘进 PNG,资产保持可复用。

param(
    [string]$OutPath = (Join-Path $PSScriptRoot '..\assets\folder-white.png')
)

Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'

# 往 GraphicsPath 追加圆角矩形(顺时针绕向,保证与其它子路径一致,Winding 填充取并集)。
function Add-RoundedRect {
    param(
        [System.Drawing.Drawing2D.GraphicsPath]$Path,
        [float]$X, [float]$Y, [float]$W, [float]$H, [float]$Radius
    )
    $d = $Radius * 2
    $Path.AddArc($X, $Y, $d, $d, 180, 90)                     # 左上
    $Path.AddArc($X + $W - $d, $Y, $d, $d, 270, 90)           # 右上
    $Path.AddArc($X + $W - $d, $Y + $H - $d, $d, $d, 0, 90)   # 右下
    $Path.AddArc($X, $Y + $H - $d, $d, $d, 90, 90)            # 左下
    $Path.CloseFigure()
}

$size = 256
$bitmap = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy

    $path = New-Object System.Drawing.Drawing2D.GraphicsPath([System.Drawing.Drawing2D.FillMode]::Winding)
    try {
        Add-RoundedRect -Path $path -X 32 -Y 40 -W 96 -H 48 -Radius 8     # 标签页凸起
        Add-RoundedRect -Path $path -X 32 -Y 72 -W 192 -H 144 -Radius 12  # 主体
        $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
        try {
            $graphics.FillPath($brush, $path)
        } finally {
            $brush.Dispose()
        }
    } finally {
        $path.Dispose()
    }

    $outDir = Split-Path -Parent $OutPath
    if (-not (Test-Path $outDir)) {
        New-Item -ItemType Directory -Path $outDir | Out-Null
    }
    $bitmap.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "已生成 $((Resolve-Path $OutPath).Path)"
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
