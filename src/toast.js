import { ref } from 'vue'

// 全局 toast 状态：null = 隐藏，{ icon, text } = 显示中
export const toast = ref(null)
let timer = null

// duration = 0 表示常驻（如"扫描中"），需手动 hideToast；> 0 到点自动消失
export function showToast(text, icon, duration = 0) {
    toast.value = { icon, text }
    clearTimeout(timer)
    if (duration > 0) timer = setTimeout(hideToast, duration)
}

export function hideToast() {
    clearTimeout(timer)
    toast.value = null
}