<script setup lang="ts">
import { CheckIcon, PlusIcon, SpinnerIcon, TrashIcon } from '@modrinth/assets'
import { Button, ConfirmModal, defineMessages, useVIntl } from '@modrinth/ui'
import { onUnmounted, ref } from 'vue'

import { type Cape, equip_cape, getCapePreviewUrl, remove_cape, upload_cape } from '@/helpers/skins'

const props = defineProps<{ accountId: string; capes: Cape[]; disabled?: boolean }>()
const emit = defineEmits<{ changed: [] }>()
const { formatMessage: t } = useVIntl()
const messages = defineMessages({
    title: { id: 'ysumc.capes.title', defaultMessage: 'Cape library' },
    hint: {
        id: 'ysumc.capes.hint',
        defaultMessage:
            'Upload a PNG cape to your YSUMC closet, then choose Wear. Use a 2:1 image such as 64 × 32 pixels.',
    },
    add: { id: 'ysumc.capes.add', defaultMessage: 'Add cape' },
    name: { id: 'ysumc.capes.name', defaultMessage: 'Cape name' },
    upload: { id: 'ysumc.capes.upload', defaultMessage: 'Upload cape' },
    cancel: { id: 'ysumc.capes.cancel', defaultMessage: 'Cancel' },
    wear: { id: 'ysumc.capes.wear', defaultMessage: 'Wear' },
    worn: { id: 'ysumc.capes.worn', defaultMessage: 'Wearing' },
    unequip: { id: 'ysumc.capes.unequip', defaultMessage: 'Take off cape' },
    remove: { id: 'ysumc.capes.remove', defaultMessage: 'Remove from closet' },
    removeHint: {
        id: 'ysumc.capes.remove-hint',
        defaultMessage: 'Remove this cape from your closet? If worn, it will also be taken off.',
    },
    invalid: {
        id: 'ysumc.capes.invalid',
        defaultMessage:
            'Choose a PNG file no larger than 1 MiB. Its width must be twice its height, up to 1024 × 512 pixels.',
    },
    empty: { id: 'ysumc.capes.empty', defaultMessage: 'No capes yet. Add your first cape here.' },
})
const fileInput = ref<HTMLInputElement>()
const file = ref<File>()
const preview = ref('')
const name = ref('')
const busy = ref(false)
const error = ref('')
const deleting = ref<Cape>()
const deleteModal = ref<InstanceType<typeof ConfirmModal>>()
let disposed = false

function clearFile() {
    if (preview.value) URL.revokeObjectURL(preview.value)
    preview.value = ''
    file.value = undefined
    if (fileInput.value) fileInput.value.value = ''
}
async function chooseFile(event: Event) {
    error.value = ''
    const selected = (event.target as HTMLInputElement).files?.[0]
    if (!selected) return
    clearFile()
    try {
        if (selected.type !== 'image/png' || selected.size > 1024 * 1024)
            throw new Error(t(messages.invalid))
        const bitmap = await createImageBitmap(selected)
        const valid =
            bitmap.height > 0 && bitmap.width === bitmap.height * 2 && bitmap.width <= 1024
        bitmap.close()
        if (!valid) throw new Error(t(messages.invalid))
        if (disposed) return
        file.value = selected
        name.value = selected.name.replace(/\.png$/i, '').slice(0, 80)
        preview.value = URL.createObjectURL(selected)
    } catch {
        error.value = t(messages.invalid)
    }
}
async function changeCape(action: () => Promise<unknown>) {
    if (busy.value || props.disabled) return
    busy.value = true
    error.value = ''
    try {
        await action()
        if (!disposed) emit('changed')
    } catch (reason) {
        if (!disposed) error.value = String(reason)
    } finally {
        busy.value = false
    }
}
async function upload() {
    const selected = file.value
    const accountId = props.accountId
    const capeName = name.value.trim()
    if (!selected || !capeName) return
    await changeCape(async () => {
        await upload_cape(accountId, new Uint8Array(await selected.arrayBuffer()), capeName)
        clearFile()
    })
}
function confirmRemoval(cape: Cape) {
    deleting.value = cape
    deleteModal.value?.show()
}
async function remove() {
    const cape = deleting.value
    if (!cape) return
    await changeCape(() => remove_cape(props.accountId, cape.id))
    deleting.value = undefined
}
onUnmounted(() => {
    disposed = true
    clearFile()
})
</script>

