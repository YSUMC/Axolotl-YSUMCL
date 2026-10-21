import './meta'

import type { RouteRecordRaw } from 'vue-router'

export const homeRoutes: RouteRecordRaw[] = [
    { path: '/dashboard', name: 'Dashboard', component: () => import('@/pages/Index.vue') },
    {
        path: '/',
        name: 'Home',
        component: () => import('@/pages/Community.vue'),
        meta: {
            breadcrumb: [{ name: 'Home' }],
            discordActivity: 'Idling...',
        },
    },
]
