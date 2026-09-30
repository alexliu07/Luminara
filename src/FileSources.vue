<script setup>
import { ref, onMounted } from 'vue'
import {invoke} from "@tauri-apps/api/core";
// 样例数据
const sources = ref([])
const sourcemap = {0:'本地',1:'WebDAV'}
const menuOpen = ref(false)
const ctxOpen = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxItem = ref(null)

const menuItems = [
  { id: 'local', label: '添加本地文件夹' },
  { id: 'network', label: '添加 WebDAV 共享' }
]

async function loadSources() {
  try {
    sources.value = await invoke('list_sources')
  } catch (e) {
    console.error('加载文件源失败', e)
  }
}
onMounted(loadSources)

async function onMenuItem(item) {
  menuOpen.value = false
  // TODO: 各项的实际动作
  if(item.id === 'local'){
    if(await invoke('add_local_source')){
      let data = await invoke('list_sources')
      console.log(data);
      sources.value = data;
    }
  }
}

function uiScale() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-scale')) || 1
}

function openCtx(e, s) {
  ctxItem.value = s
  ctxX.value = e.clientX / uiScale()
  ctxY.value = e.clientY / uiScale()
  ctxOpen.value = true
  menuOpen.value = false // 顺手关掉加号菜单
}
function onDeleteSource() {
  ctxOpen.value = false
  // TODO: 后面接删除逻辑（ctxItem.value 里有被点的文件源）
}
</script>

<template>
  <div class="page">
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-plus" viewBox="0 0 24 24">
        <path d="M12 5.5v13M5.5 12h13" />
      </symbol>
    </svg>
    <div class="top-bar">
      <h1 class="page-title">文件源</h1>
      <button class="round-btn" type="button" aria-label="添加" @click="menuOpen = !menuOpen">
        <svg class="ic"><use href="#i-plus" /></svg>
      </button>
      <div class="menu-backdrop" v-if="menuOpen || ctxOpen" @click="menuOpen = false; ctxOpen = false"></div>
      <Transition name="menu">
        <div class="src-menu" v-if="menuOpen">
          <button
              class="src-menu-item"
              v-for="m in menuItems"
              :key="m.id"
              type="button"
              @click="onMenuItem(m)"
          >
            {{ m.label }}
          </button>
        </div>
      </Transition>
    </div>
    <Transition name="menu">
      <div class="src-menu ctx-menu" v-if="ctxOpen" :style="{ left: ctxX + 'px', top: ctxY + 'px' }">
        <button class="ctx-menu-item src-menu-item danger" type="button" @click="onDeleteSource">删除文件源</button>
      </div>
    </Transition>
    <ul class="list">
      <li class="item" v-for="s in sources" :key="s.id" @contextmenu.prevent="openCtx($event,s)">
        <div class="item-main">
          <p class="item-name">{{ s.name }}</p>
          <p class="item-sub">{{sourcemap[s.source]}} | {{ s.data_a }}</p>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.sprite {
  position: absolute;
  width: 0;
  height: 0;
  overflow: hidden;
}
.page {
  min-height: calc(100vh / var(--ui-scale));
  padding: 18px 14px 110px;
  background: var(--bg);
  color: var(--card-ink);
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}
.page-title {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  color: var(--ink);
}
.list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
  padding: 13px 15px;
  border-radius: 12px;
  background: var(--card);
  box-shadow: 0 1px 3px rgba(40, 25, 30, 0.16);
}
.item-name {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}
.item-sub {
  margin: 3px 0 0;
  font-size: 10px;
  color: var(--sub);
}
.top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.round-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 50%;
  color: var(--ink);
  position: fixed;
  top: 14px;
  right: 14px;
  z-index: 20;
  background: var(--glass-bg);   /* 玻璃底色：半透明，深浅模式通用 */
  backdrop-filter: blur(5px) saturate(180%);
  -webkit-backdrop-filter: blur(5px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.28);  /* 玻璃棱边 */
  box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.45),    /* 顶部高光边（折射光） */
      inset 0 -1px 0 rgba(255, 255, 255, 0.12),   /* 底部微反光 */
      0 4px 16px rgba(40, 25, 30, 0.25);
}

.ic {
  width: 20px;
  height: 20px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 18;              /* 比按钮(20)低、比内容高 */
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
  background: rgba(255, 255, 255, 0.18);
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

.ctx-menu {
  top: auto;   /* 覆盖 .src-menu 的固定位置 */
  right: auto;
}

.ctx-menu-item.danger {
  color: #e5484d;
}
.ctx-menu-item.danger:hover {
  background: rgba(229, 72, 77, 0.16);
}

</style>
