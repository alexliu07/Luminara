<script setup>
import {ref, computed, onMounted, onActivated, onDeactivated, nextTick} from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { useRouter } from 'vue-router'
import { usePageScroller } from './scroll.js'
import {getUnscraped, getWork, getWorks} from "./data.js";
import { showToast, hideToast } from './toast.js'
import { extractSubtitles } from '@cryguy/mkv-subtitle-extractor'

const router = useRouter()
const scroller = usePageScroller()
// ── hero 轮播 ───────────────────────────────────────────
const heroItems = ref([])
const heroIndex = ref(0)
const heroDrag = ref(null)    // { startX, dx } 拖拽中

// 拖动结束后的瞬间忽略 click，防止“拖着滑行后松手在原卡上”误开详情
let justDragged = false
function markDragged() {
  justDragged = true
  setTimeout(() => { justDragged = false }, 0)   // click 派发完再复位
}
// 库中不足 5 部全上，多了随机抽 5 部（每次刷新重新抽）
function pickHeroes() {
  const arr = [...works.value]
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[arr[i], arr[j]] = [arr[j], arr[i]]
  }
  heroItems.value = arr.slice(0, 5).map(w => ({
    key: 'h' + w.kind + w.tmdb_id,
    tmdb: w.tmdb_id,
    kind: w.kind,
    title: w.title,
    rating: w.rating ? w.rating.toFixed(1) : '',
    year: w.air_date ? w.air_date.slice(0, 4) : '',
    country: w.region || '',
    genres: (w.genres || []).join(','),
    cert: w.certification || '',
    cover: w.backdrop ? convertFileSrc(w.backdrop) : (w.poster ? convertFileSrc(w.poster) : null),
  }))
  heroIndex.value = 0
}

const hero = computed(() => heroItems.value[heroIndex.value] || {})
const heroTrackStyle = computed(() => ({
  transform: `translateX(calc(${-heroIndex.value * 100}% + ${heroDrag.value ? heroDrag.value.dx : 0}px))`,
  transition: heroDrag.value ? 'none' : 'transform 0.28s ease',
}))

// hero 左右拖（松手超过 40px 翻页）
function heroDown(e) {
  heroDrag.value = { startX: e.clientX, dx: 0 }
  window.addEventListener('mousemove', heroMove)
  window.addEventListener('mouseup', heroUp)
}
function heroMove(e) {
  if (heroDrag.value) heroDrag.value.dx = e.clientX - heroDrag.value.startX
}
function heroUp() {
  const dx = heroDrag.value ? heroDrag.value.dx : 0
  heroDrag.value = null
  window.removeEventListener('mousemove', heroMove)
  window.removeEventListener('mouseup', heroUp)
  if (Math.abs(dx) > 5) markDragged()
  if (dx < -40 && heroIndex.value < heroItems.value.length - 1) heroIndex.value++
  if (dx > 40 && heroIndex.value > 0) heroIndex.value--
}
function openAll(name) {
  getWorks();
  getUnscraped();
  router.push('/all/' + encodeURIComponent(name))
}
// ── 继续观看：每部作品最近一集，按观看时间倒序 ────────────
const history = ref([])
async function loadHistory() {
  try { history.value = await invoke('list_continue_watching') }
  catch (e) { console.error('加载继续观看失败', e) }
}
onActivated(loadHistory)   // MediaLibrary 在 KeepAlive 里，从播放页返回不会重新 onMounted，靠激活刷新
const continueRow = computed(() => ({
  title: '继续观看',
  items: history.value.map(h => ({
    key: 'c' + h.kind + h.tmdb_id,
    tmdb: h.tmdb_id,
    kind: h.kind,
    file_id: h.file_id,
    title: h.title,
    sub: h.kind === 'tv' ? `第${h.season}季 第${h.episode}话` : '电影',
    cover: h.still ? convertFileSrc(h.still) : (h.backdrop ? convertFileSrc(h.backdrop) : null),
    progress: h.duration > 0 ? Math.min(1, h.position / h.duration) : 0,
  })),
}))
function openContinue(it) {
  if (justDragged) return
  router.push(`/play/${it.kind}/${it.tmdb}/${it.file_id}`)   // 播放页自己 seek 回进度
}

