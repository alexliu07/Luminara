// MKV 字幕窗口读取：Cues 定位 + HTTP Range，按时间窗只拉附近字幕块（边播边补）
// 依赖容器的 SeekHead 与 CueRelativePosition（mkvmerge/makemkv 等常见 muxer 都有）；
// 不满足时抛错，由调用方回退整文件提取
// 压缩轨（ContentEncodings）在块级还原：algo 0=zlib 走 DecompressionStream，3=去头补字节

const ID = {
    EBML: 0x1a45dfa3, SEGMENT: 0x18538067,
    SEEK_HEAD: 0x114d9b74, SEEK: 0x4dbb, SEEK_ID: 0x53ab, SEEK_POSITION: 0x53ac,
    INFO: 0x1549a966, TIMECODE_SCALE: 0x2ad7b1,
    TRACKS: 0x1654ae6b, TRACK_ENTRY: 0xae, TRACK_NUMBER: 0xd7, TRACK_TYPE: 0x83,
    CODEC_ID: 0x86, CODEC_PRIVATE: 0x63a2, LANGUAGE: 0x22b59c, NAME: 0x536e,
    CONTENT_ENCODINGS: 0x6d80, CONTENT_ENCODING: 0x6240, CONTENT_COMPRESSION: 0x5034,
    CONTENT_COMP_ALGO: 0x4254, CONTENT_COMP_SETTINGS: 0x4255,
    ATTACHMENTS: 0x1941a469, ATTACHED_FILE: 0x61a7, FILE_NAME: 0x466e, FILE_DATA: 0x465c,
    CUES: 0x1c53bb6b, CUE_POINT: 0xbb, CUE_TIME: 0xb3,
    CUE_TRACK_POSITIONS: 0xb7, CUE_TRACK: 0xf7, CUE_CLUSTER_POSITION: 0xf1, CUE_RELATIVE_POSITION: 0xf0,
    SIMPLE_BLOCK: 0xa3, BLOCK_GROUP: 0xa0, BLOCK: 0xa1, BLOCK_DURATION: 0x9b,
}
const dec = new TextDecoder()

// ---------- EBML 基元 ----------
function vintLen(first) {
    let len = 1, mask = 0x80
    while (len <= 8 && !(first & mask)) { mask >>= 1; len++ }
    return len
}
// [值, 字节数, 值位是否全 1（=未知长度）]；keepMarker=true 时值含长度标记（元素 ID 用）
function readVint(buf, off, keepMarker = false) {
    const first = buf[off]
    const len = vintLen(first)
    if (len > 8 || off + len > buf.length) throw new Error('vint 越界')
    const marker = 0x80 >> (len - 1)
    let val = keepMarker ? first : (first & (marker - 1))
    let allOnes = (first & (marker - 1)) === marker - 1
    for (let i = 1; i < len; i++) {
        val = val * 256 + buf[off + i]
        if (buf[off + i] !== 0xff) allOnes = false
    }
    return [val, len, allOnes]
}
function readUint(buf, off, len) {
    let val = 0
    for (let i = 0; i < len; i++) val = val * 256 + buf[off + i]
    return val
}
// 元素头：{ id, size, headerLen, dataOff, unknown }
function readElem(buf, off) {
    const [id, il] = readVint(buf, off, true)
    const [size, sl, unknown] = readVint(buf, off + il)
    return { id, size, headerLen: il + sl, dataOff: off + il + sl, unknown }
}
// 顺序遍历 buf[from..to) 里的子元素
function walk(buf, cb, from = 0, to = buf.length) {
    let p = from
    while (p + 2 <= to) {
        let e
        try { e = readElem(buf, p) } catch { return }
        if (e.unknown || e.dataOff + e.size > to) return
        cb(e)
        p = e.dataOff + e.size
    }
}
function readStr(buf, e) {
    return dec.decode(buf.subarray(e.dataOff, e.dataOff + e.size)).replace(/\0+$/, '')
}
// ContentEncodings → t.comp / t.compSettings（对齐后端 mkv.rs 的 parse_content_encodings）
function parseContentEncodings(buf, el, t) {
    walk(buf, (e) => {
        if (e.id !== ID.CONTENT_ENCODING) return
        walk(buf, (c) => {
            if (c.id !== ID.CONTENT_COMPRESSION) return
            let algo = 0     // 缺省按规范为 zlib
            walk(buf, (d) => {
                if (d.id === ID.CONTENT_COMP_ALGO) algo = readUint(buf, d.dataOff, d.size)
                else if (d.id === ID.CONTENT_COMP_SETTINGS) t.compSettings = buf.slice(d.dataOff, d.dataOff + d.size)
            }, c.dataOff, c.dataOff + c.size)
            t.comp = algo
        }, e.dataOff, e.dataOff + e.size)
    }, el.dataOff, el.dataOff + el.size)
}

