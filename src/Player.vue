<script setup>
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { useRoute, useRouter } from 'vue-router'
import { showToast, hideToast } from './toast.js'
import { srtToVtt, loadCjkFonts } from './subtitles.js'
import '@videojs/html/video/player'
import '@videojs/html/video/skin'
import '@videojs/html/video/skin.css'
import JASSUB from 'jassub'
import workerUrl from 'jassub/dist/worker/worker.js?worker&url'
import wasmUrl from 'jassub/dist/wasm/jassub-worker.wasm?url'
import modernWasmUrl from 'jassub/dist/wasm/jassub-worker-modern.wasm?url'
import {getWork} from './data.js'
import { extractSubtitles } from '@cryguy/mkv-subtitle-extractor'
import { openMkv, fetchWindow, assHeader } from './mkv.js'
import { PgsRenderer } from 'libpgs'
import pgsWorkerUrl from 'libpgs/dist/libpgs.worker.js?url'   // libpgs 是 classic worker，只能 ?url 原样引用（?worker&url 是 module worker）


const route = useRoute()
const router = useRouter()
const video = ref(null)
const info = ref(null)
const subs = ref([])        // [{file_id, label, format, text, blobUrl}]，blobUrl 仅 srt/vtt 有
const activeSub = ref(-1)   // -1 = 关闭
const menuOpen = ref(false)
let ass = null              // JASSUB 实例（ASS/SSA 用 libass 渲染）
let pgs = null              // PgsRenderer 实例（PGS 位图字幕）
const pgsData = new Map()   // file_id:track → Uint8Array（不进响应式，同 embeddedFonts 的原因）
const uiHidden = ref(false)   // 顶栏自动隐藏（播放中无操作时）
const title = ref('')
let hideTimer = 0
// ===== 内嵌字幕时间窗流式加载 =====
const mkvMeta = new Map()      // file_id → Promise<meta>（SeekHead/Tracks/Cues/字体，一次解析常驻）
const streamState = new Map()  // file_id:track → { loadedUntil, busy, pushed, cues, render }
let timeHooked = false

// 内嵌轨提取：@cryguy/mkv-subtitle-extractor 按 HTTP Range 只拉字幕块（不下载音视频）
// 一次提取全部轨，Promise 去重防并发重复拉
let extractPromise = null
// 内嵌字体：原始字节存模块级 Map——放进 subs 会被 Vue 响应式包成 Proxy，postMessage 无法克隆
const embeddedFonts = new Map()   // 轨号 → Uint8Array[]
// 文本落进 s.text；srt/vtt 顺手建 <track> 用的 blob
function setText(s, text) {
  s.text = text
  if (s.format !== 'ass' && s.format !== 'ssa') {
    const vtt = s.format === 'srt' ? srtToVtt(text) : s.format === 'vtt' ? text : null
    if (vtt) s.blobUrl = URL.createObjectURL(new Blob([vtt], { type: 'text/vtt' }))
  }
}
async function loadEmbedded() {
  if (!extractPromise) {
    extractPromise = (async () => {
      const p = info.value
      const url = p.src_kind === 'local' ? convertFileSrc(p.src) : p.src   // webdav 已是本地代理地址
      const t0 = performance.now()
      const tracks = await extractSubtitles(url, { concurrency: 8, allowFullDownload: true, verbose: true })
      console.log(`[字幕] Range 提取完成，用时 ${(performance.now() - t0).toFixed(0)}ms，轨数 ${tracks.length}`)
      const texts = {}
      for (const t of tracks) {
        const s = subs.value.find(x => x.track === t.metadata.trackNumber)
        if (!s || s.format === 'pgs') continue   // extractor 对 PGS 输出乱码，走 get_subtitle_bin
        const text = new TextDecoder().decode(t.output.subtitle)
        setText(s, text)
        texts[t.metadata.trackNumber] = text
        embeddedFonts.set(t.metadata.trackNumber, (t.output.fonts || []).map(f => f.data))   // 只留字节，不进响应式对象
      }
      if (Object.keys(texts).length) invoke('save_sub_cache', { fileId: p.file_id, texts }).catch(() => {})
    })().catch(e => { extractPromise = null; throw e })
  }
  return extractPromise
}