// ── 右上角菜单 ───────────────────────────────────────────
const menuOpen = ref(false)
const scanning = ref(false)   // 防重复触发（扫描里含刮削，可能跑很久）
// 扫描全部文件源（本地 + WebDAV），结束后刷新整页数据
async function scanAll() {
  menuOpen.value = false
  if (scanning.value) return
  scanning.value = true
  let added = 0, removed = 0, updated = 0
  showToast('正在扫描文件源', 'spin')
  try {
    const list = await invoke('list_sources')
    if (!list.length) {
      hideToast()
      showToast('还没有添加文件源', 'alert', 2000)
      return
    }
    for (const s of list) {
      const n = s.source === 0
          ? await invoke('index_local_source', { id: s.id })
          : await invoke('index_webdav_source', { id: s.id })
      added += n.added
      removed += n.removed
      updated += n.updated
    }
    window.dispatchEvent(new Event('library-refresh'))   // 让 data.js 丢掉页面缓存
    works.value = await invoke('list_works')
    unscraped.value = await invoke('list_unscraped')
    pickHeroes()
    loadHistory()   // 刚刮削出来的作品要重新查才进得了「继续观看」
    hideToast()
    showToast(`扫描完毕，添加了 ${added} 个文件，删除了 ${removed} 个文件，更新了 ${updated} 个文件`, 'check', 2000)
  } catch (e) {
    hideToast()
    showToast('扫描失败：' + e, 'alert', 3000)
  } finally {
    scanning.value = false
  }
}

// 缓存所有字幕：后端先给清单（已缓存的跳过），这里逐个跑，好显示进度和随时叫停
const caching = ref(false)
const cancelCache = ref(false)
function onCacheSubtitles() {
  if (caching.value) { cancelCache.value = true; return }   // 缓存中再点一次=停止
  cacheAllSubtitles()
}
async function cacheAllSubtitles() {
  menuOpen.value = false
  if (caching.value) return
  caching.value = true
  cancelCache.value = false
  let done = 0, tracks = 0, failed = 0
  showToast('正在统计待缓存的字幕…', 'spin')
  try {
    const targets = await invoke('list_cache_targets')
    if (!targets.length) {
      hideToast()
      showToast('没有需要缓存的文件', 'check', 2000)
      return
    }
    for (const t of targets) {
      if (cancelCache.value) {
        hideToast()
        showToast(`已停止：缓存了 ${tracks} 条字幕轨`, 'check', 2500)
        return
      }
      done++
      showToast(`缓存字幕 ${done}/${targets.length}${t.online ? '（在线）' : ''}：${t.name}`, 'spin')
      try {
        tracks += await cacheOne(t)
      } catch (e) {
        failed++
        console.warn('字幕缓存失败', t.name, e)
      }
    }
    hideToast()
    showToast(`字幕缓存完成：${targets.length} 个文件，${tracks} 条字幕轨` + (failed ? `，${failed} 个失败` : ''), 'check', 3000)
  } catch (e) {
    hideToast()
    showToast('缓存失败：' + e, 'alert', 3000)
  } finally {
    caching.value = false
  }
}
// 单个文件：在线且全是文本轨的走前端提取器 —— 它按 Cues 直接跳字幕块 + 8 并发，
// 通常只读整文件的几个百分点；后端的整解要遍历每个簇和块头，在线源慢得多。
// 本地文件仍走后端（seek 跳读本来就快），带 PGS / zlib 压缩轨的也走后端（提取器不认这两种）
async function cacheOne(target) {
  if (!target.online) return invoke('cache_subtitle', { fileId: target.file_id })
  const p = await invoke('get_playback_info', { fileId: target.file_id })
  const emb = p.subtitles.filter(s => s.track != null)
  // 一条内嵌轨都没有：轨清单刚才已经拿到了，不用再叫后端整解一遍，直接写个空标记
  if (!emb.length) {
    await invoke('save_sub_cache', { fileId: p.file_id, texts: {} })
    return 0
  }
  const hard = emb.filter(s => s.format === 'pgs' || s.comp !== 0xFF)
  // 有 PGS / 压缩轨：后端整解一次，顺带把文本轨也解了，别跑两遍
  if (hard.length) return invoke('cache_subtitle', { fileId: target.file_id })
  const t0 = performance.now()
  const list = await extractSubtitles(p.src, { concurrency: 8, allowFullDownload: true })
  const texts = {}
  for (const tr of list) {
    const s = emb.find(x => x.track === tr.metadata.trackNumber)
    if (s) texts[tr.metadata.trackNumber] = new TextDecoder().decode(tr.output.subtitle)
  }
  console.log(`[字幕缓存] ${target.name}：${Object.keys(texts).length} 轨，用时 ${(performance.now() - t0).toFixed(0)}ms`)
  await invoke('save_sub_cache', { fileId: p.file_id, texts })   // 空结果也写，标记解过了
  return Object.keys(texts).length
}

