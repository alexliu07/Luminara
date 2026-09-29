<script setup>
import { ref } from 'vue'
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
</script>

<template>
  <main class="app">
    <!-- 页面容器：随底部栏切换，KeepAlive 保留各页状态 -->
    <KeepAlive>
      <component :is="pages[current]" />
    </KeepAlive>

    <!-- 底部主导航 -->
    <nav class="tabbar">
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

      <a
        class="tab"
        v-for="t in tabs"
        :key="t.id"
        :class="{ on: t.id === current }"
        @click="current = t.id"
      >
        <svg class="ic"><use :href="'#tb-' + t.icon" /></svg>
        <span>{{ t.label }}</span>
      </a>
    </nav>
  </main>
</template>

<style>
/* 整体 UI 缩放：1 = 原始大小，嫌小就往上调（如 1.25 / 1.5） */
html {
  zoom: 1.25;
}
</style>

<style scoped>
.app {
  min-height: 100vh;
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
  background: rgba(255, 255, 255, 0.16);   /* 玻璃底色：半透明，深浅模式通用 */
  backdrop-filter: blur(24px) saturate(180%);
  -webkit-backdrop-filter: blur(24px) saturate(180%);
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
</style>
