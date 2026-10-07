<script setup>
import { ref, computed, onMounted, nextTick } from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { useRoute, useRouter } from 'vue-router'
import { usePageScroller } from './scroll.js'
import {getWork} from "./data.js";

const route = useRoute()
const router = useRouter()
const scroller = usePageScroller()

const detail = ref(null)
const season = ref(1)         // 当前季
const epChip = ref(0)         // 当前范围芯片
const seasonOpen = ref(false) // 季下拉

onMounted(async () => {
  try {
    detail.value = await getWork(route.params.tmdb, route.params.kind)
    season.value = detail.value.episodes[0]?.season ?? 1
  } catch (e) {
    console.error('加载详情失败', e)
  }
  nextTick(() => window.dispatchEvent(new Event('resize')))
})

function onBack() {
  router.back()
}

// 拖拽平移（与媒体库同款）
function rowDrag(e) {
  const el = e.currentTarget
  const startX = e.clientX
  const startLeft = el.scrollLeft
  el.classList.add('dragging')
  function onMove(ev) { el.scrollLeft = startLeft - (ev.clientX - startX) }
  function onUp() {
    el.classList.remove('dragging')
    window.removeEventListener('mousemove', onMove)
    window.removeEventListener('mouseup', onUp)
  }
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mouseup', onUp)
}

// 分集按季分组、每 10 个一片
const seasons = computed(() => [...new Set((detail.value?.episodes || []).map(e => e.season))].sort((a, b) => a - b))
const seasonEps = computed(() => (detail.value?.episodes || []).filter(e => e.season === season.value))
const epChips = computed(() => Array.from({ length: Math.max(1, Math.ceil(seasonEps.value.length / 10)) }, (_, i) => i))
const epShow = computed(() => seasonEps.value.slice(epChip.value * 10, epChip.value * 10 + 10))

function fmtDate(s) {
  if (!s) return ''
  const [y, m, d] = s.split('-').map(Number)
  return `${y}/${m}/${d}`
}
function fmtSize(n) {
  if (n >= 1073741824) return (n / 1073741824).toFixed(2) + ' GB'
  if (n >= 1048576) return (n / 1048576).toFixed(1) + ' MB'
  if (n >= 1024) return (n / 1024).toFixed(0) + ' KB'
  return n + ' B'
}
const detailMeta = computed(() => {
  const d = detail.value
  if (!d) return []
  return [fmtDate(d.air_date), d.region, (d.genres || []).join(','), d.certification].filter(Boolean)
})
const detailSub = computed(() => {
  const d = detail.value
  if (!d) return ''
  return d.kind === 'tv'
      ? `共${d.total_episodes ?? '?'}集 (找到${d.episodes.length}集)`
      : fmtSize(d.total_size)
})

const grad = (c) => `linear-gradient(155deg, ${c[0]} 0%, ${c[1]} 100%)`
const emptyColors = ['#4a4a52', '#2e2e34']
</script>

