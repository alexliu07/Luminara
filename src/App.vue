<script setup>
import { ref, onMounted, onBeforeUnmount, nextTick } from 'vue'
import MediaLibrary from './MediaLibrary.vue'
import FileSources from './FileSources.vue'
import Settings from './Settings.vue'

// tab id → 页面组件
const pages = {
  library: MediaLibrary,
  sources: FileSources,
  settings: Settings,
}

const tabs = [
  { id: 'library', label: '媒体库', icon: 'library' },
  { id: 'sources', label: '文件源', icon: 'folder' },
  { id: 'settings', label: '设置', icon: 'gear' },
]

const current = ref('library')
const trackEl = ref(null)
const thumbEl = ref(null)
const tabbarEl = ref(null)
const indicatorEl = ref(null)

function updateScrollbar() {
  const doc = document.documentElement
  const track = trackEl.value
  const thumb = thumbEl.value
  if (!track || !thumb) return
  const scrollable = doc.scrollHeight - doc.clientHeight
  if (scrollable <= 0) {
    thumb.style.opacity = '0'   // 内容不滚动就隐藏滑块
    return
  }
  thumb.style.opacity = '1'
  const trackH = track.clientHeight
  const thumbH = Math.max(30, (trackH * doc.clientHeight) / doc.scrollHeight)
  const top = (doc.scrollTop / scrollable) * (trackH - thumbH)
  thumb.style.height = thumbH + 'px'
  thumb.style.transform = `translateY(${top}px)`
}

function uiScale() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-scale')) || 1
}

function onThumbDown(e) {
  const doc = document.documentElement
  const trackH = trackEl.value.clientHeight
  const thumbH = thumbEl.value.clientHeight
  const scrollable = doc.scrollHeight - doc.clientHeight
  if (scrollable <= 0 || trackH <= thumbH) return   // 不可滚动就不拖

  const startY = e.clientY            // 记下起点（视口坐标，未缩放）
  const startScroll = doc.scrollTop

  function onMove(ev) {
    // 鼠标位移 ÷ 缩放倍率 = 布局 px，再按比例换算成滚动距离
    const dy = (ev.clientY - startY) / uiScale()
    doc.scrollTop = startScroll + dy * (scrollable / (trackH - thumbH))
  }
  function onUp() {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
  }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
}

onMounted(() => {
  window.addEventListener('scroll', updateScrollbar, { passive: true })
  window.addEventListener('resize', updateScrollbar)
  window.addEventListener('resize', updateIndicator)
  updateScrollbar()
  nextTick(updateIndicator)
})
onBeforeUnmount(() => {
  window.removeEventListener('scroll', updateScrollbar)
  window.removeEventListener('resize', updateScrollbar)
})

function switchTab(id) {
  current.value = id
  window.scrollTo(0, 0) // 切换页面后回到顶部
  nextTick(updateScrollbar);
  nextTick(updateIndicator);
}

function updateIndicator() {
  const bar = tabbarEl.value
  const ind = indicatorEl.value
  if (!bar || !ind) return
  const els = bar.querySelectorAll('.tab')
  const i = tabs.findIndex((t) => t.id === current.value)
  const el = els[i]
  if (!el) return
  const padX = 12 // 胶囊比 tab 窄一点，做出内边距
  const padY = 7
  ind.style.width = el.offsetWidth - padX * 2 + 'px'
  ind.style.transform = `translate(${el.offsetLeft + padX}px, ${el.offsetTop + padY}px)`
}

</script>

<template>
  <main class="app">
    <!-- 自定义滚动条 -->
    <div class="scrollbar" ref="trackEl">
      <div class="scrollbar-thumb" ref="thumbEl" @pointerdown.prevent="onThumbDown"></div>
    </div>
    <!-- 页面容器：随底部栏切换，KeepAlive 保留各页状态 -->
    <KeepAlive>
      <component :is="pages[current]" />
    </KeepAlive>

    <!-- 底部主导航 -->
    <nav class="tabbar" ref="tabbarEl">
      <!-- 图标 sprite -->
      <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
        <symbol id="tb-library" viewBox="0 0 24 24">
          <rect x="3.5" y="4.5" width="17" height="15" rx="3" />
          <path d="M10.3 9.1 14.8 12 10.3 14.9Z" />
        </symbol>
        <symbol id="tb-folder" viewBox="0 0 24 24">
          <path
            d="M3.5 7.2a2 2 0 0 1 2-2h3.6l2 2.4h7.4a2 2 0 0 1 2 2v8.2a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2z"
          />
        </symbol>
        <symbol id="tb-server" viewBox="0 0 24 24">
          <rect x="3.5" y="4.2" width="17" height="6.2" rx="2" />
          <rect x="3.5" y="13.6" width="17" height="6.2" rx="2" />
          <path d="M7 7.3h.02M7 16.7h.02" stroke-width="2.4" />
        </symbol>
        <symbol id="tb-gear" viewBox="0 0 24 24">
          <circle cx="12" cy="12" r="6.6" />
          <circle cx="12" cy="12" r="3" />
          <path d="M12 2.8v2.6M12 18.6v2.6M2.8 12h2.6M18.6 12h2.6M5.5 5.5l1.9 1.9M16.6 16.6l1.9 1.9M18.5 5.5l-1.9 1.9M7.4 16.6l-1.9 1.9" />
        </symbol>
      </svg>
      <div class="tab-indicator" ref="indicatorEl"></div>
      <a
        class="tab"
        v-for="t in tabs"
        :key="t.id"
        :class="{ on: t.id === current }"
        @click="switchTab(t.id)"
      >
        <svg class="ic"><use :href="'#tb-' + t.icon" /></svg>
        <span>{{ t.label }}</span>
      </a>
    </nav>
  </main>
</template>

<style scoped>
.app {
  min-height: calc(100vh / var(--ui-scale));
}

.sprite {
  position: absolute;
  width: 0;
  height: 0;
  overflow: hidden;
}

.tabbar {
  position: fixed;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  z-index: 10;
  display: flex;
  width: min(340px, calc(100% - 32px));
  height: 56px;
  border-radius: 28px;
  background: var(--glass-bg);   /* 玻璃底色：半透明，深浅模式通用 */
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);  /* 玻璃棱边 */
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),    /* 顶部高光边（折射光） */
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),   /* 底部微反光 */
      0 4px 16px rgba(40, 25, 30, 0.25);
}

.tab {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 3px;
  font-size: 10.5px;
  color: var(--sub);
  text-decoration: none;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
  user-select: none;
  position: relative;
  z-index: 1;
  transition: color 0.25s ease;
}

.tab-indicator {
  position: absolute;
  top: 0;
  left: 0;
  height: 42px; /* 或让 JS 一并设置 height */
  border-radius: 24px;
  background: var(--tab-bg);          /* 与玻璃胶囊同款 */
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.5);
  transition: transform 0.3s ease, width 0.3s ease;
  pointer-events: none;
  z-index: 0;
  will-change: transform, width;
}

.tab .ic {
  width: 21px;
  height: 21px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.tab.on {
  color: var(--ink);
}

.scrollbar {
  position: fixed;
  top: 16px;
  right: 6px;
  bottom: 16px;
  width: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.14);
  z-index: 30;
}
.scrollbar-thumb {
  width: 100%;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.55);
  transition: opacity 0.25s;
  will-change: transform;
}
</style>
