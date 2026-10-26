<script setup lang="ts">
import { RightArrowIcon } from '@modrinth/assets'
import { useVIntl } from '@modrinth/ui'

import { mutmcMessages as messages } from './messages'

withDefaults(defineProps<{ error?: string; busy?: boolean }>(), { error: '', busy: false })
defineEmits<{ login: [] }>()
const { formatMessage: t } = useVIntl()
</script>

<template>
    <div
        class="grid min-h-[40rem] overflow-hidden rounded-2xl border border-solid border-divider bg-surface-2 md:grid-cols-[1.15fr_1fr]"
    >
        <div
            class="mutmc-photo-hero relative isolate flex min-h-64 flex-col justify-between overflow-hidden p-8 lg:p-12"
        >
            <img
                src="/brands/mutmc/hero.webp"
                alt=""
                class="absolute inset-0 -z-20 h-full w-full object-cover"
            />
            <img src="/brands/mutmc/logo.webp" alt="MUT" class="w-32 object-contain" />
            <div class="my-12 max-w-sm">
                <p class="text-xs tracking-[0.2em]">MC UNIVERSITY TOURNAMENT</p>
                <h2 class="text-4xl font-bold leading-tight">{{ t(messages.title) }}</h2>
                <p class="leading-relaxed">{{ t(messages.intro) }}</p>
            </div>
            <p class="m-0 text-xs tracking-widest opacity-70">PLAY · CONNECT · COMPETE</p>
        </div>
        <div class="flex flex-col justify-center px-8 py-12 lg:px-12">
            <p class="mb-4 text-xs font-bold tracking-[0.15em] text-secondary">MUTMCL / LAUNCHER</p>
            <h1 class="m-0 text-3xl font-bold leading-tight">{{ t(messages.loginTitle) }}</h1>
            <p class="mb-9 mt-5 max-w-md leading-relaxed text-secondary">
                {{ t(messages.loginIntro) }}
            </p>
            <button
                type="button"
                class="mutmc-primary-link flex w-full items-center justify-center gap-3 rounded-xl border-0 px-5 py-4 text-base font-bold disabled:opacity-60"
                :disabled="busy"
                @click="$emit('login')"
            >
                <svg viewBox="0 0 24 24" aria-hidden="true" class="h-5 w-5 fill-current">
                    <path d="M1 1h10v10H1zM13 1h10v10H13zM1 13h10v10H1zM13 13h10v10H13z" />
                </svg>
                {{ t(messages.login) }}<RightArrowIcon class="ml-auto h-5 w-5" />
            </button>
            <p
                v-if="error"
                role="alert"
                class="mt-4 rounded-lg border border-solid border-divider bg-surface-3 p-4 text-sm"
            >
                {{ error }}
            </p>
            <p class="mb-0 mt-5 max-w-md text-xs leading-relaxed text-secondary">
                {{ t(messages.privacy) }}
            </p>
            <p
                class="mb-0 mt-12 border-0 border-t border-solid border-divider pt-5 text-xs text-secondary"
            >
                MINECRAFT JAVA EDITION
            </p>
        </div>
    </div>
</template>
