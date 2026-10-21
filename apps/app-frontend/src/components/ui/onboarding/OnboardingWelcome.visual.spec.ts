import '@/test/visual-styles.scss'
import '@/assets/stylesheets/ysumc.scss'

import { mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { page } from 'vitest/browser'

import type { OnboardingStep } from './onboardingConfig'
import OnboardingWelcome from './OnboardingWelcome.vue'

vi.mock('@modrinth/ui', async () => ({
    Button: (await import('../../../../../../packages/ui/src/components/base/buttons/Button.vue'))
        .default,
    defineMessages: (messages: unknown) => messages,
    useVIntl: () => ({
        formatMessage: (message: { defaultMessage: string }) => message.defaultMessage,
    }),
    LOCALES: [{ code: 'zh-CN', name: '简体中文', translatedName: { defaultMessage: '简体中文' } }],
}))
vi.mock('@/helpers/settings.ts', () => ({
    get: async () => ({ locale: 'zh-CN' }),
    update: vi.fn(),
}))
vi.mock('@/i18n.config', () => ({
    default: { global: { locale: { value: 'zh-CN' } } },
    getSystemResolvedLocale: () => 'zh-CN',
    isFollowingSystemLocale: () => true,
    setAppLocale: vi.fn(),
    setFollowSystemLocale: vi.fn(),
}))

let wrapper: ReturnType<typeof mount> | undefined
afterEach(() => wrapper?.unmount())

it('keeps the full association name visible at desktop and narrow window sizes', async () => {
    document.documentElement.className = 'dark-mode'
    wrapper = mount(OnboardingWelcome, {
        attachTo: document.body,
        props: {
            step: {
                id: 'welcome',
                title: { defaultMessage: '一切就绪' },
                description: { defaultMessage: '从这里，进入我们的世界。' },
                action: { defaultMessage: '带我逛逛' },
            } as OnboardingStep,
        },
    })
    for (const width of [1280, 620]) {
        await page.viewport(width, 850)
        await vi.waitFor(
            () => {
                expect(
                    getComputedStyle(wrapper!.get('.onboarding-welcome-panel').element).opacity,
                ).toBe('1')
            },
            { timeout: 6000 },
        )
        await vi.waitFor(
            () => {
                const wordmark = wrapper!.get('.onboarding-welcome-wordmark').element as HTMLElement
                expect(wordmark.clientWidth).toBeGreaterThanOrEqual(wordmark.scrollWidth - 1)
            },
            { timeout: 5000 },
        )
        expect(wrapper.text()).toContain('燕山大学Minecraft学生同好者协会')
        expect(wrapper.text()).toContain('YanShan University Minecraft Club')
        expect(wrapper.text()).toContain('学生同好者协会')
        expect(wrapper.get('.onboarding-welcome-wordmark').text()).not.toContain('Axolotl')
        if (import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE) {
            await page.screenshot({
                path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-welcome-${width}.png`,
            })
        }
    }
})