// ── 继续观看：右键菜单 + 删除确认 ────────────────────────
const ctxOpen = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxItem = ref(null)
const ctxMenuEl = ref(null)
const confirmOpen = ref(false)
// 界面按 --ui-scale 缩放，鼠标坐标要换算回布局坐标
function uiScale() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-scale')) || 1
}
// 右键弹菜单：贴着鼠标，量出尺寸后夹在视口内
function openCtx(e, item) {
  const scale = uiScale()
  const x = e.clientX / scale
  const y = e.clientY / scale
  ctxItem.value = item
  ctxX.value = x
  ctxY.value = y
  ctxOpen.value = true
  menuOpen.value = false   // 顺手关掉右上角菜单

  nextTick(() => {
    const el = ctxMenuEl.value
    if (!el) return
    const m = 8   // 离边最小留白
    ctxX.value = Math.max(m, Math.min(x, window.innerWidth / scale - el.offsetWidth - m))
    ctxY.value = Math.max(m, Math.min(y, window.innerHeight / scale - el.offsetHeight - m))
  })
}
// 被 KeepAlive 缓存起来（切 tab / 进播放页）时收起所有浮层，回来不会剩个菜单挂着
onDeactivated(() => {
  menuOpen.value = false
  ctxOpen.value = false
  confirmOpen.value = false
  closeSearch()
})
function onDeleteRecord() {
  ctxOpen.value = false
  confirmOpen.value = true
}
function onCancelDelete() {
  confirmOpen.value = false
}
async function onConfirmDelete() {
  confirmOpen.value = false
  const it = ctxItem.value
  if (!it) return
  try {
    await invoke('delete_progress', { tmdb: it.tmdb, kind: it.kind })
  } catch (e) {
    showToast('删除失败：' + e, 'alert', 2500)
    return
  }
  history.value = history.value.filter(h => !(h.tmdb_id === it.tmdb && h.kind === it.kind))   // 先本地移除
  ctxItem.value = null
  nextTick(() => window.dispatchEvent(new Event('resize')))   // 行高变了，让滚动条重算
}

// ── 搜索 ────────────────────────────────────────────────
const searchOpen = ref(false)
const q = ref('')
const searchInput = ref(null)
// 命中作品标题（大小写不敏感的子串），最多列 60 条
const results = computed(() => {
  const s = q.value.trim().toLowerCase()
  if (!s) return []
  return works.value
      .filter(w => (w.title || '').toLowerCase().includes(s))
      .slice(0, 60)
      .map(w => ({
        key: 's' + w.kind + w.tmdb_id,
        tmdb: w.tmdb_id,
        kind: w.kind,
        title: w.title,
        cover: w.poster ? convertFileSrc(w.poster) : null,
        rating: w.rating ? w.rating.toFixed(1) : '',
        sub: [w.kind === 'tv' ? `共${w.total_seasons ?? '?'}季` : '电影',
              w.air_date ? w.air_date.slice(0, 4) : '', w.region || ''].filter(Boolean).join(' · '),
      }))
})
function openSearch() {
  menuOpen.value = false
  ctxOpen.value = false
  searchOpen.value = true
  nextTick(() => searchInput.value?.focus())   // 展开后直接能打字
}
function closeSearch() {
  searchOpen.value = false
  q.value = ''
}
// 收起时点它=展开，展开后点它=回到输入框
function onSearchFace() {
  if (searchOpen.value) searchInput.value?.focus()
  else openSearch()
}
function openResult(w) {
  closeSearch()
  openDetail({ tmdb: w.tmdb, kind: w.kind })
}

// ── 真实数据 ─────────────────────────────────────────────
const works = ref([])
const unscraped = ref([])

onMounted(async () => {
  try {
    works.value = await invoke('list_works')
    unscraped.value = await invoke('list_unscraped')
    pickHeroes()
  } catch (e) {
    console.error('加载媒体库失败', e)
  }
})

// 作品 → 海报卡
function toCard(w) {
  return {
    key: 'w' + w.kind + w.tmdb_id,
    tmdb: w.tmdb_id,
    kind: w.kind,
    title: w.title,
    sub: w.kind === 'tv' ? `共${w.total_seasons ?? '?'}季` : (w.air_date ? w.air_date.slice(0, 4) : ''),
    rating: w.rating ? w.rating.toFixed(1) : '',
    cover: w.poster ? convertFileSrc(w.poster) : null,
  }
}

