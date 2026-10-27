import type { ResourceOperationPlan } from './resources'

/** Trace failures back to explicit selections; optional suggestions do not block their parents. */
export function blockedInstallRoots(plan: ResourceOperationPlan): string[] {
    const pending = plan.issues
        .filter((issue) => issue.blocking && issue.selectionId)
        .map((issue) => issue.selectionId!)
    const visited = new Set<string>()
    const roots = new Set<string>()
    while (pending.length) {
        const id = pending.pop()!
        if (visited.has(id)) continue
        visited.add(id)
        if (plan.activeRoots.includes(id)) roots.add(id)
        for (const root of plan.nodes.find((node) => node.id === id)?.rootSelectionIds ?? []) {
            if (plan.activeRoots.includes(root)) roots.add(root)
        }
        for (const edge of plan.edges) {
            if (edge.child === id && edge.kind === 'required') pending.push(edge.parent)
        }
    }
    return plan.activeRoots.filter((id) => roots.has(id))
}
