import assert from 'node:assert/strict'
import test from 'node:test'

import type { ManagedInstanceSpec } from './managed.ts'
import { canInstallManagedSpec, managedServerAddress } from './managed-catalog.ts'

test('server addresses retain explicit ports and allow SRV when omitted', () => {
    assert.equal(managedServerAddress({ address: 'play.example', port: null }), 'play.example')
    assert.equal(
        managedServerAddress({ address: 'play.example', port: 25565 }),
        'play.example:25565',
    )
    assert.equal(
        managedServerAddress({ address: '2001:db8::1', port: 25566 }),
        '[2001:db8::1]:25566',
    )
    assert.equal(managedServerAddress(null), null)
})

test('only supported pack and server adapters enable installation', () => {
    for (const kind of ['mrpack', 'server', 'curseforge-zip', 'multimc', 'server-zip', 'raw']) {
        assert.equal(
            canInstallManagedSpec({ pack: { kind } } as ManagedInstanceSpec),
            ['mrpack', 'server'].includes(kind),
        )
    }
})