// 分类行：电影 / 电视剧 / 未刮削（空分类不显示）
const posterRows = computed(() => {
  const rows = []
  const movie = works.value.filter(w => w.kind === 'movie').map(toCard)
  const tv = works.value.filter(w => w.kind === 'tv').map(toCard)
  const none = unscraped.value.map(u => ({
    key: 'f' + u.rel_path,
    title: u.name,
    sub: u.rel_path,
    rating: '',
    cover: null,
  }))
  if (movie.length) rows.push({ title: '电影', items: movie })
  if (tv.length) rows.push({ title: '电视剧', items: tv })
  if (none.length) rows.push({ title: '未刮削', items: none })
  return rows
})
// 海报行：按住拖动平移
function rowDrag(e) {
  const el = e.currentTarget
  const startX = e.clientX
  const startLeft = el.scrollLeft
  let moved = false
  el.classList.add('dragging')
  function onMove(ev) {
    if (Math.abs(ev.clientX - startX) > 5) moved = true
    el.scrollLeft = startLeft - (ev.clientX - startX)   // 拖动跟手
  }
  function onUp() {
    el.classList.remove('dragging')
    window.removeEventListener('mousemove', onMove)
    window.removeEventListener('mouseup', onUp)
    if(moved)markDragged()
  }
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mouseup', onUp)
}
function openDetail(w) {
  if (justDragged) return
  if (!w.tmdb) return
  getWork(w.tmdb,w.kind)
  router.push(`/work/${w.kind}/${w.tmdb}`)
}
const grad = (c) => `linear-gradient(155deg, ${c[0]} 0%, ${c[1]} 100%)`
const emptyColors = ['#4a4a52', '#2e2e34']   // 无封面占位色
</script>
<template>
  <div class="media-page" ref="scroller">
    <!-- 图标 sprite -->
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-search" viewBox="0 0 24 24">
        <circle cx="11" cy="11" r="6.8" />
        <path d="M16.2 16.2 20.5 20.5" />
      </symbol>
      <symbol id="i-more" viewBox="0 0 24 24">
        <path d="M5.2 12h.02M12 12h.02M18.8 12h.02" stroke-width="2.6" />
      </symbol>
      <symbol id="i-play-circle" viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="8.6" />
        <path d="M10.2 8.6 15.6 12 10.2 15.4Z" />
      </symbol>
      <symbol id="i-chevron" viewBox="0 0 24 24">
        <path d="M9.5 5.5 16 12 9.5 18.5" />
      </symbol>
      <symbol id="i-star" viewBox="0 0 24 24">
        <path
          d="M12 3.6l2.57 5.2 5.73.83-4.15 4.05.98 5.71L12 16.67l-5.13 2.72.98-5.71-4.15-4.05 5.73-.83z"
        />
      </symbol>
      <symbol id="i-video" viewBox="0 0 24 24">
        <rect x="3" y="6" width="18" height="12" rx="2" />
        <path d="M10.2 9.4 15.6 12 10.2 14.6Z" />
      </symbol>
      <symbol id="i-back" viewBox="0 0 24 24">
        <path d="M15 5.5 8.5 12l6.5 6.5" />
      </symbol>
    </svg>
    <Transition name="main">
      <div class="main-view">
        <div class="hero-bar">
          <div class="hero-actions" :class="{ faded: searchOpen }">
            <button class="round-btn" type="button" aria-label="更多" @click="menuOpen = !menuOpen">
              <svg class="ic"><use href="#i-more" /></svg>
            </button>
          </div>
          <div class="menu-backdrop" v-if="menuOpen || ctxOpen" @click="menuOpen = false; ctxOpen = false"></div>
          <Transition name="menu">
            <div class="src-menu" v-if="menuOpen">
              <button class="src-menu-item" type="button" @click="scanAll">扫描文件源</button>
              <button class="src-menu-item" type="button" @click="onCacheSubtitles">
                {{ caching ? '停止缓存' : '缓存所有字幕' }}
              </button>
            </div>
          </Transition>
        </div>
        <!-- 顶部 hero：作品轮播 -->
        <header class="hero" v-if="heroItems.length" @mousedown.left="heroDown">
          <div class="hero-track" :style="heroTrackStyle">
            <div class="hero-slide" v-for="h in heroItems" :key="h.key" @click="openDetail(hero)">
              <img v-if="h.cover" class="hero-img" :src="h.cover" draggable="false" />
              <div v-else class="hero-img" :style="{ background: grad(emptyColors) }"></div>
            </div>
          </div>
          <div class="hero-info">
            <h1 class="hero-title">{{ hero.title }}</h1>
            <div class="hero-meta">
              <template v-if="hero.rating">
                <svg class="ic ic-star"><use href="#i-star" /></svg>
                <span>{{ hero.rating }}</span>
              </template>
              <template v-for="(m, i) in [hero.year, hero.country, hero.genres, hero.cert].filter(Boolean)" :key="m">
                <i v-if="i || hero.rating" class="sep"></i>
                <span :class="{ cert: m === hero.cert }">{{ m }}</span>
              </template>
            </div>
            <div class="dots">
              <i v-for="(h, i) in heroItems" :key="h.key" class="dot"
                 :class="{ on: i === heroIndex }" @click.stop="heroIndex = i"></i>
            </div>
          </div>
        </header>

        <!-- 继续观看 -->
        <section class="sec" v-if="continueRow.items.length">
          <div class="sec-head">
            <h2 class="sec-title">{{ continueRow.title }}</h2>
            <a class="sec-more" @click="openAll(continueRow.title)">查看全部 <svg class="ic ic-sm"><use href="#i-chevron" /></svg></a>
          </div>
          <div class="row" @mousedown.left="rowDrag">
            <article class="w-card" v-for="item in continueRow.items" :key="item.key"
                     @click="openContinue(item)" @contextmenu.prevent="openCtx($event, item)">
              <div class="w-art" :style="item.cover ? {} : { background: grad(emptyColors) }">
                <img v-if="item.cover" class="p-img" :src="item.cover" :alt="item.title" draggable="false" />
                <svg class="ic ic-lg w-play"><use href="#i-play-circle" /></svg>
                <div class="w-progress"><i :style="{ width: item.progress * 100 + '%' }"></i></div>
              </div>
              <h3 class="cap-title">{{ item.title }}</h3>
              <p class="cap-sub">{{ item.sub }}</p>
            </article>
          </div>
        </section>

        <!-- 海报行 -->
        <section class="sec" v-for="row in posterRows" :key="row.title">
          <div class="sec-head">
            <h2 class="sec-title">{{ row.title }}</h2>
            <a class="sec-more" @click="openAll(row.title)">查看全部 <svg class="ic ic-sm"><use href="#i-chevron" /></svg></a>
          </div>
          <div class="row" @mousedown.left="rowDrag">
            <article class="p-card" v-for="item in row.items" :key="item.key" @click="openDetail(item)">
              <div class="p-art" :style="item.cover ? {} : { background: grad(emptyColors) }">
                <img v-if="item.cover" class="p-img" :src="item.cover"  alt="item.title" draggable="false"/>
                <svg v-else class="ic ic-lg p-video"><use href="#i-video" /></svg>
                <span class="p-rate" v-if="item.rating">{{ item.rating }}</span>
              </div>
              <h3 class="cap-title">{{ item.title }}</h3>
              <p class="cap-sub">{{ item.sub }}</p>
            </article>
          </div>
        </section>
      </div>
    </Transition>

    <!-- 搜索：收起时就是右上角那个圆按钮（固定在它原来的位置），点开后滑成搜索条 -->
    <Transition name="fade">
      <div class="search-backdrop" v-if="searchOpen" @click="closeSearch"></div>
    </Transition>
    <div class="search-shell" :class="{ on: searchOpen }">
      <button class="search-face" type="button" aria-label="搜索" @click="onSearchFace">
        <svg class="ic"><use href="#i-search" /></svg>
      </button>
      <input v-if="searchOpen" ref="searchInput" class="search-input" v-model="q" placeholder="搜索作品"
             @keydown.esc="closeSearch" @keydown.enter="results[0] && openResult(results[0])" />
      <button v-if="searchOpen" class="search-cancel" type="button" @click="closeSearch">取消</button>
    </div>
    <Transition name="fade">
      <div class="search-panel" v-if="searchOpen && q.trim()">
        <p class="search-empty" v-if="!results.length">没有找到「{{ q.trim() }}」</p>
        <ul class="search-list" v-else>
          <li class="search-item" v-for="it in results" :key="it.key" @click="openResult(it)">
            <div class="search-poster">
              <img v-if="it.cover" :src="it.cover" :alt="it.title" draggable="false" />
              <svg v-else class="ic"><use href="#i-video" /></svg>
            </div>
            <div class="search-body">
              <h3 class="search-name">{{ it.title }}</h3>
              <p class="search-meta">{{ it.sub }}</p>
            </div>
            <span class="search-rate" v-if="it.rating">{{ it.rating }}</span>
          </li>
        </ul>
      </div>
    </Transition>

    <!-- 继续观看右键菜单：位置用鼠标坐标（同样按 --ui-scale 换算） -->
    <Transition name="menu">
      <div class="src-menu ctx-menu" ref="ctxMenuEl" v-if="ctxOpen" :style="{ left: ctxX + 'px', top: ctxY + 'px' }">
        <button class="src-menu-item danger" type="button" @click="onDeleteRecord">删除记录</button>
      </div>
    </Transition>

    <!-- 删除确认 -->
    <Transition name="confirm">
      <div class="confirm-backdrop" v-if="confirmOpen" @click="onCancelDelete">
        <div class="confirm-box" @click.stop>
          <h2 class="confirm-title">删除观看记录</h2>
          <p class="confirm-text">确定要删除「{{ ctxItem?.title }}」的观看记录吗？</p>
          <div class="confirm-actions">
            <button class="confirm-btn" type="button" @click="onCancelDelete">取消</button>
            <button class="confirm-btn danger" type="button" @click="onConfirmDelete">删除</button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
