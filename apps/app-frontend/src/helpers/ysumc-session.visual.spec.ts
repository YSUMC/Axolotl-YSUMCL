import { afterEach, expect, it, vi } from 'vitest'

import { YsumcConfig } from '@/config'
import type { MinecraftAccount } from '@/helpers/minecraft-account'

import {
    acceptYsumcSession,
    refreshYsumcSession,
    ysumcAccount,
    ysumcSignedIn,
} from './ysumc-session'

const calls = vi.hoisted(() => ({ invoke: vi.fn(), users: vi.fn(), select: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: calls.invoke }))
vi.mock('./auth', () => ({ users: calls.users, set_default_user: calls.select }))

const account = {
    account_type: 'yggdrasil',
    account_id: 'test-ysumc',
    yggdrasil: { api_root: YsumcConfig.yggdrasilApiRoot },
} as MinecraftAccount

afterEach(() => {
    ysumcAccount.value = null
    vi.resetAllMocks()
})

it('requires a validated server session even when a cached account exists', async () => {
    ysumcAccount.value = account
    calls.invoke.mockRejectedValue(new Error('revoked'))
    calls.users.mockResolvedValue([account])
    await refreshYsumcSession()
    expect(ysumcSignedIn.value).toBe(false)
    expect(calls.users).not.toHaveBeenCalled()
})

it('only admits the validated YSUMC account', async () => {
    calls.invoke.mockResolvedValue(account.account_id)
    calls.users.mockResolvedValue([account])
    await refreshYsumcSession()
    expect(ysumcSignedIn.value).toBe(true)
    expect(calls.invoke).toHaveBeenCalledWith('plugin:auth|validate_ysumc_session')
    calls.users.mockResolvedValue([
        { ...account, yggdrasil: { api_root: 'https://other.example' } },
    ])
    await refreshYsumcSession()
    expect(ysumcSignedIn.value).toBe(false)
})

it('a completed login supersedes an older in-flight session check', async () => {
    let finish!: (value: null) => void
    calls.invoke.mockReturnValue(
        new Promise((resolve) => {
            finish = resolve
        }),
    )
    const pending = refreshYsumcSession()
    await acceptYsumcSession(account)
    finish(null)
    await pending
    expect(ysumcAccount.value?.account_id).toBe(account.account_id)
    expect(calls.select).toHaveBeenCalledWith(account.account_id)
})
