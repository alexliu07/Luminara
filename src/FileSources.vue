<script setup>
// 样例数据
const sources = [
  { name: '本地磁盘', path: '/data/media', count: 23 },
  { name: '网络共享', path: '//nas/movies', count: 41 },
  { name: '移动硬盘', path: '/mnt/usb', count: 7 },
]
</script>

<template>
  <div class="page">
    <svg class="sprite" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <symbol id="i-more" viewBox="0 0 24 24">
        <path d="M5.2 12h.02M12 12h.02M18.8 12h.02" stroke-width="2.6" />
      </symbol>
    </svg>
    <div class="top-bar">
      <h1 class="page-title">文件源</h1>
      <button class="round-btn" type="button" aria-label="更多">
        <svg class="ic"><use href="#i-more" /></svg>
      </button>
    </div>
    <ul class="list">
      <li class="item" v-for="s in sources" :key="s.path">
        <div class="item-main">
          <p class="item-name">{{ s.name }}</p>
          <p class="item-sub">{{ s.path }}</p>
        </div>
        <span class="item-side">{{ s.count }}</span>
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
  min-height: 100vh;
  padding: 18px 14px 110px;
  background: rgba(0,0,0,0.9);
  color: #241e20;
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}
.page-title {
  margin: 0;
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
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}
.item-sub {
  margin: 3px 0 0;
  font-size: 10px;
  color: #5f585b;
}
.item-side {
  font-size: 12px;
  color: rgba(36, 30, 32, 0.62);
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
  border: 0;
  border-radius: 50%;
  background: #fff;
  color: #241e20;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12);
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
</style>
