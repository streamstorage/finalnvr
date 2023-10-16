<template>
    <va-modal v-model="showModal" ok-text="Apply" no-dismiss @ok="onClick(name, location, url)">
        <h3 class="va-h3">{{ props.title }}</h3>
        <va-form ref="camera" class="mb-2 flex flex-col gap-2">
            <va-input
                v-model="name"
                label="Name"
                :rules="[(value) => (value && value.length > 0) || 'Name is required']"
            />
            <va-input
                v-model="location"
                label="Location"
                :rules="[(value) => (value && value.length > 0) || 'Location is required']"
            />
            <va-input
                v-model="url"
                label="URL"
                :rules="[(value) => (value && value.length > 0) || 'URL is required']"
            />
        </va-form>
    </va-modal>
</template>

<script setup lang="ts">
    import { ref } from 'vue'
    import { useForm } from 'vuestic-ui'

    const showModal = ref(false)
    const name = ref('')
    const location = ref('')
    const url = ref('')
    let id = ''

    const props = defineProps({
        title: { type: String, required: true },
    })

    const emit = defineEmits<{
        (e: 'ok', id: string, name: string, location: string, url: string): void
    }>()

    const { validate } = useForm('camera')
    function onClick(name: string, location: string, url: string) {
        let isValid: boolean = validate()
        if (isValid) {
            emit('ok', id, name, location, url)
            showModal.value = false
        } else {
            showModal.value = true
        }
    }

    function show(i?: string, n?: string, l?: string, u?: string) {
        if (i) id = i
        if (n) name.value = n
        if (l) location.value = l
        if (u) url.value = u
        showModal.value = true
    }

    defineExpose({
        show,
    })
</script>
