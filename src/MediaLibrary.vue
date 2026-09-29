<script setup>
// ── 样例数据 ─────────────────────────────────────────────
const hero = {
  title: '异世界想转生成为什么',
  rating: '7.2',
  year: '2026',
  country: '日本',
  genres: 'Sci-Fi & Fantasy,动作冒险,动画,喜剧',
  cert: 'TV-14',
  colors: ['#4b3a78', '#c98f5e'],
}

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

const posterRows = [
  {
    title: '最近添加',
    items: [
      { title: 'Re: 从零开始的异世界生活', sub: '共1季', rating: '8.1', colors: ['#3c4a8c', '#8fa8e0'] },
      { title: '身为VTuber的我…', sub: '共1季', rating: '5.8', colors: ['#7fb8d8', '#d6eef8'] },
      { title: '游戏人生', sub: '共1季', rating: '8.0', colors: ['#c4447c', '#f5a3c0'] },
      { title: '游戏人生 Zero', sub: '2017', rating: '7.8', colors: ['#b84a72', '#f0879f'] },
      { title: '夏日重现', sub: '共1季', rating: '8.1', colors: ['#3f7a6c', '#9fd6c2'] },
      { title: '天气之子', sub: '2019', rating: '8.0', colors: ['#3a72c4', '#9cc6f2'] },
      { title: '你的名字。', sub: '2016', rating: '8.5', colors: ['#2c4f9e', '#e07a92'] },
      { title: '我们仍未知道那天所看见的花的名字。', sub: '共1季', rating: '8.2', colors: ['#5a8fd0', '#cfe6f8'] },
      { title: '命运石之门', sub: '共1季', rating: '8.5', colors: ['#5c3a4a', '#c8926c'] },
      { title: '女神"异世界转生"…', sub: '共1季', rating: '7.2', colors: ['#d8a24c', '#f5e0a8'] },
      { title: '不时用俄语小声说真心话的邻桌艾莉…', sub: '共1季', rating: '7.6', colors: ['#6d7fa8', '#c7d2e8'] },
    ],
  },
  {
    title: '电影',
    items: [
      { title: '游戏人生 Zero', sub: '2017', rating: '7.8', colors: ['#b84a72', '#f0879f'] },
      { title: '天气之子', sub: '2019', rating: '8.0', colors: ['#3a72c4', '#9cc6f2'] },
      { title: '你的名字。', sub: '2016', rating: '8.5', colors: ['#2c4f9e', '#e07a92'] },
      { title: '超时空辉夜姬！', sub: '2026', rating: '8.1', colors: ['#7a3ac4', '#d89cf2'] },
    ],
  },
  {
    title: '电视剧',
    items: [
      { title: 'Re: 从零开始的异世界生活', sub: '共1季', rating: '8.1', colors: ['#3c4a8c', '#8fa8e0'] },
      { title: '身为VTuber的我…', sub: '共1季', rating: '5.8', colors: ['#7fb8d8', '#d6eef8'] },
      { title: '游戏人生', sub: '共1季', rating: '8.0', colors: ['#c4447c', '#f5a3c0'] },
      { title: '夏日重现', sub: '共1季', rating: '8.1', colors: ['#3f7a6c', '#9fd6c2'] },
      { title: '我们仍未知道那天所看见的花的名字。', sub: '共1季', rating: '8.2', colors: ['#5a8fd0', '#cfe6f8'] },
      { title: '命运石之门', sub: '共1季', rating: '8.5', colors: ['#5c3a4a', '#c8926c'] },
      { title: '女神"异世界转生"…', sub: '共1季', rating: '7.2', colors: ['#d8a24c', '#f5e0a8'] },
      { title: '不时用俄语小声说真心话的邻桌艾莉…', sub: '共1季', rating: '7.6', colors: ['#6d7fa8', '#c7d2e8'] },
      { title: '想吃掉我的非人少女', sub: '共1季', rating: '8.2', colors: ['#8a6f9e', '#e2c4d8'] },
      { title: '86-不存在的战区-', sub: '共1季', rating: '8.1', colors: ['#8c3c3c', '#e08f8f'] },
      { title: '我们不可能成为恋人！', sub: '共1季', rating: '8.3', colors: ['#c4884a', '#f2c89c'] },
    ],
  },
  {
    title: '动漫',
    items: [
      { title: '身为VTuber的我…', sub: '共1季', rating: '5.8', colors: ['#7fb8d8', '#d6eef8'] },
      { title: '游戏人生', sub: '共1季', rating: '8.0', colors: ['#c4447c', '#f5a3c0'] },
      { title: '游戏人生 Zero', sub: '2017', rating: '7.8', colors: ['#b84a72', '#f0879f'] },
      { title: '夏日重现', sub: '共1季', rating: '8.1', colors: ['#3f7a6c', '#9fd6c2'] },
      { title: '天气之子', sub: '2019', rating: '8.0', colors: ['#3a72c4', '#9cc6f2'] },
      { title: '你的名字。', sub: '2016', rating: '8.5', colors: ['#2c4f9e', '#e07a92'] },
      { title: '我们仍未知道那天所看见的花的名字。', sub: '共1季', rating: '8.2', colors: ['#5a8fd0', '#cfe6f8'] },
      { title: '命运石之门', sub: '共1季', rating: '8.5', colors: ['#5c3a4a', '#c8926c'] },
      { title: '女神"异世界转生"…', sub: '共1季', rating: '7.2', colors: ['#d8a24c', '#f5e0a8'] },
      { title: '不时用俄语小声说真心话的邻桌艾莉…', sub: '共1季', rating: '7.6', colors: ['#6d7fa8', '#c7d2e8'] },
      { title: '想吃掉我的非人少女', sub: '共1季', rating: '8.2', colors: ['#8a6f9e', '#e2c4d8'] },
    ],
  },
]

