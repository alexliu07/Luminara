<script setup>
import {ref, computed, onMounted, nextTick} from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { useRouter } from 'vue-router'
import { usePageScroller } from './scroll.js'
import {getUnscraped, getWork, getWorks} from "./data.js";

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
// ── 样例数据 ─────────────────────────────────────────────

const continueRow = {
  title: '继续观看',
  items: [
    {
      title: '不时用俄语小声说真心话的邻桌艾莉…',
      sub: '第1季 第12集',
      colors: ['#6d7fa8', '#c7d2e8'],
      progress: 0.45,
    },
    {
      title: '想吃掉我的非人少女',
      sub: '第1季 第5集',
      colors: ['#8a6f9e', '#e2c4d8'],
      progress: 0.2,
    },
  ],
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
          <div class="hero-actions">
            <button class="round-btn" type="button" aria-label="搜索">
              <svg class="ic"><use href="#i-search" /></svg>
            </button>
            <button class="round-btn" type="button" aria-label="更多">
              <svg class="ic"><use href="#i-more" /></svg>
            </button>
          </div>
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
        <section class="sec">
          <div class="sec-head">
            <h2 class="sec-title">{{ continueRow.title }}</h2>
            <a class="sec-more">查看全部 <svg class="ic ic-sm"><use href="#i-chevron" /></svg></a>
          </div>
          <div class="row" @mousedown.left="rowDrag">
            <article class="w-card" v-for="item in continueRow.items" :key="item.title">
              <div class="w-art" :style="{ background: grad(item.colors) }">
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
