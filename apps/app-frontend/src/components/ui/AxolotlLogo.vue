<template>
    <div
        aria-label="YSUMC"
        class="flex h-full items-center gap-2 font-extrabold text-[var(--color-text-primary)]"
    >
        <img aria-hidden="true" class="aspect-square h-full object-contain" :src="axolotlVisual" />
        <span v-if="!iconOnly" class="hidden text-sm tracking-wide xl:inline">YSUMC</span>
    </div>
</template>

<script setup lang="ts">
const axolotlVisual = '/ysumc/logo.svg'

defineProps<{
    iconOnly?: boolean
}>()
</script>
