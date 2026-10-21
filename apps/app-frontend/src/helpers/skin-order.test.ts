import assert from 'node:assert/strict'
import test from 'node:test'

import { newestSiteSkinsFirst } from './skin-order.ts'
import type { Skin } from './skins.ts'

test('puts newly uploaded site skins first without changing built-in order or input', () => {
    const skins = [
        { texture_key: '9', source: 'custom' },
        { texture_key: 'steve', source: 'default' },
        { texture_key: '100', source: 'custom' },
        { texture_key: 'alex', source: 'default' },
        { texture_key: '20', source: 'custom' },
    ] as Skin[]
    assert.deepEqual(
        newestSiteSkinsFirst(skins).map((skin) => skin.texture_key),
        ['100', '20', '9', 'steve', 'alex'],
    )
    assert.equal(skins[0].texture_key, '9')
})