const grad = (c) => `linear-gradient(155deg, ${c[0]} 0%, ${c[1]} 100%)`
</script>

<template>
  <div class="media-page">
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
    </svg>
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
    <!-- 顶部 hero -->
    <header class="hero" :style="{ background: grad(hero.colors) }">
      <div class="hero-info">
        <h1 class="hero-title">{{ hero.title }}</h1>
        <div class="hero-meta">
          <svg class="ic ic-star"><use href="#i-star" /></svg>
          <span>{{ hero.rating }}</span>
          <i class="sep"></i>
          <span>{{ hero.year }}</span>
          <i class="sep"></i>
          <span>{{ hero.country }}</span>
          <i class="sep"></i>
          <span>{{ hero.genres }}</span>
          <i class="sep"></i>
          <span class="cert">{{ hero.cert }}</span>
        </div>
        <div class="dots">
          <i class="dot on"></i>
          <i class="dot"></i>
        </div>
      </div>
    </header>

    <!-- 继续观看 -->
    <section class="sec">
      <div class="sec-head">
        <h2 class="sec-title">{{ continueRow.title }}</h2>
        <a class="sec-more">查看全部 <svg class="ic ic-sm"><use href="#i-chevron" /></svg></a>
      </div>
      <div class="row">
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
        <a class="sec-more">查看全部 <svg class="ic ic-sm"><use href="#i-chevron" /></svg></a>
      </div>
      <div class="row">
        <article class="p-card" v-for="item in row.items" :key="item.title + item.sub">
          <div class="p-art" :style="{ background: grad(item.colors) }">
            <span class="p-rate">{{ item.rating }}</span>
          </div>
          <h3 class="cap-title">{{ item.title }}</h3>
          <p class="cap-sub">{{ item.sub }}</p>
        </article>
      </div>
    </section>
  </div>
</template>

<style scoped>
/* ── 基础 ───────────────────────────────────────────── */
.media-page {
  min-height: 100vh;
  padding: 0;
  background: var(--bg);
  color: var(--ink);
  font-family: 'PingFang SC', 'HarmonyOS Sans SC', 'MiSans', 'Microsoft YaHei',
    -apple-system, 'Segoe UI', sans-serif;
  -webkit-font-smoothing: antialiased;
}

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
  height: 34px;
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
  background: var(--tabbar-bg);
  color: var(--ink);
  background: rgba(255, 255, 255, 0.16);   /* 玻璃底色：半透明，深浅模式通用 */
  backdrop-filter: blur(24px) saturate(180%);
  -webkit-backdrop-filter: blur(24px) saturate(180%);
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
  color: rgba(255, 255, 255, 0.8);
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
  color: rgba(255, 255, 255, 0.92);
}
.hero-meta .ic-star {
  width: 15px;
  height: 15px;
  color: #f2c14e;
  fill: #f2c14e;
  stroke: none;
}
.sep {
  width: 1px;
  height: 11px;
  background: rgba(255, 255, 255, 0.5);
}
.cert {
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.22);
  font-size: 11px;
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
  background: var(--white);
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
}

.row {
  display: flex;
  gap: 10px;
  padding: 0 14px;
  overflow-x: auto;
  scrollbar-width: none;
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
}
.p-art {
  position: relative;
  width: 100%;
  aspect-ratio: 2 / 3;
  border-radius: 10px;
  box-shadow: 0 1px 4px rgba(40, 25, 30, 0.22);
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
  color: var(--white);
  letter-spacing: 0.2px;
}

/* ── 继续观看卡 ─────────────────────────────────────── */
.w-card {
  flex-shrink: 0;
  width: clamp(180px, 19.5vw, 212px);
}
.w-art {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  border-radius: 10px;
  box-shadow: 0 1px 4px rgba(40, 25, 30, 0.22);
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
  background: var(--white);
}
</style>
