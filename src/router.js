import { createRouter, createWebHashHistory } from 'vue-router'
import MediaLibrary from './MediaLibrary.vue'
import FileSources from './FileSources.vue'
import Settings from './Settings.vue'
import ViewAll from './ViewAll.vue'
import WorkDetail from './WorkDetail.vue'

export const router = createRouter({
    history: createWebHashHistory(),   // hash 模式，刷新/深链不 404
    routes: [
        { path: '/', component: MediaLibrary },
        { path: '/sources', component: FileSources },
        { path: '/settings', component: Settings },
        { path: '/all/:category', component: ViewAll, meta: { overlay: true } },
        { path: '/work/:kind/:tmdb', component: WorkDetail, meta: { overlay: true } },
        { path: '/:pathMatch(.*)*', redirect: '/' },
    ],
})