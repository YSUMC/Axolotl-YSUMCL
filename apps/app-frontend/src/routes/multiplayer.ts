import type { RouteRecordRaw } from 'vue-router'

export const multiplayerRoutes: RouteRecordRaw[] = [
    { path: '/multiplayer', redirect: '/' },
    { path: '/multiplayer/servers', name: 'MultiplayerServers', redirect: '/' },
    { path: '/multiplayer/servers/:id', name: 'MultiplayerServerDetail', redirect: '/' },
    { path: '/multiplayer/servers/:id/studio', name: 'MultiplayerServerFileStudio', redirect: '/' },
    { path: '/multiplayer/rooms', name: 'MultiplayerRooms', redirect: '/' },
    { path: '/multiplayer/:pathMatch(.*)*', redirect: '/' },
]
