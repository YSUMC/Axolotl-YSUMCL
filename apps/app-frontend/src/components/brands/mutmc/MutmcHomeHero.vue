<script setup lang="ts">
import { ChangeSkinIcon, ExternalIcon, RightArrowIcon } from '@modrinth/assets'
import { Button, SkinPreviewRenderer, useVIntl } from '@modrinth/ui'
import { RouterLink } from 'vue-router'

import { launcherBrand } from '@/branding'
import type { SkinModel } from '@/helpers/skins'

import { mutmcMessages as messages } from './messages'

withDefaults(
    defineProps<{ name?: string; texture?: string; variant?: SkinModel; cape?: string }>(),
    {
        name: '',
        texture: '',
        variant: 'CLASSIC',
        cape: '',
    },
)
defineEmits<{ switchAccount: []; website: [] }>()
const { formatMessage: t } = useVIntl()
</script>

<template>
    <div class="flex flex-col gap-5">
        <section
            class="mutmc-photo-hero relative isolate overflow-hidden rounded-2xl"
            data-onboarding-id="ysumc-welcome"
        >
            <img
                src="/brands/mutmc/hero.webp"
                alt=""
                class="absolute inset-0 -z-20 h-full w-full object-cover object-[center_35%]"
            />
            <div
                class="relative flex min-h-[23rem] flex-col items-start justify-center px-8 py-10 lg:px-12"
            >
                <p class="mb-5 text-xs font-semibold tracking-[0.2em]">MC UNIVERSITY TOURNAMENT</p>
                <h1 class="m-0 max-w-lg text-4xl font-bold leading-tight lg:text-5xl">
                    {{ t(messages.title) }}
                </h1>
                <p class="mb-7 mt-5 max-w-md text-base leading-relaxed">{{ t(messages.intro) }}</p>
                <div class="flex flex-wrap gap-3">
                    <RouterLink
                        to="/library"
                        class="mutmc-primary-link inline-flex items-center gap-2 rounded-xl px-5 py-3 font-bold"
                        >{{ t(messages.library) }}<RightArrowIcon class="h-4 w-4"
                    /></RouterLink>
                    <a
                        :href="launcherBrand.website"
                        class="inline-flex items-center gap-2 rounded-xl border border-solid border-white/40 px-5 py-3 font-semibold !text-white"
                        @click.prevent="$emit('website')"
                        >{{ t(messages.website) }}<ExternalIcon class="h-4 w-4"
                    /></a>
                </div>
            </div>
            <span class="absolute bottom-5 right-6 text-xs tracking-widest opacity-70"
                >PLAY · CONNECT · COMPETE</span
            >
        </section>
        <section
            class="flex flex-wrap items-center gap-5 rounded-2xl border border-solid border-divider bg-surface-2 px-6 py-5"
            data-onboarding-id="ysumc-account"
        >
            <div v-if="texture" class="h-24 w-16 shrink-0 overflow-hidden" aria-hidden="true">
                <SkinPreviewRenderer
                    :texture-src="texture"
                    :variant="variant"
                    :cape-src="cape"
                    :initial-rotation="Math.PI / 8"
                />
            </div>
            <img
                v-else
                src="/brands/mutmc/icon.webp"
                alt=""
                class="h-14 w-14 shrink-0 object-contain"
            />
            <div class="min-w-0 flex-1">
                <p class="mb-1 mt-0 text-xs font-semibold uppercase tracking-wider text-secondary">
                    {{ t(messages.account) }}
                </p>
                <h2 class="m-0 break-words text-xl font-bold">{{ name || 'MUTMCL' }}</h2>
                <p class="mb-0 mt-1 text-sm text-secondary">{{ t(messages.edition) }}</p>
            </div>
            <RouterLink
                to="/skins"
                class="inline-flex items-center gap-2 rounded-xl bg-surface-3 px-4 py-3 font-semibold !text-[var(--color-text-primary)] no-underline"
                ><ChangeSkinIcon class="h-5 w-5" />{{ t(messages.skins) }}</RouterLink
            >
            <Button @click="$emit('switchAccount')">{{ t(messages.switchAccount) }}</Button>
        </section>
    </div>
</template>