// 内嵌轨取文本：缓存 → Range 提取（快、带字体）→ 失败回退后端提取
async function fetchEmbeddedText(s) {
  try {
    const cached = await invoke('peek_sub_cache', { fileId: s.file_id, track: s.track })
    if (cached != null) { setText(s, cached); return }
  } catch {}
  try {
    await loadEmbedded()
    if (s.text != null) return
  } catch (e) {
    console.error('Range 提取失败，回退后端提取', e)
  }
  setText(s, await invoke('get_subtitle_text', { fileId: s.file_id, track: s.track }))
}
function fmtVtt(ms) {
  ms = Math.max(0, Math.round(ms))
  const p = (n, w) => String(n).padStart(w, '0')
  return `${p(Math.floor(ms / 3600000), 2)}:${p(Math.floor(ms / 60000 % 60), 2)}:${p(Math.floor(ms / 1000 % 60), 2)}.${p(ms % 1000, 3)}`
}

// 时间窗流式：失败抛错由调用方回退整文件提取
async function startStream(s, v) {
  const p = info.value
  const url = p.src_kind === 'local' ? convertFileSrc(p.src) : p.src
  if (!mkvMeta.has(s.file_id)) mkvMeta.set(s.file_id, openMkv(url))
  const meta = await mkvMeta.get(s.file_id)
  const t = meta.tracks.find(x => x.number === s.track)
  if (!t) throw new Error('内嵌轨不存在')
  const st = { loadedUntil: 0, busy: false, pushed: new Set(), cues: [], render: null }
  streamState.set(s.file_id + ':' + s.track, st)   // 新渲染器换新状态，旧推送作废
  if (s.format === 'ass' || s.format === 'ssa') {
    let fonts, defaultFont
    try { ({ fonts, defaultFont } = await loadCjkFonts()) } catch (e) {
      console.error('中文字体加载失败', e)
      ;({ fonts, defaultFont } = { fonts: [], defaultFont: '' })
    }
    if (activeSub.value !== subs.value.indexOf(s) || video.value !== v) return
    const allFonts = [...meta.fonts.map(f => f.data), ...fonts]   // 内嵌字体优先
    ass = new JASSUB({
      video: v,
      subContent: assHeader(t),        // 只装 Styles 头，事件走 processChunk 流式追加
      workerUrl, wasmUrl, modernWasmUrl,
      ...(allFonts.length ? { fonts: allFonts, ...(defaultFont ? { defaultFont } : {}) } : {}),
    })
    await ass.ready
    st.render = blocks => { for (const b of blocks) ass.renderer.processChunk(b.text, b.timeMs, b.durMs) }
  } else {
    st.render = blocks => {
      st.cues.push(...blocks)
      st.cues.sort((a, b) => a.timeMs - b.timeMs)
      const body = st.cues.map((c, i) =>
          `${i + 1}\n${fmtVtt(c.timeMs)} --> ${fmtVtt(c.timeMs + c.durMs)}\n${c.text}\n`).join('\n')
      if (s.blobUrl) URL.revokeObjectURL(s.blobUrl)
      s.blobUrl = URL.createObjectURL(new Blob(['WEBVTT\n\n' + body], { type: 'text/vtt' }))
      nextTick(() => {
        const idx = subs.value.indexOf(s)
        if (activeSub.value !== idx || video.value !== v) return
        const el = v.querySelector(`track[data-sub="${idx}"]`)
        if (el?.track) el.track.mode = 'showing'
      })
    }
  }
  if (!timeHooked) { v.addEventListener('timeupdate', onTimeUpdate); timeHooked = true }
  await fillStream(s, st, Math.max(0, (v.currentTime || 0) * 1000 - 10000))
}

// 拉一个 150 秒窗并推给渲染器；已推过的块去重
async function fillStream(s, st, fromMs) {
  if (st.busy) return
  st.busy = true
  try {
    const meta = await mkvMeta.get(s.file_id)
    const blocks = await fetchWindow(meta, fromMs, fromMs + 150000, new Set([s.track]))
    const fresh = blocks.filter(b => !st.pushed.has(b.timeMs + '�' + b.text))
    for (const b of fresh) st.pushed.add(b.timeMs + '�' + b.text)
    st.loadedUntil = Math.max(st.loadedUntil, fromMs + 150000)
    if (fresh.length) st.render(fresh)
  } finally {
    st.busy = false
  }
}