<template>
    <section
        class="mb-5 border border-solid border-divider bg-surface-2 p-4"
        aria-labelledby="cape-library-title"
    >
        <div class="flex flex-wrap items-center justify-between gap-3">
            <h2 id="cape-library-title" class="m-0 text-xl">{{ t(messages.title) }}</h2>
            <div class="flex flex-wrap gap-2">
                <Button
                    v-if="capes.some((cape) => cape.is_equipped)"
                    :disabled="busy || disabled"
                    @click="changeCape(() => equip_cape(accountId, null))"
                    >{{ t(messages.unequip) }}</Button
                >
                <Button :disabled="busy || disabled" @click="fileInput?.click()"
                    ><PlusIcon />{{ t(messages.add) }}</Button
                >
            </div>
        </div>
        <p class="text-sm text-secondary">{{ t(messages.hint) }}</p>
        <input ref="fileInput" type="file" accept="image/png" class="hidden" @change="chooseFile" />
        <form
            v-if="file"
            class="mb-4 flex flex-wrap items-end gap-3 bg-surface-3 p-3"
            @submit.prevent="upload"
        >
            <img
                :src="preview"
                alt=""
                class="h-16 w-32 object-contain [image-rendering:pixelated]"
            />
            <label class="flex min-w-0 flex-1 flex-col gap-1"
                >{{ t(messages.name)
                }}<input v-model="name" maxlength="80" required :disabled="busy"
            /></label>
            <Button :disabled="busy || disabled || !name.trim()" @click="upload"
                ><SpinnerIcon v-if="busy" class="animate-spin" />{{ t(messages.upload) }}</Button
            >
            <Button :disabled="busy" @click="clearFile">{{ t(messages.cancel) }}</Button>
        </form>
        <p v-if="error" role="alert" class="break-words text-red">{{ error }}</p>
        <p v-if="!capes.length" class="mb-0 text-sm text-secondary">{{ t(messages.empty) }}</p>
        <div
            v-else
            class="grid max-h-80 grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3 overflow-y-auto"
        >
            <article
                v-for="cape in capes"
                :key="cape.id"
                class="min-w-0 border border-solid border-divider p-3"
                :class="{ 'border-brand': cape.is_equipped }"
            >
                <img
                    :src="getCapePreviewUrl(cape.texture)"
                    :alt="cape.name"
                    class="h-20 w-full object-contain [image-rendering:pixelated]"
                />
                <p class="my-2 break-words text-sm">{{ cape.name }}</p>
                <div class="flex items-center gap-2">
                    <Button
                        class="min-w-0 flex-1"
                        :disabled="busy || disabled || cape.is_equipped"
                        @click="changeCape(() => equip_cape(accountId, cape.id))"
                        ><CheckIcon v-if="cape.is_equipped" />{{
                            t(cape.is_equipped ? messages.worn : messages.wear)
                        }}</Button
                    >
                    <Button
                        icon-only
                        :aria-label="t(messages.remove)"
                        :disabled="busy || disabled"
                        @click="confirmRemoval(cape)"
                        ><TrashIcon
                    /></Button>
                </div>
            </article>
        </div>
        <ConfirmModal
            ref="deleteModal"
            :title="t(messages.remove)"
            :description="t(messages.removeHint)"
            :proceed-label="t(messages.remove)"
            @proceed="remove"
        />
    </section>
</template>
