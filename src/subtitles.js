// 字幕工具
import { invoke, convertFileSrc } from '@tauri-apps/api/core'

// SRT → WebVTT：加头 + 时间戳毫秒的逗号转点（浏览器 <track> 只认 VTT）
export function srtToVtt(text) {
    const body = text
        .replace(/^﻿/, '')
        .replace(/\r/g, '')
        .replace(/(\d{2}:\d{2}:\d{2}),(\d{3})/g, '$1.$2')
    return 'WEBVTT\n\n' + body.trim() + '\n'
}

// 系统中文字体字节（ASS 渲染用），整个会话缓存只读一次
let fontCache = null
export function loadCjkFonts() {
    if (!fontCache) {
        fontCache = (async () => {
            const files = await invoke('get_cjk_fonts')
            const fonts = []
            let defaultFont = ''
            for (const f of files) {
                try {
                    const buf = await (await fetch(convertFileSrc(f.path))).arrayBuffer()
                    fonts.push(new Uint8Array(buf))
                    if (!defaultFont) defaultFont = f.family
                } catch (e) {
                    console.error('字体加载失败', f.path, e)
                }
            }
            return { fonts, defaultFont }
        })().catch(e => { fontCache = null; throw e })
    }
    return fontCache
}
