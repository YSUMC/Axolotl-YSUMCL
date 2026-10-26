import assert from 'node:assert/strict'
import test from 'node:test'

import { createMemoryHistory, createRouter } from 'vue-router'

import { multiplayerRoutes } from '../routes/multiplayer.ts'
import { resolveOpenRoute } from './deep-links.ts'

test('YSUMC rejects external multiplayer links and keeps game downloads available', () => {
    for (const path of ['/multiplayer', '/multiplayer/rooms', '/multiplayer/servers/example']) {
        assert.equal(resolveOpenRoute(path), null)
    }
    assert.ok(resolveOpenRoute('/browse/modpack'))
    assert.ok(resolveOpenRoute('/downloads'))
})

test('saved multiplayer routes return to Home instead of mounting network services', async () => {
    const router = createRouter({
        history: createMemoryHistory(),
        routes: [{ path: '/', component: {} }, ...multiplayerRoutes],
    })
    for (const path of [
        '/multiplayer/rooms',
        '/multiplayer/servers/example/studio',
        '/multiplayer/old',
    ]) {
        await router.push(path)
        assert.equal(router.currentRoute.value.path, '/')
    }
})
