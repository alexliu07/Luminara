<script setup>
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { showToast } from './toast.js'
import { usePageScroller } from './scroll.js'

const scroller = usePageScroller()
const tmdbKey = ref('')
const saving = ref(false)

// 读取已保存的 TMDB API Key
onMounted(async () => {
  try {
    tmdbKey.value = (await invoke('get_setting', { key: 'tmdb_api_key' })) || ''
  } catch (e) {
    console.error('读取设置失败', e)
  }
})
// 保存到 settings 表
async function onSaveTmdb() {
  saving.value = true
  try {
    await invoke('set_setting', { key: 'tmdb_api_key', value: tmdbKey.value.trim() })
    showToast('已保存 TMDB API Key', 'check', 2000)
  } catch (e) {
    showToast('保存失败：' + e, 'alert', 2500)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="page" ref="scroller">
    <h1 class="page-title">设置</h1>
    <ul class="list">
      <li class="item">
        <span class="item-name">TMDB Read Access Token</span>
        <input class="form-input" type="password" v-model="tmdbKey"
               placeholder="输入 TMDB Read Access Token" @keyup.enter="onSaveTmdb" />
        <button class="save-btn" :disabled="saving" @click="onSaveTmdb">保存</button>
      </li>
      <li class="item">
        <span class="item-name">关于</span>
        <span class="item-value">v1.0.0</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.page {
  height: calc(100vh / var(--ui-scale));
  overflow-y: auto;
  scrollbar-width: none;
  padding: 18px 14px 110px;
  background: var(--bg);
  color: var(--card-ink);
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}
.page::-webkit-scrollbar { display: none; }
.page-title {
  margin: 0 0 14px;
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
  font-size: 13px;
  font-weight: 600;
}
.item-value {
  font-size: 12px;
  color: var(--sub);
}
.item .form-input {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  margin-left: 50px;
  border: 1px solid var(--input-border);
  border-radius: 8px;
  background: var(--input-bg);
  font-size: 12px;
  color: var(--ink);
  outline: none;
}
.item .form-input:focus { border-color: var(--input-border-focus); }
.save-btn {
  flex-shrink: 0;
  height: 28px;
  padding: 0 14px;
  border: none;
  border-radius: 8px;
  background: var(--ink);
  color: var(--bg);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.save-btn:disabled { opacity: 0.5; cursor: default; }
</style>
