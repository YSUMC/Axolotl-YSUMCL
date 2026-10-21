import assert from 'node:assert/strict'
import test from 'node:test'

import { daysSinceOpening } from './ysumc-server.ts'

test('opening days roll over at Shanghai midnight without inventing absent dates', () => {
    assert.equal(daysSinceOpening('2026-10-20', Date.parse('2026-10-20T15:59:59Z')), 1)
    assert.equal(daysSinceOpening('2026-10-20', Date.parse('2026-10-20T16:00:00Z')), 2)
    for (const date of [null, '', '2026-02-30', 'next week', '2026-11-01']) {
        assert.equal(daysSinceOpening(date, Date.parse('2026-10-20T16:00:00Z')), null)
    }
})