/* ── 基础 ───────────────────────────────────────────── */
.media-page {
  height: calc(100vh / var(--ui-scale));
  overflow-y: auto;
  scrollbar-width: none;
  padding: 0 0 110px;
  background: var(--bg);
  color: var(--ink);
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
  position: relative;
}
.media-page::-webkit-scrollbar { display: none; }
.sprite {
  position: absolute;
  width: 0;
  height: 0;
  overflow: hidden;
}

.ic {
  width: 20px;
  height: 20px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
  flex-shrink: 0;
}
.ic-sm {
  width: 14px;
  height: 14px;
}
.ic-lg {
  width: 34px;
  height: 34px;
}

/* ── hero ───────────────────────────────────────────── */
.hero {
  position: relative;
  aspect-ratio: 2.27;
  min-height: 210px;
  overflow: hidden;
}
.hero::before {
  /* 叠加光斑 */
  content: '';
  position: absolute;
  inset: 0;
  background:
    radial-gradient(38% 55% at 22% 38%, rgba(255, 214, 140, 0.55), transparent 70%),
    radial-gradient(45% 60% at 76% 26%, rgba(122, 88, 214, 0.5), transparent 72%),
    radial-gradient(60% 45% at 50% 100%, rgba(255, 236, 210, 0.35), transparent 75%);
}
.hero::after {
  /* 底部融入背景 */
  content: '';
  position: absolute;
  inset: auto 0 0;
  height: 32%;
  background: linear-gradient(180deg, transparent, var(--bg));
}

