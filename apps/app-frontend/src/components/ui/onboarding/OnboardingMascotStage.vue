<script setup lang="ts">
import { isMutmc } from '@/branding'

defineProps<{
    alt: string
}>()
const mascotVisual = isMutmc ? '/brands/mutmc/icon.webp' : '/ysumc/logo.svg'
</script>

<template>
    <img class="onboarding-mascot" :src="mascotVisual" :alt="alt" />
</template>

<style scoped lang="scss">
.onboarding-mascot {
    width: clamp(6rem, 11vw, 9rem);
    height: auto;
    object-fit: contain;
}
</style>
