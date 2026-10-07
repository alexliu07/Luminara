// 页面数据缓存：Promise 级去重 + 预热，二级页挂载时直接取
import { invoke } from '@tauri-apps/api/core'

const worksP = ref0()   // 简化写法见下
function ref0() { return { p: null } }

// 作品列表（媒体库/查看全部共用）
export function getWorks() {
    if (!worksP.p) worksP.p = invoke('list_works').catch(e => { worksP.p = null; throw e })
    return worksP.p
}
const unscrapedP = { p: null }
export function getUnscraped() {
    if (!unscrapedP.p) unscrapedP.p = invoke('list_unscraped').catch(e => { unscrapedP.p = null; throw e })
    return unscrapedP.p
}

// 单作品详情（按 tmdb+kind 缓存）
const workDetail = new Map()
export function getWork(tmdb, kind) {
    const key = kind + ':' + tmdb
    if (!workDetail.has(key)) {
        workDetail.set(key, invoke('get_work', { tmdb: Number(tmdb), kind })
            .catch(e => { workDetail.delete(key); throw e }))
    }
    return workDetail.get(key)
}

// 扫描/刮削后清全缓存（挂到 library-refresh 事件上）
window.addEventListener('library-refresh', () => {
    worksP.p = null
    unscrapedP.p = null
    workDetail.clear()
})