<template>
  <div class="detail-page" ref="scroller">
    <!-- 图标 sprite -->
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-back" viewBox="0 0 24 24"><path d="M15 5.5 8.5 12l6.5 6.5" /></symbol>
      <symbol id="i-more" viewBox="0 0 24 24"><path d="M5.2 12h.02M12 12h.02M18.8 12h.02" stroke-width="2.6" /></symbol>
      <symbol id="i-star" viewBox="0 0 24 24"><path d="M12 3.6l2.57 5.2 5.73.83-4.15 4.05.98 5.71L12 16.67l-5.13 2.72.98-5.71-4.15-4.05 5.73-.83z" /></symbol>
      <symbol id="i-chevron" viewBox="0 0 24 24"><path d="M9.5 5.5 16 12 9.5 18.5" /></symbol>
      <symbol id="i-play" viewBox="0 0 24 24"><path d="M8 5.5 18 12 8 18.5Z" /></symbol>
      <symbol id="i-heart" viewBox="0 0 24 24"><path d="M12 20s-7-4.35-7-9a4 4 0 0 1 7-2.6A4 4 0 0 1 19 11c0 4.65-7 9-7 9z" /></symbol>
      <symbol id="i-list-add" viewBox="0 0 24 24"><path d="M4 7h12M4 12h8M4 17h8M18 13v6M15 16h6" /></symbol>
      <symbol id="i-flag" viewBox="0 0 24 24"><path d="M6 21V4m0 1h11l-2.2 3.5L17 12H6" /></symbol>
    </svg>

    <div class="d-bar">
      <button class="round-btn" type="button" aria-label="返回" @click="onBack">
        <svg class="ic"><use href="#i-back" /></svg>
      </button>
      <button class="round-btn" type="button" aria-label="更多">
        <svg class="ic"><use href="#i-more" /></svg>
      </button>
    </div>

    <!-- 背景图：缓存的 backdrop，缺图降级渐变 -->
    <template v-if="detail">
    <div class="d-hero">
      <img v-if="detail.backdrop" class="d-bg" :src="convertFileSrc(detail.backdrop)" draggable="false" />
      <div v-else class="d-bg" :style="{ background: grad(emptyColors) }"></div>
      <div class="d-hero-fade"></div>
      <!-- 标题 + 元信息 + 操作 -->
      <div class="d-head">
        <div class="d-info">
          <h1 class="d-title">{{ detail.title }}</h1>
          <div class="d-meta">
            <template v-if="detail.rating">
              <svg class="ic ic-star"><use href="#i-star" /></svg>
              <span>{{ detail.rating.toFixed(1) }}</span>
            </template>
            <template v-for="(m, i) in detailMeta" :key="m">
              <i v-if="i || detail.rating" class="sep"></i>
              <span :class="{ cert: m === detail.certification }">{{ m }}</span>
            </template>
          </div>
          <p class="d-sub">{{ detailSub }}</p>
        </div>
        <div class="d-actions">
          <button class="d-play" type="button"><!-- TODO: 播放 -->
            <svg class="ic"><use href="#i-play" /></svg><span>播放</span>
          </button>
          <div class="d-icons">
            <button class="round-btn" type="button" aria-label="收藏"><svg class="ic"><use href="#i-heart" /></svg></button>
            <button class="round-btn" type="button" aria-label="加入列表"><svg class="ic"><use href="#i-list-add" /></svg></button>
            <button class="round-btn" type="button" aria-label="标记"><svg class="ic"><use href="#i-flag" /></svg></button>
          </div>
        </div>
      </div>
    </div>

    <!-- 剧集（仅电视剧）：季下拉 + 分集横滑 + 范围芯片 -->
    <template v-if="detail.kind === 'tv'">
      <div class="d-sec-bar">
        <div class="d-season">
          <button class="d-season-btn" type="button" @click="seasonOpen = !seasonOpen">
            第{{ season }}季 <svg class="ic ic-sm"><use href="#i-chevron" /></svg>
          </button>
          <Transition name="menu">
            <div class="d-menu" v-if="seasonOpen">
              <button class="d-menu-item" type="button" v-for="s in seasons" :key="s"
                      @click="season = s; epChip = 0; seasonOpen = false">第{{ s }}季</button>
            </div>
          </Transition>
        </div>
      </div>
      <div class="row d-eps" @mousedown.left="rowDrag">
        <article class="e-card" v-for="ep in epShow" :key="ep.file_id">
          <div class="e-art">
            <img v-if="ep.still" class="e-img" :src="convertFileSrc(ep.still)" draggable="false" />
            <span v-else class="e-num">{{ ep.episode }}</span>
          </div>
          <p class="cap-title">{{ ep.episode }}. {{ ep.title }}</p>
        </article>
      </div>
      <div class="d-chips">
        <button class="chip" type="button" v-for="c in epChips" :key="c"
                :class="{ on: c === epChip }" @click="epChip = c">
          {{ c * 10 + 1 }}-{{ Math.min((c + 1) * 10, seasonEps.length) }}
        </button>
      </div>
    </template>

    <!-- 演职人员（电影/剧都有） -->
    <section class="sec" v-if="detail.cast.length">
      <div class="sec-head"><h2 class="sec-title">演职人员</h2></div>
      <div class="row" @mousedown.left="rowDrag">
        <article class="c-card" v-for="(p, i) in detail.cast" :key="i">
          <div class="c-art">
            <img v-if="p.photo" class="c-img" :src="convertFileSrc(p.photo)" draggable="false" />
            <span v-else class="c-char">{{ p.name.slice(0, 1) }}</span>
          </div>
          <p class="cap-title">{{ p.name }}</p>
          <p class="cap-sub">{{ p.character }}</p>
        </article>
      </div>
    </section>
    </template>
  </div>