// 播放推进：前方余量不足 1 分钟就补下一窗（跳进度就以当前位置开窗）
function onTimeUpdate() {
  const i = activeSub.value
  if (i < 0) return
  const s = subs.value[i]
  if (!s || s.track == null || s.windowed === false) return
  const st = streamState.get(s.file_id + ':' + s.track)
  if (!st || st.busy) return
  const nowMs = (video.value?.currentTime || 0) * 1000
  if (nowMs + 60000 <= st.loadedUntil) return
  fillStream(s, st, Math.max(0, nowMs > st.loadedUntil ? nowMs - 10000 : st.loadedUntil))
      .catch(e => console.warn('字幕窗口拉取失败', e))
}
// 鼠标一动就显示顶栏并重新计时；播放中静止 2.5 秒后隐藏
function pokeUi() {
  uiHidden.value = false
  clearTimeout(hideTimer)
  hideTimer = setTimeout(() => {
    const v = video.value
    if (v && !v.paused && !v.ended && !menuOpen.value) uiHidden.value = true
  }, 2000)
}
// 暂停/播完强制显示
function showUi() {
  clearTimeout(hideTimer)
  uiHidden.value = false
}
// 鼠标移出窗口立即隐藏（对齐底栏的 mouseleave → setInactive）
function hideUiNow() {
  clearTimeout(hideTimer)
  const v = video.value
  if (v && !v.paused && !v.ended && !menuOpen.value) uiHidden.value = true
}
// 播放地址：本地走 asset protocol，WebDAV 走后端本地代理
const playSrc = computed(() => {
  const p = info.value
  if (!p) return ''
  return p.src_kind === 'local' ? convertFileSrc(p.src) : p.src
})
// 走原生 <track> 的字幕（srt 已转 vtt blob）
const textSubs = computed(() => subs.value.map((s, i) => ({ s, i })).filter(x => x.s.blobUrl))
// TODO: 播放进度记忆（记住 currentTime 与 file_id，重返本集时续播）
onMounted(async () => {
  window.addEventListener('mousemove', pokeUi)
  try {
    const p = await invoke('get_playback_info', { fileId: Number(route.params.fileId) })
    const list = []
    for (const s of p.subtitles) {
      if (s.track != null) { list.push({ ...s, text: null, blobUrl: null }); continue }  // 内嵌轨选中时才提取
      try {
        const text = await invoke('get_subtitle_text', { fileId: s.file_id, track: s.track })
        const vtt = s.format === 'srt' ? srtToVtt(text) : s.format === 'vtt' ? text : null
        list.push({ ...s, text, blobUrl: vtt ? URL.createObjectURL(new Blob([vtt], { type: 'text/vtt' })) : null })
      } catch (e) {
        console.error('字幕加载失败', s.label, e)
      }
    }
    subs.value = list
    info.value = p
    getWork(route.params.tmdb, route.params.kind).then(w => {
      const ep = (w.episodes || []).find(e => e.file_id === Number(route.params.fileId))
      if (!ep) return        // 没刮削到的文件：保留文件名
      title.value = ep.episode > 0
          ? `第${ep.episode}话 ${ep.title}`.trim()
          : (ep.title || p.name)   // 电影条目没有集号，直接作品名
    }).catch(() => {})
  } catch (e) {
    console.error('加载播放信息失败', e)
    showToast('加载失败：' + e, 'alert', 3000)
  }
})

onBeforeUnmount(() => {
  destroyAss()
  window.removeEventListener('mousemove', pokeUi)
  clearTimeout(hideTimer)
  for (const s of subs.value) if (s.blobUrl) URL.revokeObjectURL(s.blobUrl)
  video.value?.removeEventListener('timeupdate', onTimeUpdate)
})

function selectSub(i) {
  activeSub.value = i
  menuOpen.value = false
  applySub()
}

