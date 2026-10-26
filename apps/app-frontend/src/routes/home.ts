import './meta'

import type { RouteRecordRaw } from 'vue-router'

import { launcherBrand } from '@/branding'

export const homeRoutes: RouteRecordRaw[] = [
    ...(launcherBrand.partners
        ? [
              {
                  path: '/partners',
                  name: 'Partners',
                  component: () => import('@/pages/Community.vue'),
                  props: { partners: true },
                  meta: { breadcrumb: [{ name: 'Partners' }] },
              },
          ]
        : []),
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
