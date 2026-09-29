<script setup>
// 样例数据
const rows = [
  { label: '扫描间隔', value: '每天' },
  { label: '播放器', value: '内置' },
  { label: '字幕语言', value: '简体中文' },
  { label: '关于', value: 'v1.0.0' },
]
</script>

<template>
  <div class="page">
    <h1 class="page-title">设置</h1>
    <ul class="list">
      <li class="item" v-for="r in rows" :key="r.label">
        <span class="item-name">{{ r.label }}</span>
        <span class="item-value">{{ r.value }}</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.page {
  min-height: 100vh;
  padding: 18px 14px 110px;
  background: rgba(0,0,0,0.9);
  color: #241e20;
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}
.page-title {
  margin: 0 0 14px;
  font-size: 20px;
  font-weight: 700;
  color: #fff;
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
  background: #e9e5e5;
  box-shadow: 0 1px 3px rgba(40, 25, 30, 0.16);
}
.item-name {
  font-size: 13px;
  font-weight: 600;
}
.item-value {
  font-size: 12px;
  color: rgba(36, 30, 32, 0.62);
}
</style>