// 单选切换：原生轨全禁用，ASS 走 JASSUB 覆盖层（字体加载是异步的）
async function applySub() {
  destroyAss()
  const v = video.value
  if (!v) return
  for (const t of v.textTracks) t.mode = 'disabled'
  const i = activeSub.value
  if (i < 0) return
  const s = subs.value[i]
  // PGS 位图字幕：libpgs 整轨加载（无分块 API），不走时间窗
  if (s.format === 'pgs') {
    showToast('正在加载 PGS 字幕…', 'spin')
    try {
      const key = s.file_id + ':' + s.track
      if (!pgsData.has(key)) {
        const b64 = await invoke('get_subtitle_bin', { fileId: s.file_id, track: s.track })
        pgsData.set(key, Uint8Array.from(atob(b64), c => c.charCodeAt(0)))
      }
    } catch (e) {
      console.error('PGS 字幕加载失败', e)
      showToast('PGS 字幕加载失败：' + e, 'alert', 3000)
      return
    }
    hideToast()
    if (activeSub.value !== i || video.value !== v) return   // 等待期间又切走了
    pgs = new PgsRenderer({ video: v, workerUrl: pgsWorkerUrl })
    await pgs.loadFromBuffer(pgsData.get(s.file_id + ':' + s.track))
    return
  }
  // 内嵌轨：命中缓存秒开，否则时间窗流式（边播边补）；容器不支持时回退整文件提取
  if (s.track != null && s.windowed !== false) {
    let cached = null
    try { cached = await invoke('peek_sub_cache', { fileId: s.file_id, track: s.track }) } catch {}
    if (cached != null) {
      s.windowed = false
      setText(s, cached)
    } else {
      try {
        await startStream(s, v)
        return
      } catch (e) {
        console.warn('窗口流式加载不可用，回退整文件提取', e)
        s.windowed = false
      }
    }
  }
  // 内嵌轨懒提取：首次选中才拉（一次提取全部轨，之后换轨零成本）
  if (s.text == null) {
    showToast('正在提取内嵌字幕…', 'spin')
    try {
      await fetchEmbeddedText(s)
    } catch (e) {
      console.error('字幕提取失败', s.label, e)
      showToast('字幕提取失败：' + e, 'alert', 3000)
      return
    }
    hideToast()
    if (activeSub.value !== i || video.value !== v) return   // 等待期间又切走了
  }
  await nextTick()   // 让 <track> 元素先渲染出来
  if (activeSub.value !== i || video.value !== v) return
  if (s.format === 'ass' || s.format === 'ssa') {
    // 中文字形靠系统字体（WebView2 的 queryLocalFonts 不一定可用），失败也要能渲拉丁字
    let fonts, defaultFont
    try {
      ({ fonts, defaultFont } = await loadCjkFonts())
    } catch (e) {
      console.error('中文字体加载失败', e)
      ;({ fonts, defaultFont } = { fonts: [], defaultFont: '' })
    }
    if (activeSub.value !== i || video.value !== v) return   // 等待期间又切走了
    try {
      const allFonts = [...(embeddedFonts.get(s.track) || []), ...fonts]
      ass = new JASSUB({
        video: v,
        subContent: s.text,
        workerUrl, wasmUrl, modernWasmUrl,
        ...(allFonts.length ? { fonts: allFonts, ...(defaultFont ? { defaultFont } : {}) } : {}),
      })
    } catch (e) {
      console.error('ASS 渲染初始化失败', e)
      showToast('ASS 字幕渲染失败', 'alert', 3000)
    }
    return
  }
  const el = v.querySelector(`track[data-sub="${i}"]`)
  if (el?.track) el.track.mode = 'showing'
}

function destroyAss() {
  if (ass) {
    try { ass.destroy() } catch {}
    ass = null
  }
  if (pgs) {
    try { pgs.dispose() } catch {}
    pgs = null
  }
}

function onBack() {
  router.back()
}
</script>