.hero-bar {
  position: sticky;
  top: 0;
  left: 0;
  right: 0;
  z-index: 20;
  height: 62px;
  margin-bottom: -62px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 14px;
}

.hero-actions {
  display: flex;
  gap: 10px;
  transition: opacity 0.2s ease;
}
.hero-actions.faded {          /* 搜索展开时让位给搜索条 */
  opacity: 0;
  pointer-events: none;
}
.round-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  color: var(--ink);
  background: var(--glass-bg);   /* 玻璃底色：半透明，深浅模式通用 */
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);  /* 玻璃棱边 */
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),    /* 顶部高光边（折射光） */
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),   /* 底部微反光 */
      0 4px 16px rgba(40, 25, 30, 0.25);
}

/* ── 右上角菜单：样式对齐文件源页面的添加菜单 ─────────── */
.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 18;              /* 比按钮低、比内容高 */
}

.src-menu {
  position: fixed;
  top: 56px;                /* 按钮 top14 + 高34 + 间距8 */
  right: 14px;              /* 与按钮右对齐 */
  z-index: 22;
  min-width: 168px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  border-radius: 16px;
  background: var(--glass-bg);
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),
      0 4px 16px rgba(40, 25, 30, 0.25);
}

.src-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  font-size: 13px;
  color: var(--ink);
  cursor: pointer;
  transition: background 0.15s ease;
}
.src-menu-item:hover {
  background: var(--option-bg);
}

/* 弹出动画：从按钮右上角弹出 */
.menu-enter-active,
.menu-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
  transform-origin: top right;
}
.menu-enter-from,
.menu-leave-to {
  opacity: 0;
  transform: scale(0.92) translateY(-6px);
}

/* 右键菜单：位置由鼠标坐标给，清掉默认的右上角定位 */
.ctx-menu {
  top: auto;
  right: auto;
}
.src-menu-item.danger {
  color: #e5484d;
}
.src-menu-item.danger:hover {
  background: rgba(229, 72, 77, 0.16);
}

