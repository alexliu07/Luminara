import { ref, onMounted, onUnmounted, onActivated, onDeactivated } from 'vue'

// 当前活动页面的滚动容器（自定义滚动条跟随它）
export const activeScrollEl = ref(null)

const savedY = new WeakMap()   // 各容器的滚动存档（元素为键）

// 页面根元素用法：<div class="xxx-page" ref="scroller">
export function usePageScroller() {
  const el = ref(null)
  const save = () => { if (el.value) savedY.set(el.value, el.value.scrollTop) }
  const restore = () => {
    if (!el.value) return
    activeScrollEl.value = el.value                       // 顺带纠正滚动条跟随对象
    el.value.scrollTop = savedY.get(el.value) ?? 0
  }
  onMounted(() => {
    el.value?.addEventListener('scroll', save, { passive: true })
    restore()
  })
  onUnmounted(() => el.value?.removeEventListener('scroll', save))
  onActivated(restore)      // 从缓存回来：登记 + 恢复滚动位置
  onDeactivated(save)       // 双保险再存一次
  return el
}