<template>
  <div class="player-page" :class="{ 'ui-hidden': uiHidden }" @mouseleave="hideUiNow">
    <!-- 图标 sprite -->
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-back" viewBox="0 0 24 24"><path d="M15 5.5 8.5 12l6.5 6.5" /></symbol>
      <symbol id="i-cc" viewBox="0 0 24 24"><rect x="3" y="5.5" width="18" height="13" rx="2.5" /><path d="M10.75 10.5a2.5 2.5 0 1 0 0 3M17.75 10.5a2.5 2.5 0 1 0 0 3" /></symbol>
    </svg>

    <div class="p-bar">
      <button class="round-btn" type="button" aria-label="返回" @click="onBack">
        <svg class="ic"><use href="#i-back" /></svg>
      </button>
      <p class="p-title">{{ title }}</p>
      <div class="p-subs" v-if="subs.length">
        <button class="round-btn" type="button" aria-label="字幕" @click="menuOpen = !menuOpen">
          <svg class="ic"><use href="#i-cc" /></svg>
        </button>
        <Transition name="menu">
          <div class="p-menu" v-if="menuOpen">
            <button class="p-menu-item" type="button" :class="{ on: activeSub === -1 }" @click="selectSub(-1)">关闭</button>
            <button class="p-menu-item" type="button" v-for="(s, i) in subs" :key="s.file_id + ':' + s.track"
                    :class="{ on: activeSub === i }" @click="selectSub(i)">{{ s.label }}</button>
          </div>
        </Transition>
      </div>
    </div>

    <div class="p-stage">
      <video-player v-if="playSrc">
        <video-skin>
          <video ref="video" :src="playSrc" playsinline @pause="showUi" @ended="showUi">
            <track v-for="x in textSubs" :key="x.i" kind="subtitles" :src="x.s.blobUrl"
                   :label="x.s.label" :data-sub="x.i" />
          </video>
        </video-skin>
      </video-player>
    </div>
  </div>
</template>

<style scoped>
/* ── 基础（同 WorkDetail 的视觉语言）────────────────── */
.player-page {
  height: calc(100vh / var(--ui-scale));
  display: flex;
  flex-direction: column;
  background: #17161a;
  color: #f2eff0;
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
  -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
  position: relative;
  overflow: hidden;
  scrollbar-width: none;
}
.player-page::-webkit-scrollbar { display: none; }
.player-page.ui-hidden { cursor: none; }
.sprite { position: absolute; width: 0; height: 0; overflow: hidden; }
.ic {
  width: 20px; height: 20px;
  fill: none; stroke: currentColor; stroke-width: 2;
  stroke-linecap: round; stroke-linejoin: round; flex-shrink: 0;
}
.round-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  color: #f2eff0;
  background: rgba(50,50,50,0.5);
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),
      0 4px 16px rgba(40, 25, 30, 0.25);
  cursor: pointer;
}

/* ── 顶栏 ─────────────────────────────────────────── */
.p-bar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  background: linear-gradient(180deg, rgba(0, 0, 0, 0.5), transparent);
  transition: opacity 0.25s ease, transform 0.25s ease;
}
.p-title {
  margin: 0;
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.92);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.player-page.ui-hidden .p-bar {
  opacity: 0;
  transform: translateY(-8px);
  pointer-events: none;
}
.p-subs { position: relative; }
.p-menu {
  position: absolute;
  top: 110%;
  right: 0;
  z-index: 30;
  min-width: 130px;
  max-height: 320px;
  overflow-y: auto;
  padding: 6px;
  border-radius: 12px;
  background: rgba(50,50,50,0.5);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow: 0 8px 24px rgba(40, 25, 30, 0.3);
  scrollbar-width: none;
}
.p-menu::-webkit-scrollbar { display: none; }
.p-menu-item {
  display: block;
  width: 100%;
  padding: 7px 12px;
  border: none;
  border-radius: 8px;
  background: transparent;
  font-size: 13px;
  color: #f2eff0;
  text-align: left;
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.p-menu-item:hover { background: var(--option-bg); }
.p-menu-item.on { font-weight: 700; }
.menu-enter-active, .menu-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.menu-enter-from, .menu-leave-to { opacity: 0; transform: scale(0.95) translateY(-4px); }

/* ── 播放器 ───────────────────────────────────────── */
.p-stage {
  position: absolute;
  inset: 0;
}
video-skin {
  display: block;
  width: 100%;
  height: 100%;
  position: relative;   /* JASSUB 画布定位基准 */
  background: #000;
  --media-border-radius: 0;
}
video { object-fit: contain; }
</style>
