import '@/test/visual-styles.scss'
import '@/assets/stylesheets/ysumc.scss'

import { mount } from '@vue/test-utils'
import { afterEach, expect, it } from 'vitest'
import { page } from 'vitest/browser'
import { h } from 'vue'

import Button from '../../../../../../packages/ui/src/components/base/buttons/Button.vue'
import Chips from '../../../../../../packages/ui/src/components/base/Chips.vue'
import Combobox from '../../../../../../packages/ui/src/components/base/Combobox.vue'
import FilterPills from '../../../../../../packages/ui/src/components/base/FilterPills.vue'
import Tabs from '../../../../../../packages/ui/src/components/base/Tabs.vue'

let wrapper: ReturnType<typeof mount> | undefined
afterEach(() => {
    wrapper?.unmount()
    document.querySelector('#teleports')?.remove()
})

function luminance(color: string) {
    const [r, g, b] = color
        .match(/[\d.]+/g)!
        .slice(0, 3)
        .map((value) => {
            const channel = Number(value) / 255
            return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
        })
    return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

for (const theme of ['light', 'dark']) {
    it(`keeps channel chips, filters, tabs and highlighted buttons readable in ${theme}`, () => {
        document.documentElement.className = `${theme}-mode`
        wrapper = mount(
            {
                render: () =>
                    h('div', [
                        h(Chips, {
                            modelValue: 'Beta 测试版',
                            items: ['正式版', 'Beta 测试版', 'Alpha 测试版'],
                        }),
                        h(FilterPills, {
                            modelValue: ['fabric'],
                            options: [{ id: 'fabric', label: 'Fabric' }],
                        }),
                        h(Tabs, { value: 'all', tabs: [{ value: 'all', label: '全部' }] }),
                        ...(['chip-text', 'highlight'] as const).map((type) =>
                            h(Button, { type, color: 'brand' }, () =>
                                h('span', { class: 'text-brand' }, type),
                            ),
                        ),
                    ]),
            },
            { attachTo: document.body, global: { directives: { tooltip: {} } } },
        )
        const selected = wrapper.element.querySelectorAll(
            '[aria-checked="true"], [aria-pressed="true"], [aria-selected="true"], .button-frame--chip-text, .button-frame--highlight',
        )
        expect(selected).toHaveLength(5)
        for (const element of selected) {
            const background = luminance(getComputedStyle(element).backgroundColor)
            for (const label of [element, ...element.querySelectorAll('span, svg')]) {
                const foreground = luminance(getComputedStyle(label).color)
                expect(
                    (Math.max(foreground, background) + 0.05) /
                        (Math.min(foreground, background) + 0.05),
                ).toBeGreaterThanOrEqual(4.5)
            }
        }
    })
    it(`keeps selected language text readable in the ${theme} theme`, async () => {
        document.documentElement.className = `${theme}-mode`
        const teleports = document.createElement('div')
        teleports.id = 'teleports'
        document.body.append(teleports)
        wrapper = mount(Combobox, {
            attachTo: document.body,
            props: {
                modelValue: 'zh-CN',
                width: 'full',
                options: [
                    { value: 'en-US', label: 'English', subLabel: '英语' },
                    { value: 'zh-CN', label: '简体中文', subLabel: '中文（简体）' },
                ],
            },
        })
        await page.getByRole('button', { name: '简体中文', exact: true }).click()
        await expect
            .element(page.getByRole('option', { name: '简体中文 中文（简体）' }))
            .toBeVisible()
        const selected = document.querySelector('[role="option"][aria-selected="true"]')!
        const background = luminance(getComputedStyle(selected).backgroundColor)
        for (const label of selected.querySelectorAll('.text-brand')) {
            const foreground = luminance(getComputedStyle(label).color)
            expect(
                (Math.max(foreground, background) + 0.05) /
                    (Math.min(foreground, background) + 0.05),
            ).toBeGreaterThanOrEqual(4.5)
        }
        if (import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE) {
            await page.screenshot({
                path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-selection-${theme}.png`,
            })
        }
    })
}
