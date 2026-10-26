import assert from 'node:assert/strict'
import test from 'node:test'

import { parseLauncherReleases, shouldWaitForRelease } from './launcher-releases.ts'

const release = {
    version: '1.0.1',
    notes: '修复下载\n- 支持自动切源',
    published_at: '2026-10-21T10:48:00Z',
}
test('release notes accept metadata without a same-site installer and preserve Markdown', () => {
    assert.deepEqual(parseLauncherReleases({ releases: [release] }), [release])
    assert.deepEqual(parseLauncherReleases({ releases: [] }), [])
    assert.deepEqual(parseLauncherReleases({ releases: [{ ...release, notes: null }] }), [
        { ...release, notes: null },
    ])
})
test('invalid or ambiguous responses cannot replace a valid release cache', () => {
    for (const response of [
        null,
        {},
        { versions: [] },
        { releases: [release, release] },
        { releases: [{ ...release, version: '<script>' }] },
        { releases: [{ ...release, published_at: 'yesterday' }] },
        { releases: [{ ...release, notes: 'x'.repeat(20001) }] },
        { releases: Array(101).fill(release) },
    ]) {
        assert.equal(parseLauncherReleases(response), null)
    }
})
test('manual checks and immediate updates see a just-published release', () => {
    const now = Date.parse(release.published_at) + 1000
    assert.equal(shouldWaitForRelease(release.published_at, true, false, false, now), false)
    assert.equal(shouldWaitForRelease(release.published_at, false, true, false, now), false)
    assert.equal(shouldWaitForRelease(release.published_at, false, false, true, now), false)
    assert.equal(shouldWaitForRelease(undefined, false, true, false, now), false)
})
test('an explicitly selected wait applies only to automatic checks for the first day', () => {
    const published = Date.parse(release.published_at)
    assert.equal(
        shouldWaitForRelease(release.published_at, false, false, false, published + 86399999),
        true,
    )
    assert.equal(
        shouldWaitForRelease(release.published_at, false, false, false, published + 86400000),
        false,
    )
})