// ---------- Range 读取（32KB 预读缓存） ----------
class RangeReader {
    constructor(url) { this.url = url; this.size = 0; this.cacheOff = -1; this.cache = null }
    async read(off, len) {
        if (this.cache && off >= this.cacheOff && off + len <= this.cacheOff + this.cache.length) {
            return this.cache.subarray(off - this.cacheOff, off - this.cacheOff + len)
        }
        const end = Math.min(off + Math.max(len, 32 * 1024) - 1, this.size - 1)
        const resp = await fetch(this.url, { headers: { Range: `bytes=${off}-${end}` } })
        if (resp.status !== 206) throw new Error('服务器不支持 Range，无法窗口加载')
        this.cacheOff = off
        this.cache = new Uint8Array(await resp.arrayBuffer())
        return this.cache.subarray(0, Math.min(len, this.cache.length))
    }
}

// 读 SeekHead 指向的整个元素（先读 4KB 头拿 size，不够再补读）
async function readElementAt(r, segOff, relPos) {
    const abs = segOff + relPos
    const head = await r.read(abs, 4096)
    const el = readElem(head, 0)
    if (el.unknown) throw new Error('未知长度元素')
    const total = el.headerLen + el.size
    const buf = total <= head.length ? head.subarray(0, total) : await r.read(abs, total)
    return { id: el.id, data: buf.subarray(el.headerLen, el.headerLen + el.size) }
}

