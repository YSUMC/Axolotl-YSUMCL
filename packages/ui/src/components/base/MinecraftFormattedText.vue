<script setup lang="ts">
import { autoCleanToText } from '@sfirew/minecraft-motd-parser'
import { computed } from 'vue'

const props = defineProps<{
    text: string
}>()

// Content names use the surrounding theme color, not Minecraft's in-game formatting.
const plainText = computed(() => autoCleanToText(props.text))
</script>

<template>
    <span>{{ plainText }}</span>
</template>