/* ── 确认弹窗：样式对齐文件源页面 ───────────────────── */
.confirm-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;                       /* 盖住菜单(22)和滚动条(30)，模态就该全盖 */
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(20, 16, 18, 0.28); /* 压暗背景突出弹窗 */
}
.confirm-box {
  width: min(320px, calc(100% - 48px));
  padding: 18px 18px 14px;
  border-radius: 18px;
  background: var(--glass-bg);
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),
      0 4px 16px rgba(40, 25, 30, 0.25);
}
.confirm-title {
  margin: 0 0 8px;
  font-size: 15px;
  font-weight: 700;
  color: var(--ink);
}
.confirm-text {
  margin: 0 0 16px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--sub);
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.confirm-btn {
  padding: 8px 16px;
  border: 0;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.18);
  font-size: 12px;
  color: var(--ink);
  cursor: pointer;
  transition: background 0.15s ease;
}
.confirm-btn:hover {
  background: rgba(255, 255, 255, 0.28);
}
.confirm-btn.danger {
  background: #e5484d;
  color: #ffffff;
}
.confirm-btn.danger:hover {
  background: #f2555a;
}
.confirm-enter-active,
.confirm-leave-active {
  transition: opacity 0.18s ease;
}
.confirm-enter-from,
.confirm-leave-to {
  opacity: 0;
}

/* ── 搜索：圆按钮 ⇄ 搜索条 ──────────────────────────── */
.search-backdrop {
  position: fixed;
  inset: 0;
  z-index: 34;              /* 盖住内容、胶囊栏(25)和滚动条(30)，低于确认弹窗(40) */
  background: rgba(20, 16, 18, 0.18);
  backdrop-filter: blur(14px) saturate(160%);
  -webkit-backdrop-filter: blur(14px) saturate(160%);
}
.search-shell {
  position: fixed;
  top: 14px;
  height: 34px;
  z-index: 38;
  /* 收起时正好占右上角搜索按钮那一格：右边 14 + 更多按钮 34 + 间距 10 = 58 */
  left: calc(100% - 92px);
  right: 58px;
  display: flex;
  align-items: center;
  overflow: hidden;
  border-radius: 999px;
  background: var(--glass-bg);
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),
      0 4px 16px rgba(40, 25, 30, 0.25);
  transition: left 0.3s cubic-bezier(0.2, 0.8, 0.2, 1),
              right 0.3s cubic-bezier(0.2, 0.8, 0.2, 1),
              border-radius 0.3s ease;
}
.search-shell.on {
  left: calc(0% + 14px);    /* 写成 calc 是为了和收起态的 calc(100% - 92px) 分量插值 */
  right: 14px;
  border-radius: 14px;
}
.search-face {
  flex-shrink: 0;
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 0;
  background: transparent;
  color: var(--ink);
  cursor: pointer;
}
.search-input {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  background: transparent;
  font-size: 13px;
  color: var(--ink);
  outline: none;
}
.search-cancel {
  flex-shrink: 0;
  padding: 0 12px;
  border: 0;
  background: transparent;
  font-size: 12.5px;
  color: var(--sub);
  cursor: pointer;
}
.search-cancel:hover { color: var(--ink); }

.search-panel {
  position: fixed;
  top: 58px;                /* 搜索条底边(14+34) + 10 */
  left: 14px;
  right: 14px;
  z-index: 36;
  max-height: calc(100vh / var(--ui-scale) - 120px);
  overflow-y: auto;
  scrollbar-width: none;
  padding: 6px;
  border-radius: 16px;
  background: var(--glass-bg);
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),
      0 4px 16px rgba(40, 25, 30, 0.25);
}
.search-panel::-webkit-scrollbar { display: none; }
.search-empty {
  margin: 0;
  padding: 14px 12px;
  font-size: 12.5px;
  color: var(--sub);
  text-align: center;
}
.search-list { margin: 0; padding: 0; list-style: none; }
.search-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 8px;
  border-radius: 12px;
  cursor: pointer;
  transition: background 0.15s ease;
}
.search-item:hover { background: var(--option-bg); }
.search-poster {
  flex-shrink: 0;
  width: 30px;
  aspect-ratio: 2 / 3;
  border-radius: 5px;
  overflow: hidden;
  background: rgba(127, 127, 127, 0.12);
  display: flex; align-items: center; justify-content: center;
  color: rgba(255, 255, 255, 0.55);
}
.search-poster img { width: 100%; height: 100%; object-fit: cover; display: block; }
.search-body { flex: 1; min-width: 0; }
.search-name {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--ink);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.search-meta {
  margin: 2px 0 0;
  font-size: 11px;
  color: var(--sub);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.search-rate {
  flex-shrink: 0;
  font-size: 11.5px;
  font-weight: 600;
  color: #f2c14e;
}

.fade-enter-active,
.fade-leave-active { transition: opacity 0.2s ease; }
.fade-enter-from,
.fade-leave-to { opacity: 0; }

.hero-info {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 6px;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}

.hero-title {
  margin: 6px 0 8px;
  font-size: clamp(28px, 4vw, 44px);
  font-weight: 800;
  letter-spacing: 2px;
  -webkit-background-clip: text;
  background-clip: text;
  color: var(--hero-title);
  filter: drop-shadow(0 2px 3px rgba(60, 30, 10, 0.4));
}

.hero-meta {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 6px;
  padding: 0 16px;
  font-size: 13px;
  color: var(--hero-meta);
}
.hero-meta .ic-star {
  width: 15px;
  height: 15px;
  color: #f2c14e;
  fill: #f2c14e;
  stroke: none;
}
.hero-track {
  display: flex;
  height: 100%;
  will-change: transform;
}
.hero-slide {
  position: relative;
  flex: 0 0 100%;
  height: 100%;
}
.hero-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.sep {
  width: 1px;
  height: 11px;
  background: rgba(255, 255, 255, 0.5);
}

.dots {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.45);
}
.dot.on {
  background: #ffffff;
}

