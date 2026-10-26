import type { ManagedInstanceAction, ManagedInstanceRecord } from './managed'

/** Local install records remain authoritative when a cached manifest arrives late. */
export function reconcileManagedActions(
    actions: ManagedInstanceAction[],
    records: ManagedInstanceRecord[],
): ManagedInstanceAction[] {
    return actions.map((action) => {
        const record = records.find((entry) => entry.server_instance_id === action.spec.id)
        const instanceId = record?.instance_id || null
        return {
            ...action,
            instance_id: instanceId,
            applied_revision: instanceId ? record!.revision : null,
            action:
                action.action === 'retired'
                    ? 'retired'
                    : !instanceId
                      ? 'create'
                      : record!.revision === action.spec.revision
                        ? 'current'
                        : 'update',
        }
    })
}