// ---------- 对外：打开容器（元数据一次解析常驻） ----------
export async function openMkv(url) {
    const r = new RangeReader(url)
    // 探测首 256KB：文件大小 + EBML 头 + SeekHead 都在开头
    const resp = await fetch(url, { headers: { Range: 'bytes=0-262143' } })
    if (resp.status !== 206) throw new Error('服务器不支持 Range')
    const m = (resp.headers.get('Content-Range') || '').match(/\/(\d+)/)
    if (!m) throw new Error('响应缺 Content-Range')
    r.size = Number(m[1])
    r.cacheOff = 0
    r.cache = new Uint8Array(await resp.arrayBuffer())
    const probe = r.cache

    let el = readElem(probe, 0)
    if (el.id !== ID.EBML) throw new Error('不是 MKV 文件')
    el = readElem(probe, el.dataOff + el.size)
    if (el.id !== ID.SEGMENT) throw new Error('未找到 Segment')
    const segOff = el.dataOff
    // 找 SeekHead（通常在 Segment 最前）
    let seekEl = null, off = segOff
    while (off + 2 <= probe.length) {
        let e
        try { e = readElem(probe, off) } catch { break }
        if (e.id === ID.SEEK_HEAD) { seekEl = e; break }
        if (e.unknown || e.dataOff + e.size > probe.length) break
        off = e.dataOff + e.size
    }
    if (!seekEl || seekEl.dataOff + seekEl.size > probe.length) throw new Error('文件缺 SeekHead')

    // SeekHead → 各段位置（相对 Segment 数据起点）
    const seeks = new Map()
    walk(probe, (e) => {
        if (e.id !== ID.SEEK) return
        let sid = null, spos = null
        walk(probe, (c) => {
            if (c.id === ID.SEEK_ID) sid = readUint(probe, c.dataOff, c.size)
            if (c.id === ID.SEEK_POSITION) spos = readUint(probe, c.dataOff, c.size)
        }, e.dataOff, e.dataOff + e.size)
        if (sid != null && spos != null) seeks.set(sid, spos)
    }, seekEl.dataOff, seekEl.dataOff + seekEl.size)

    const meta = { reader: r, segOff, scale: 1e6, tracks: [], cues: [], fonts: [] }

    // Info → TimestampScale
    if (seeks.has(ID.INFO)) {
        const info = await readElementAt(r, segOff, seeks.get(ID.INFO))
        walk(info.data, (e) => {
            if (e.id === ID.TIMECODE_SCALE) meta.scale = readUint(info.data, e.dataOff, e.size) || 1e6
        })
    }
    // Tracks → 文本字幕轨（含 CodecPrivate 头）
    const tracksPos = seeks.get(ID.TRACKS)
    if (tracksPos == null) throw new Error('SeekHead 缺 Tracks 索引')
    const tracksEl = await readElementAt(r, segOff, tracksPos)
    walk(tracksEl.data, (e) => {
        if (e.id !== ID.TRACK_ENTRY) return
        const t = { number: 0, type: 0, codec: '', lang: '', name: '', private: new Uint8Array(),
                    comp: 0xff, compSettings: new Uint8Array() }
        walk(tracksEl.data, (c) => {
            if (c.id === ID.TRACK_NUMBER) t.number = readUint(tracksEl.data, c.dataOff, c.size)
            else if (c.id === ID.TRACK_TYPE) t.type = readUint(tracksEl.data, c.dataOff, c.size)
            else if (c.id === ID.CODEC_ID) t.codec = readStr(tracksEl.data, c)
            else if (c.id === ID.CODEC_PRIVATE) t.private = tracksEl.data.slice(c.dataOff, c.dataOff + c.size)
            else if (c.id === ID.LANGUAGE) t.lang = readStr(tracksEl.data, c)
            else if (c.id === ID.NAME) t.name = readStr(tracksEl.data, c)
            else if (c.id === ID.CONTENT_ENCODINGS) parseContentEncodings(tracksEl.data, c, t)
        }, e.dataOff, e.dataOff + e.size)
        if (t.type === 0x11) meta.tracks.push(t)
    })
    meta.byNum = new Map(meta.tracks.map(t => [t.number, t]))
    // Cues → 块定位索引
    const cuesPos = seeks.get(ID.CUES)
    if (cuesPos != null) {
        const cuesEl = await readElementAt(r, segOff, cuesPos)
        walk(cuesEl.data, (e) => {
            if (e.id !== ID.CUE_POINT) return
            let timeMs = null, track = null, clusterPos = null, relPos = null
            walk(cuesEl.data, (c) => {
                if (c.id === ID.CUE_TIME) timeMs = readUint(cuesEl.data, c.dataOff, c.size) * meta.scale / 1e6
                else if (c.id === ID.CUE_TRACK_POSITIONS) {
                    walk(cuesEl.data, (d) => {
                        if (d.id === ID.CUE_TRACK) track = readUint(cuesEl.data, d.dataOff, d.size)
                        else if (d.id === ID.CUE_CLUSTER_POSITION) clusterPos = readUint(cuesEl.data, d.dataOff, d.size)
                        else if (d.id === ID.CUE_RELATIVE_POSITION) relPos = readUint(cuesEl.data, d.dataOff, d.size)
                    }, c.dataOff, c.dataOff + c.size)
                }
            }, e.dataOff, e.dataOff + e.size)
            if (timeMs != null && track != null && clusterPos != null) meta.cues.push({ timeMs, track, clusterPos, relPos })
        })
        meta.cues.sort((a, b) => a.timeMs - b.timeMs)
    }
    // Attachments → 内嵌字体（ASS 特效字形）
    const attPos = seeks.get(ID.ATTACHMENTS)
    if (attPos != null) {
        const attEl = await readElementAt(r, segOff, attPos)
        walk(attEl.data, (e) => {
            if (e.id !== ID.ATTACHED_FILE) return
            let name = '', data = null
            walk(attEl.data, (c) => {
                if (c.id === ID.FILE_NAME) name = readStr(attEl.data, c)
                else if (c.id === ID.FILE_DATA) data = attEl.data.slice(c.dataOff, c.dataOff + c.size)
            }, e.dataOff, e.dataOff + e.size)
            if (data && /\.(ttf|otf|ttc|woff2?)$/i.test(name)) meta.fonts.push({ name, data })
        })
    }
    return meta
}

// 块数据还原：0=zlib（每块独立、带 zlib 头，正好是 DecompressionStream('deflate') 的格式）
// 3=去头压缩（把被剥掉的固定字节补回块首）；0xFF = 轨没有 ContentEncodings，原样返回
async function decodeBlock(payload, track) {
    if (!track || track.comp === 0xff) return payload
    if (track.comp === 3) {
        const s = track.compSettings
        const out = new Uint8Array(s.length + payload.length)
        out.set(s, 0)
        out.set(payload, s.length)
        return out
    }
    const stream = new Blob([payload]).stream().pipeThrough(new DecompressionStream('deflate'))
    return new Uint8Array(await new Response(stream).arrayBuffer())
}

