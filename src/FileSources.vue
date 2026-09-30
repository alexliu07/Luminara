<script setup>
import { ref, onMounted, nextTick } from 'vue'
import {invoke} from "@tauri-apps/api/core";
import { showToast, hideToast } from './toast.js'
// 数据
const sources = ref([])
const sourcemap = {0:'本地',1:'WebDAV'}
const files = ref([])
const filesSource = ref(null)   // null=显示源列表，否则显示该源的文件
const typemap = { video: '视频', subtitle: '字幕' }
const menuOpen = ref(false)
const ctxOpen = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxItem = ref(null)
const ctxMenuEl = ref(null)
const confirmOpen = ref(false)
const webdavOpen = ref(false)
const webdav = ref({ name: '', host: '', port: '', https: true, username: '', password: '', path: '/' })
const testing = ref(false)
const menuItems = [
  { id: 'local', label: '添加本地文件夹' },
  { id: 'webdav', label: '添加 WebDAV 共享' }
]

// 加载文件源列表
async function loadSources() {
  try {
    sources.value = await invoke('list_sources')
    refreshScrollbar()
  } catch (e) {
    console.error('加载文件源失败', e)
  }
}
onMounted(loadSources)
// 刷新滚动条
function refreshScrollbar() {
  // 等列表渲染出新高度，再让 App.vue 的 updateScrollbar 重算
  nextTick(() => window.dispatchEvent(new Event('resize')))
}
// 添加文件源菜单点击事件
async function onMenuItem(item) {
  menuOpen.value = false
  // TODO: 各项的实际动作
  if(item.id === 'local'){
    const id = await invoke('add_local_source');
    if(id){
      sources.value = await invoke('list_sources');
      refreshScrollbar();
      showToast('正在扫描文件源','spin')
      let n;
      try{
        n = await invoke('index_local_source',{id})
      } finally {
        hideToast()
        showToast('扫描完毕，添加了 '+String(n.added)+' 个文件，删除了 '+String(n.removed)+' 个文件，更新了 '+String(n.updated)+' 个文件','check',2000);
      }
    }
  }
  if(item.id === 'webdav'){
    webdavOpen.value = true
  }
}

// 获取界面缩放比例
function uiScale() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-scale')) || 1
}

// 打开右键菜单
function openCtx(e, s) {
  ctxItem.value = s
  const scale = uiScale()
  const x = e.clientX / scale
  const y = e.clientY / scale
  ctxX.value = x
  ctxY.value = y
  ctxOpen.value = true
  menuOpen.value = false // 顺手关掉加号菜单

  // 菜单进 DOM 后量出尺寸，把位置夹在视口内
  nextTick(() => {
    const el = ctxMenuEl.value
    if (!el) return
    const m = 8 // 离边最小留白
    const vw = window.innerWidth / scale
    const vh = window.innerHeight / scale
    ctxX.value = Math.max(m, Math.min(x, vw - el.offsetWidth - m))
    ctxY.value = Math.max(m, Math.min(y, vh - el.offsetHeight - m))
  })
}

// 删除文件源相关
function onDeleteSource() {
  ctxOpen.value = false
  confirmOpen.value = true
}
function onCancelDelete(){
  confirmOpen.value = false
}
async function onConfirmDelete(){
  confirmOpen.value = false
  const s = ctxItem.value
  if (!s) return
  await invoke('delete_source', { id: s.id })
  sources.value = sources.value.filter(x => x.id !== s.id)  // 先本地移除
  refreshScrollbar()
}

// 手动扫描文件源
async function onScan(){
  ctxOpen.value = false;
  let source = ctxItem.value.source;
  let id = ctxItem.value.id;
  showToast('正在扫描文件源','spin')
  let n;
  try{
    if (source === 0){
      n = await invoke('index_local_source',{id})
    }
    if(source === 1){
      n = await invoke('index_webdav_source',{id});
    }
  } finally {
    hideToast()
    showToast('扫描完毕，添加了 '+String(n.added)+' 个文件，删除了 '+String(n.removed)+' 个文件，更新了 '+String(n.updated)+' 个文件','check',2000);
  }
}

