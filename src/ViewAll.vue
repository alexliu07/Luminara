<script setup>
import {ref, computed, onMounted, nextTick} from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useRoute, useRouter } from 'vue-router'
import { usePageScroller } from './scroll.js'
import {getUnscraped, getWork, getWorks} from "./data.js";

const route = useRoute()
const router = useRouter()
const scroller = usePageScroller()
const category = computed(() => decodeURIComponent(route.params.category))

const works = ref([])
const unscraped = ref([])

onMounted(async () => {
  try {
    works.value = await getWorks()
    unscraped.value = await getUnscraped()
  } catch (e) {
    console.error('加载列表失败', e)
  }
  nextTick(() => window.dispatchEvent(new Event('resize')))
})

function onBack() {
  router.back()
}
function openDetail(w) {
  if (!w.tmdb) return
  getWork(w.tmdb, w.kind)
  router.push(`/work/${w.kind}/${w.tmdb}`)
}

const items = computed(() => {
  const toRow = (w) => ({
    key: 'w' + w.kind + w.tmdb_id,
    tmdb: w.tmdb_id,
    kind: w.kind,
    title: w.title,
    cover: w.poster ? convertFileSrc(w.poster) : null,
    rating: w.rating ? w.rating.toFixed(1) : '',
    date: w.air_date || '',
    seasons: w.kind === 'tv' ? `共${w.total_seasons ?? '?'}季` : '',
    country: w.region || '',
    cert: w.certification || '',
    overview: w.overview || '',
  })
  switch (category.value) {
    case '电影': return works.value.filter(w => w.kind === 'movie').map(toRow)
    case '电视剧': return works.value.filter(w => w.kind === 'tv').map(toRow)
    case '动漫': return works.value.filter(w => (w.genres || []).includes('动画')).map(toRow)
    case '未刮削': return unscraped.value.map(u => ({
      key: 'f' + u.rel_path, tmdb: 0, title: u.name, cover: null,
      rating: '', date: '', seasons: '', country: '', cert: '', overview: u.rel_path,
    }))
    default: return []
  }
})
</script>

<template>
  <div class="all-page" ref="scroller">
    <!-- 图标 sprite -->
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-back" viewBox="0 0 24 24"><path d="M15 5.5 8.5 12l6.5 6.5" /></symbol>
      <symbol id="i-star" viewBox="0 0 24 24"><path d="M12 3.6l2.57 5.2 5.73.83-4.15 4.05.98 5.71L12 16.67l-5.13 2.72.98-5.71-4.15-4.05 5.73-.83z" /></symbol>
      <symbol id="i-video" viewBox="0 0 24 24"><rect x="3" y="6" width="18" height="12" rx="2" /><path d="M10.2 9.4 15.6 12 10.2 14.6Z" /></symbol>
    </svg>

    <!-- 返回键固定，标题随页滚动居中 -->
    <button class="round-btn all-back" type="button" aria-label="返回" @click="onBack">
      <svg class="ic"><use href="#i-back" /></svg>
    </button>
    <div class="all-bar">
      <h1 class="all-title">{{ category }}</h1>
    </div>

    <ul class="all-list">
      <li class="all-item" v-for="it in items" :key="it.key" @click="openDetail(it)">
        <div class="all-poster">
          <img v-if="it.cover" class="all-img" :src="it.cover" draggable="false" />
          <svg v-else class="ic p-video"><use href="#i-video" /></svg>
        </div>
        <div class="all-body">
          <h3 class="all-name">{{ it.title }}</h3>
          <div class="all-meta">
            <template v-if="it.rating">
              <svg class="ic ic-star"><use href="#i-star" /></svg>
              <span>{{ it.rating }}</span>
            </template>
            <template v-for="(m, i) in [it.date, it.seasons, it.country, it.cert].filter(Boolean)" :key="m">
              <i v-if="i || it.rating" class="sep"></i>
              <span :class="{ cert: m === it.cert }">{{ m }}</span>
            </template>
          </div>
          <p class="all-desc">{{ it.overview }}</p>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.all-page {
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
.all-page::-webkit-scrollbar { display: none; }
.sprite { position: absolute; width: 0; height: 0; overflow: hidden; }
.ic {
  width: 20px; height: 20px;
  fill: none; stroke: currentColor; stroke-width: 2;
  stroke-linecap: round; stroke-linejoin: round; flex-shrink: 0;
}
.ic-star { width: 13px; height: 13px; color: #f2c14e; fill: #f2c14e; stroke: none; }
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
.all-back {
  position: fixed;
  top: 12px;
  left: 14px;
  z-index: 20;
}
.all-bar {
  padding: 12px 14px 10px;
  text-align: center;
}
.all-title {
  margin: 0;
  padding: 0 44px;          /* 两侧避开返回键 */
  font-size: 17px;
  font-weight: 700;
  color: var(--ink);
}
.all-list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.all-item {
  display: flex;
  cursor: default;
  gap: 14px;
  padding: 16px 14px;
  border-bottom: 1px solid rgba(127, 127, 127, 0.18);
}
.all-poster {
  position: relative;
  flex-shrink: 0;
  width: 54px;
  aspect-ratio: 2 / 3;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(127, 127, 127, 0.12);
  display: flex; align-items: center; justify-content: center;
  color: rgba(255, 255, 255, 0.55);
}
.all-img { width: 100%; height: 100%; object-fit: cover; display: block; }
.all-body { min-width: 0; flex: 1; }
.all-name { margin: 0; font-size: 15px; font-weight: 700; color: var(--ink); }
.all-meta {
  display: flex; align-items: center; flex-wrap: wrap; gap: 6px;
  margin-top: 5px; font-size: 11.5px; color: var(--ink);
}
.sep { width: 1px; height: 11px; background: rgba(127, 127, 127, 0.45); }
.cert {
  padding: 1px 5px; border-radius: 4px;
  background: rgba(127, 127, 127, 0.18); font-size: 10.5px;
}
.all-desc {
  margin: 7px 0 0;
  font-size: 12px;
  line-height: 1.55;
  color: var(--sub);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>