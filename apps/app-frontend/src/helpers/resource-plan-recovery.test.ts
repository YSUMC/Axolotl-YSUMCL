import assert from 'node:assert/strict'
import test from 'node:test'

import { blockedInstallRoots } from './resource-plan-recovery.ts'
import type { ResourceOperationPlan } from './resources'

function plan(overrides: Partial<ResourceOperationPlan> = {}): ResourceOperationPlan {
    return {
        activeRoots: ['good', 'broken', 'also-broken'],
        nodes: [],
        issues: [{ blocking: true, selectionId: 'dependency' }],
        edges: [
            { parent: 'broken', child: 'middle', kind: 'required' },
            { parent: 'middle', child: 'dependency', kind: 'required' },
            { parent: 'also-broken', child: 'dependency', kind: 'required' },
            { parent: 'good', child: 'dependency', kind: 'optional' },
        ],
        ...overrides,
    } as ResourceOperationPlan
}

test('traces an unresolved dependency through all required parents, even if its node is absent', () => {
    assert.deepEqual(blockedInstallRoots(plan()), ['broken', 'also-broken'])
})

test('handles cycles and preserves explicit aliases of a merged resource', () => {
    const value = plan()
    value.edges.push({ parent: 'dependency', child: 'middle', kind: 'required' } as never)
    value.nodes.push({ id: 'broken', rootSelectionIds: ['broken', 'good'] } as never)
    assert.deepEqual(blockedInstallRoots(value), ['good', 'broken', 'also-broken'])
})

test('never treats optional warnings or unbound global failures as removable roots', () => {
    assert.deepEqual(
        blockedInstallRoots(
            plan({
                issues: [
                    { blocking: false, selectionId: 'broken' },
                    { blocking: true, selectionId: null },
                ] as never,
            }),
        ),
        [],
    )
})

test('keeps a user-selected optional dependency removable without removing its optional parent', () => {
    const value = plan({ activeRoots: ['good', 'dependency'] })
    assert.deepEqual(blockedInstallRoots(value), ['dependency'])
})
