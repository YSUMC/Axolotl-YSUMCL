import assert from 'node:assert/strict'
import test from 'node:test'

import type { ManagedInstanceAction, ManagedInstanceRecord } from './managed'
import { reconcileManagedActions } from './managed-state.ts'

test('uninstall immediately clears stale manifest bindings and permits reinstall', () => {
    const manifest = [
        {
            spec: { id: 'server', revision: 2 },
            action: 'current',
            instance_id: 'deleted',
            applied_revision: 2,
        },
    ] as ManagedInstanceAction[]
    const records = [
        { server_instance_id: 'server', instance_id: '', revision: 0 },
    ] as ManagedInstanceRecord[]
    assert.deepEqual(
        reconcileManagedActions(manifest, records).map(
            ({ action, instance_id, applied_revision }) => ({
                action,
                instance_id,
                applied_revision,
            }),
        ),
        [{ action: 'create', instance_id: null, applied_revision: null }],
    )
    records[0].instance_id = 'reinstalled'
    records[0].revision = 2
    assert.equal(reconcileManagedActions(manifest, records)[0].instance_id, 'reinstalled')
    assert.equal(reconcileManagedActions(manifest, records)[0].action, 'current')
    records[0].revision = 1
    assert.equal(reconcileManagedActions(manifest, records)[0].action, 'update')
    manifest[0].action = 'retired'
    assert.equal(reconcileManagedActions(manifest, records)[0].action, 'retired')
})