/* ── 通用 section ───────────────────────────────────── */
.sec {
  margin-top: 20px;
}
.sec-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  padding: 0 14px 10px;
}
.sec-title {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  color: var(--ink);
}
.sec-more {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 12px;
  color: var(--ink);
  cursor: pointer;
}

.row {
  display: flex;
  gap: 10px;
  padding: 8px 14px;
  margin: -8px 0;
  overflow-x: auto;
  scrollbar-width: none;
  user-select: none;
}
.row.dragging .p-card:hover,
.row.dragging .p-card:hover .p-art,
.row.dragging .w-card:hover,
.row.dragging .w-card:hover .w-art {
  transform: none;
}
.row::-webkit-scrollbar {
  display: none;
}

.cap-title {
  margin: 6px 0 0;
  font-size: 11px;
  font-weight: 600;
  line-height: 1.35;
  color: var(--ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cap-sub {
  margin: 2px 0 0;
  font-size: 9.5px;
  line-height: 1.35;
  color: var(--sub);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── 海报卡 ─────────────────────────────────────────── */
.p-card {
  flex-shrink: 0;
  width: clamp(76px, 8.3vw, 92px);
  transition: transform 0.18s ease;
}
.p-card:hover {
  transform: translateY(-4px);        /* 悬停轻微上浮 */
}
.p-art {
  position: relative;
  width: 100%;
  aspect-ratio: 2 / 3;
  border-radius: 10px;
  box-shadow: 0 1px 4px rgba(40, 25, 30, 0.22);
  transition: transform 0.18s ease, box-shadow 0.18s ease, filter 0.18s ease;
}
.p-card:hover .p-art {
  transform: scale(1.04);             /* 海报轻微放大 */
  box-shadow: 0 8px 20px rgba(40, 25, 30, 0.35);   /* 阴影加深 */
  filter: brightness(1.06);           /* 提亮一点 */
}
.p-rate {
  position: absolute;
  right: 4px;
  bottom: 4px;
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.72);
  font-size: 9.5px;
  font-weight: 600;
  color: #ffffff;
  letter-spacing: 0.2px;
}

/* ── 继续观看卡 ─────────────────────────────────────── */
.w-card {
  flex-shrink: 0;
  width: clamp(180px, 19.5vw, 212px);
  transition: transform 0.18s ease;
}
.w-card:hover {
  transform: translateY(-4px);        /* 悬停轻微上浮 */
}
.w-art {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  border-radius: 10px;
  box-shadow: 0 1px 4px rgba(40, 25, 30, 0.22);
  transition: transform 0.18s ease, box-shadow 0.18s ease, filter 0.18s ease;
}
.w-card:hover .w-art {
  transform: scale(1.04);             /* 封面轻微放大 */
  box-shadow: 0 8px 20px rgba(40, 25, 30, 0.35);
  filter: brightness(1.06);
}
.w-play {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 38px;
  height: 38px;
  color: rgba(255, 255, 255, 0.92);
}
.w-progress {
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: 7px;
  height: 3px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.35);
  overflow: hidden;
}
.w-progress i {
  display: block;
  height: 100%;
  border-radius: 2px;
  background: #ffffff;
}
.p-img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  border-radius: 10px;
}
.p-video {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 34px;
  height: 34px;
  color: rgba(255, 255, 255, 0.55);
}
.cert {
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(127, 127, 127, 0.18);
  font-size: 10.5px;
}
</style>