// 浏览文件源相关
async function onOpenSource(s) {
  filesSource.value = s
  try {
    files.value = await invoke('list_files', { id: s.id })
  } catch (e) {
    showToast('加载文件失败：' + e, 'alert', 2500)
    filesSource.value = null
    return
  }
  refreshScrollbar()
}
function onBack() {
  filesSource.value = null
  files.value = []
  refreshScrollbar()
}
function fmtSize(n) {
  if (n >= 1073741824) return (n / 1073741824).toFixed(1) + ' GB'
  if (n >= 1048576) return (n / 1048576).toFixed(1) + ' MB'
  if (n >= 1024) return (n / 1024).toFixed(0) + ' KB'
  return n + ' B'
}
function fmtTime(t) {
  return new Date(t * 1000).toLocaleString()
}

// WebDAV添加相关
function onHttpsToggle() {
  // 端口没手填过就跟着协议走
  if (!webdav.value.port || webdav.value.port === '80' || webdav.value.port === '443') {
    webdav.value.port = webdav.value.https ? '443' : '80'
  }
}
async function onAddWebdav() {
  const args = collectWebdav()
  if(!args) return
  try {
    await invoke('add_webdav_source', args)
    webdavOpen.value = false
    sources.value = await invoke('list_sources')
    refreshScrollbar()
    showToast('正在扫描文件源','spin')
    let n;
    try{
      n = await invoke('index_webdav_source',{id})
    } finally {
      hideToast()
      showToast('扫描完毕，添加了 '+String(n.added)+' 个文件，删除了 '+String(n.removed)+' 个文件，更新了 '+String(n.updated)+' 个文件','check',2000);
    }
  } catch (e) {
    showToast('添加失败：' + e, 'alert', 2500)
  }
}
function collectWebdav() {
  const w = webdav.value
  w.name = w.name.trim(); w.host = w.host.trim(); w.path = w.path.trim()
  if (!w.name || !w.host) { showToast('请填写名称和服务器地址', 'alert', 2000); return null }
  const port = parseInt(w.port || (w.https ? '443' : '80'), 10)
  if (!(port >= 1 && port <= 65535)) { showToast('端口号不正确', 'alert', 2000); return null }
  return {
    name: w.name, host: w.host, port,
    https: w.https, username: w.username, password: w.password, path: w.path || '/'
  }
}
async function onTestWebdav() {
  const args = collectWebdav()
  if (!args) return
  testing.value = true
  try {
    await invoke('test_webdav_source', args)
    showToast('连接成功', 'check', 2000)
  } catch (e) {
    showToast('连接失败：' + e, 'alert', 2500)
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <div class="page">
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-plus" viewBox="0 0 24 24">
        <path d="M12 5.5v13M5.5 12h13" />
      </symbol>
      <symbol id="i-back" viewBox="0 0 24 24">
        <path d="M15 5.5 8.5 12l6.5 6.5" />
      </symbol>
    </svg>
    <div class="top-bar">
      <button v-if="filesSource" class="round-btn" type="button" aria-label="返回" @click="onBack">
        <svg class="ic"><use href="#i-back" /></svg>
      </button>
      <h1 class="page-title">{{ filesSource ? filesSource.name : '文件源' }}</h1>
      <button v-if="!filesSource" class="round-btn" type="button" aria-label="添加" @click="menuOpen = !menuOpen">
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
      <div class="src-menu ctx-menu" ref="ctxMenuEl" v-if="ctxOpen" :style="{ left: ctxX + 'px', top: ctxY + 'px' }">
        <button class="ctx-menu-item src-menu-item" type="button" @click="onScan">扫描文件源</button>
        <button class="ctx-menu-item src-menu-item danger" type="button" @click="onDeleteSource">删除文件源</button>
      </div>
    </Transition>
    <Transition name="confirm">
      <div class="confirm-backdrop" v-if="confirmOpen" @click="onCancelDelete">
        <div class="confirm-box" @click.stop>
          <h2 class="confirm-title">删除文件源</h2>
          <p class="confirm-text">确定要删除「{{ ctxItem?.name }}」吗？只会移出列表，不会删除磁盘上的文件。</p>
          <div class="confirm-actions">
            <button class="confirm-btn" type="button" @click="onCancelDelete">取消</button>
            <button class="confirm-btn danger" type="button" @click="onConfirmDelete">删除</button>
          </div>
        </div>
      </div>
    </Transition>
    <Transition name="confirm">
      <div class="confirm-backdrop" v-if="webdavOpen" @click="webdavOpen = false">
        <div class="confirm-box webdav-box" @click.stop>
          <h2 class="confirm-title">添加 WebDAV 共享</h2>
          <div class="form-row">
            <span class="form-label">文件源名称</span>
            <input class="form-input" v-model="webdav.name" placeholder="我的网盘" />
          </div>
          <div class="form-row">
            <span class="form-label">服务器地址</span>
            <input class="form-input" v-model="webdav.host" placeholder="dav.example.com" />
          </div>
          <div class="form-row">
            <span class="form-label">端口号</span>
            <input class="form-input" v-model="webdav.port" inputmode="numeric" placeholder="443" />
          </div>
          <div class="form-row">
            <span class="form-label">启用 HTTPS</span>
            <input class="form-check" type="checkbox" v-model="webdav.https" @change="onHttpsToggle" />
          </div>
          <div class="form-row">
            <span class="form-label">用户名</span>
            <input class="form-input" v-model="webdav.username" />
          </div>
          <div class="form-row">
            <span class="form-label">密码</span>
            <input class="form-input" type="password" v-model="webdav.password" />
          </div>
          <div class="form-row">
            <span class="form-label">路径</span>
            <input class="form-input" v-model="webdav.path" placeholder="/dav" />
          </div>
          <div class="confirm-actions">
            <button class="confirm-btn test-btn" type="button" :disabled="testing" @click="onTestWebdav">{{ testing ? '测试中…' : '测试连接' }}</button>
            <button class="confirm-btn" type="button" @click="webdavOpen = false">取消</button>
            <button class="confirm-btn primary" type="button" @click="onAddWebdav">添加</button>
          </div>
        </div>
      </div>
    </Transition>
    <ul class="list" v-if="!filesSource">
      <li class="item" v-for="s in sources" :key="s.id" @click="onOpenSource(s)" @contextmenu.prevent="openCtx($event,s)">
        <div class="item-main">
          <p class="item-name">{{ s.name }}</p>
          <p class="item-sub">{{sourcemap[s.source]}} | {{ s.data_a }}</p>
        </div>
      </li>
    </ul>
    <ul class="list" v-else>
      <li class="item" v-for="f in files" :key="f.rel_path">
        <div class="item-main">
          <p class="item-name">{{ f.rel_path }}</p>
          <p class="item-sub">{{ typemap[f.kind] }} | {{ fmtSize(f.size) }} | {{ fmtTime(f.mtime) }}</p>
        </div>
      </li>
      <li class="item" v-if="!files.length">
        <div class="item-main">
          <p class="item-sub">暂无索引文件，右键源可扫描</p>
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
  cursor: pointer;
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
/* 确认弹窗 */
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

/* WebDAV 表单弹窗 */
.webdav-box {
  width: min(340px, calc(100% - 48px));
}
.form-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 6px 0;
}
.form-label {
  font-size: 12px;
  color: var(--sub);
  flex-shrink: 0;
}
.form-input {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  border: 1px solid rgba(255, 255, 255, 0.28);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.18);
  font-size: 12px;
  color: var(--ink);
  outline: none;
}
.form-input:focus {
  border-color: rgba(255, 255, 255, 0.55);
}
.form-check {
  width: 16px;
  height: 16px;
  accent-color: var(--ink);
}
.confirm-btn.primary {
  background: var(--ink);
  color: var(--bg);
}
.test-btn { margin-right: auto; }        /* 顶到左侧，取消/添加留在右侧 */
.confirm-btn:disabled { opacity: 0.5; cursor: default; }
</style>