</template>

<style scoped>
/* ── 基础（自包含，不依赖 MediaLibrary 的样式）────────── */
.detail-page {
  height: calc(100vh / var(--ui-scale));
  overflow-y: auto;
  scrollbar-width: none;
  padding-bottom: 24px;
  background: var(--bg);
  color: var(--ink);
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
  -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}
.detail-page::-webkit-scrollbar { display: none; }
.sprite { position: absolute; width: 0; height: 0; overflow: hidden; }
.ic {
  width: 20px; height: 20px;
  fill: none; stroke: currentColor; stroke-width: 2;
  stroke-linecap: round; stroke-linejoin: round; flex-shrink: 0;
}
.ic-sm { width: 14px; height: 14px; }
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
.sep { width: 1px; height: 11px; background: rgba(127, 127, 127, 0.45); }
.cert {
  padding: 1px 5px; border-radius: 4px;
  background: rgba(127, 127, 127, 0.18); font-size: 10.5px;
}
.row {
  display: flex; gap: 10px; padding: 8px 14px; margin: -8px 0;
  overflow-x: auto; scrollbar-width: none;
  user-select: none;
}
.row::-webkit-scrollbar { display: none; }
.sec { margin-top: 20px; }
.sec-head {
  display: flex; align-items: baseline; justify-content: space-between;
  padding: 0 14px 10px;
}
.sec-title { margin: 0; font-size: 20px; font-weight: 700; color: var(--ink); }
.cap-title {
  margin: 6px 0 0; font-size: 11px; font-weight: 600; line-height: 1.35;
  color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.cap-sub {
  margin: 2px 0 0; font-size: 9.5px; line-height: 1.35;
  color: var(--sub); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}

/* ── 详情布局 ───────────────────────────────────────── */
.d-bar {
  position: sticky; top: 0; z-index: 20;height: 58px;
  margin-bottom: -58px;
  align-items: center;
  display: flex; justify-content: space-between;
  padding: 12px 14px; background: none;
}
.d-hero { position: relative; width: 100%; aspect-ratio: 2.2; overflow: hidden; }
.d-bg { width: 100%; height: 100%; object-fit: cover; display: block; }
.d-hero-fade {
  position: absolute;
  inset: auto 0 0;
  height: 55%;
  background: linear-gradient(180deg, transparent, var(--bg));
  pointer-events: none;
}
.d-head {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 8px;
  z-index: 2;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 0 14px;
}
.d-title { margin: 0; font-size: 26px; font-weight: 800; color: var(--ink); }
.d-meta {
  display: flex; align-items: center; flex-wrap: wrap; gap: 6px;
  margin-top: 8px; font-size: 12px; color: var(--ink);
}
.d-meta .ic-star {
  width: 13px; height: 13px; color: #f2c14e; fill: #f2c14e; stroke: none;
}
.d-sub { margin: 6px 0 0; font-size: 11px; color: var(--sub); }
.d-actions { display: flex; flex-direction: column; align-items: center; gap: 12px; }
.d-play {
  display: inline-flex; align-items: center; justify-content: center; gap: 8px;
  width: clamp(180px, 24vw, 300px); height: 44px;
  border: none; border-radius: 22px;
  background: rgba(255, 255, 255, 0.82); color: #1c1c1e;
  font-size: 15px; font-weight: 600; cursor: pointer;
}
.d-icons { display: flex; gap: 14px; }

/* ── 分集 ───────────────────────────────────────────── */
.d-sec-bar {
  display: flex; align-items: center; justify-content: space-between;
  padding: 18px 14px 10px;
}
.d-season { position: relative; }
.d-season-btn {
  display: inline-flex; align-items: center; gap: 4px;
  border: none; background: none;
  font-size: 16px; font-weight: 700; color: var(--ink); cursor: pointer;
}
.d-menu {
  position: absolute; top: 110%; left: 0; z-index: 30;
  min-width: 100px; padding: 6px;
  border-radius: 12px; background: var(--glass-bg);
  backdrop-filter: blur(10px); -webkit-backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow: 0 8px 24px rgba(40, 25, 30, 0.3);
}
.d-menu-item {
  display: block; width: 100%; padding: 7px 12px;
  border: none; border-radius: 8px; background: transparent;
  font-size: 13px; color: var(--ink); text-align: left; cursor: pointer;
}
.d-menu-item:hover { background: var(--option-bg); }
.menu-enter-active, .menu-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.menu-enter-from, .menu-leave-to { opacity: 0; transform: scale(0.95) translateY(-4px); }

.d-eps { padding-top: -4px; }
.e-card {
  flex-shrink: 0;
  width: clamp(150px, 17vw, 190px);
  transition: transform 0.18s ease;
}
.e-card:hover {
  transform: translateY(-4px);        /* 悬停轻微上浮 */
}
.e-art {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  border-radius: 10px;
  overflow: hidden;
  background: rgba(127, 127, 127, 0.14);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.18s ease, box-shadow 0.18s ease, filter 0.18s ease;
}
.e-card:hover .e-art {
  transform: scale(1.04);
  box-shadow: 0 8px 20px rgba(40, 25, 30, 0.35);
  filter: brightness(1.06);
}
.row.dragging .e-card:hover,
.row.dragging .e-card:hover .e-art {
  transform: none;
}
.e-img { width: 100%; height: 100%; object-fit: cover; display: block; }
.e-num { font-size: 20px; font-weight: 700; color: var(--ink); }
.d-chips { display: flex; gap: 10px; padding: 12px 14px; }
.chip {
  padding: 5px 14px; border: 1px solid rgba(127, 127, 127, 0.35);
  border-radius: 16px; background: none;
  font-size: 11px; color: var(--ink); cursor: pointer;
}
.chip.on { background: var(--ink); color: var(--bg); border-color: var(--ink); }

/* ── 演职人员 ───────────────────────────────────────── */
.c-card {
  flex-shrink: 0;
  width: clamp(84px, 9vw, 100px);
  transition: transform 0.18s ease;
}
.c-card:hover {
  transform: translateY(-4px);        /* 悬停轻微上浮 */
}
.c-art {
  position: relative;
  width: 100%;
  aspect-ratio: 1;
  border-radius: 12px;
  overflow: hidden;
  background: rgba(127, 127, 127, 0.14);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.18s ease, box-shadow 0.18s ease, filter 0.18s ease;
}
.c-card:hover .c-art {
  transform: scale(1.04);
  box-shadow: 0 8px 20px rgba(40, 25, 30, 0.35);
  filter: brightness(1.06);
}
.row.dragging .c-card:hover,
.row.dragging .c-card:hover .c-art {
  transform: none;
}
.c-img { width: 100%; height: 100%; object-fit: cover; display: block; }
.c-char { font-size: 26px; font-weight: 700; color: var(--ink); }
.c-card .cap-sub { white-space: normal; }
</style>