// 解析单个块元素（SimpleBlock 或 BlockGroup）→ { track, timeMs, durMs, text }
async function parseBlockEl(slice, cue, scale, nextMs, byNum) {
    const el = readElem(slice, 0)
    const tMul = scale / 1e6
    const make = async (body, durUnits) => {
        const [tn, tl] = readVint(body, 0)
        if (tl + 3 > body.length) return null
        if (body[tl + 2] & 0x06) return null        // 带 lacing 的不是文本字幕
        const durMs = durUnits != null ? durUnits * tMul
            : (nextMs != null ? Math.max(nextMs - cue.timeMs, 200) : 3000)
        let bytes
        try {
            bytes = await decodeBlock(body.subarray(tl + 3), byNum.get(tn))
        } catch (e) {
            console.warn('字幕块解压失败，跳过该块', e)   // 坏块不拖垮整窗
            return null
        }
        return { track: tn, timeMs: cue.timeMs, durMs, text: dec.decode(bytes) }
    }
    if (el.id === ID.SIMPLE_BLOCK) return make(slice.subarray(el.dataOff, el.dataOff + el.size), null)
    if (el.id === ID.BLOCK_GROUP) {
        let dur = null, block = null
        walk(slice, (c) => {
            if (c.id === ID.BLOCK) block = slice.subarray(c.dataOff, c.dataOff + c.size)
            if (c.id === ID.BLOCK_DURATION) dur = readUint(slice, c.dataOff, c.size)
        }, el.dataOff, el.dataOff + el.size)
        return block ? make(block, dur) : null
    }
    return null
}

// ---------- 对外：拉 [fromMs, toMs) 内指定轨的字幕块 ----------
export async function fetchWindow(meta, fromMs, toMs, trackNums) {
    const hits = meta.cues.filter(c => trackNums.has(c.track) && c.timeMs >= fromMs && c.timeMs < toMs)
    if (!hits.length) return []
    if (hits.some(c => c.relPos == null)) throw new Error('Cue 缺 RelativePosition，无法定点读取')
    // 同轨下一条时间（块没带 BlockDuration 时用来估时长）
    const nextTime = new Map()
    for (const t of trackNums) {
        const list = meta.cues.filter(c => c.track === t)
        for (let i = 0; i < list.length; i++) nextTime.set(list[i], i + 1 < list.length ? list[i + 1].timeMs : null)
    }
    const hdrLen = new Map()
    for (const c of hits) {
        if (!hdrLen.has(c.clusterPos)) {
            const hb = await meta.reader.read(meta.segOff + c.clusterPos, 8)
            hdrLen.set(c.clusterPos, readElem(hb, 0).headerLen)
        }
    }
    const targets = hits.map(c => ({
        c,
        abs: meta.segOff + c.clusterPos + hdrLen.get(c.clusterPos) + c.relPos,
    })).sort((a, b) => a.abs - b.abs)
    const out = []
    const GAP = 32 * 1024, BLOCK_EXTRA = 8 * 1024
    let i = 0
    while (i < targets.length) {
        let j = i
        // 链式合并加总跨度上限
        while (j + 1 < targets.length && targets[j + 1].abs - targets[j].abs <= GAP
        && targets[j + 1].abs - targets[i].abs <= 256 * 1024) j++
        const start = targets[i].abs
        const buf = await meta.reader.read(start, targets[j].abs + BLOCK_EXTRA - start)
        for (let k = i; k <= j; k++) {
            const off = targets[k].abs - start
            if (off >= buf.length) continue
            let el
            try { el = readElem(buf, off) } catch { continue }
            // 解析出的不是块元素、或 size 离谱就丢弃，绝不按假 size 发请求
            if (el.id !== ID.SIMPLE_BLOCK && el.id !== ID.BLOCK_GROUP) continue
            const need = el.headerLen + (el.unknown ? 0 : el.size)
            if (need > 256 * 1024) continue
            const slice = off + need <= buf.length ? buf.subarray(off, off + need)
                : await meta.reader.read(targets[k].abs, need)
            const blk = await parseBlockEl(slice, targets[k].c, meta.scale, nextTime.get(targets[k].c), meta.byNum)
            if (blk && blk.track === targets[k].c.track) out.push(blk)
        }
        i = j + 1
    }
    out.sort((a, b) => a.timeMs - b.timeMs)
    return out
}

// ASS 头（CodecPrivate）：给 JASSUB 初始化用，之后 processChunk 流式追加事件
export function assHeader(track) {
    let out = dec.decode(track.private || new Uint8Array()).replace(/^﻿/, '')
    if (!out.trim()) {
        out = '[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\n' +
            'Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n' +
            'Style: Default,Microsoft YaHei,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,2,10,10,10,1\n'
    }
    if (!out.includes('[Events]')) {
        out += '\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n'
    }
    return out
}