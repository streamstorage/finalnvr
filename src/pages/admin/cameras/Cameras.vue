<template>
    <va-button class="mb-8" @click="showAddCameraModal = !showAddCameraModal"> Add camera </va-button>

    <va-modal
        v-model="showAddCameraModal"
        ok-text="Apply"
        no-dismiss
        @ok="
            addCamera(
                (formData.name as ComputedRef).value,
                (formData.location as ComputedRef).value,
                (formData.url as ComputedRef).value,
            )
        "
    >
        <h3 class="va-h3">New Camera</h3>
        <va-form ref="newCamera" stateful class="mb-2 flex flex-col gap-2">
            <va-input
                name="name"
                label="Name"
                :rules="[(value) => (value && value.length > 0) || 'Name is required']"
            />
            <va-input
                name="location"
                label="Location"
                :rules="[(value) => (value && value.length > 0) || 'Location is required']"
            />
            <va-input name="url" label="URL" :rules="[(value) => (value && value.length > 0) || 'URL is required']" />
        </va-form>
    </va-modal>

    <va-card>
        <va-card-content class="overflow-auto">
            <va-data-table :columns="addCameraColumns" :items="cameras" striped>
                <template #cell(id)="{ rowIndex }">
                    {{ rowIndex + 1 }}
                </template>
                <template #cell(status)="{ _, rowData }">
                    <va-badge :text="rowData.status" :color="rowData.status" />
                </template>
                <template #cell(actions)="{ _, rowData }">
                     <va-popover
                        placement="top"
                        message="Preview"
                    >
                        <va-button
                            preset="plain"
                            icon="preview"
                            @click="onPreview(rowData)"
                        />
                    </va-popover> 
                    <va-popover
                        placement="top"
                        message="Edit"
                    >
                        <va-button
                            preset="plain"
                            icon="edit"
                            class="ml-3"
                        />
                    </va-popover> 
                    <va-popover
                        placement="top"
                        message="Delete"
                    >
                        <va-button
                            preset="plain"
                            icon="delete"
                            class="ml-3"
                        />
                    </va-popover> 
                    <va-modal
                        v-model="rowData.showPreviewModal"
                        :before-close="beforeClosePreview"
                        blur
                        hide-default-actions
                        close-button
                    >
                        <h3 class="va-h3">{{ `${rowData.name}` }}</h3>
                        <video :id="rowData.id" preload="none" class="stream" autoplay></video>
                    </va-modal>
                </template>
            </va-data-table>
        </va-card-content>
    </va-card>
</template>

<script setup lang="ts">
    import { ICamera } from './Camera'
    import { Webrtc } from './Webrtc'
    import { ComputedRef, onMounted, ref } from 'vue'
    import { useForm } from 'vuestic-ui'
    
    const addCameraColumns = ref([
      { key: "id" },
      { key: "name", sortable: true },
      { key: "location" },
      { key: "url" },
      { key: "status" },
      { key: "actions", width: 100 },
    ]);

    const { formData, validate } = useForm('newCamera')
    const showAddCameraModal = ref(false)
    const cameras = ref([] as ICamera[])

    function setStatus(val: string) {
        val
    }

    function addCamera(name: string, location: string, url: string) {
        let isValid: boolean = validate()
        if (isValid) {
            const camera = {
                type: 'addCamera',
                name: name,
                location: location,
                url: url,
            }
            wsConn?.send(JSON.stringify(camera))
            showAddCameraModal.value = false
        } else {
            showAddCameraModal.value = true
        }
    }

    let previewId: string | undefined = undefined
    let webrtc: Webrtc | undefined

    function onPreview(camera: ICamera) {
        previewId = camera.id
        camera.showPreviewModal = true
        wsConn?.send(
            JSON.stringify({
                type: 'preview',
                id: camera.id,
                url: camera.url,
            }),
        )
    }

    function beforeClosePreview(hide: any) {
        if (previewId !== undefined) {
            wsConn?.send(
                JSON.stringify({
                    type: 'stopPreview',
                    id: previewId,
                }),
            )
        }
        webrtc?.close()
        webrtc = undefined
        previewId = undefined
        hide()
    }

    let wsPort = '8080'
    let wsUrl = `ws://${window.location.hostname}:${wsPort}/ws`
    let wsConn: WebSocket | undefined = undefined

    function connect() {
        console.log('Connecting listener')
        wsConn = new WebSocket(wsUrl)
        wsConn.addEventListener('open', () => {
            wsConn?.send(
                JSON.stringify({
                    type: 'setPeerStatus',
                    roles: ['listener'],
                }),
            )
            wsConn?.send(
                JSON.stringify({
                    type: 'listCameras',
                }),
            )
        })
        wsConn.addEventListener('error', onServerError)
        wsConn.addEventListener('message', onServerMessage)
        wsConn.addEventListener('close', onServerClose)
    }

    function onServerMessage(event: any) {
        console.log('Received ' + event.data)

        var msg: any
        try {
            msg = JSON.parse(event.data)
        } catch (e) {
            if (e instanceof SyntaxError) {
                console.error('Error parsing incoming JSON: ' + event.data)
            } else {
                console.error('Unknown error parsing response: ' + event.data)
            }
            return
        }

        if (msg.type == 'welcome') {
            console.info(`Got welcomed with ID ${msg.peer_id}`)
        } else if (msg.type == 'list') {
            // for (let i = 0; i < msg.producers.length; i++) {
            //     if (msg.producers[i].meta.id === previewId && previewId !== undefined) {
            //         console.log('Initiate webrtc connection')
            //         webrtc = new Webrtc(wsUrl, setStatus, msg.producers[i].id, previewId)
            //         return
            //     }
            // }
        } else if (msg.type == 'peerStatusChanged') {
            if (msg.roles.includes('producer') && msg.meta.id === previewId && previewId !== undefined) {
                console.log('Initiate webrtc connection')
                webrtc = new Webrtc(wsUrl, setStatus, msg.peerId, previewId)
            }
        } else if (msg.type == 'listCameras') {
            cameras.value = []
            for (let i = 0; i < msg.cameras.length; i++) {
                let val = msg.cameras[i]
                let camera: ICamera = {
                    id: val.id,
                    name: val.name,
                    location: val.location,
                    url: val.url,
                    status: 'INFO',
                    showPreviewModal: false,
                }
                cameras.value.push(camera)
            }
        } else {
            console.error('Unsupported message: ', msg)
        }
    }

    function clearConnection() {
        wsConn?.removeEventListener('error', onServerError)
        wsConn?.removeEventListener('message', onServerMessage)
        wsConn?.removeEventListener('close', onServerClose)
        wsConn = undefined
    }

    function onServerClose() {
        clearConnection()
        clearPeers()
        console.log('Close')
        window.setTimeout(connect, 1000)
    }

    function onServerError(event: any) {
        clearConnection()
        clearPeers()
        console.log('Error', event)
        window.setTimeout(connect, 1000)
    }

    function clearPeers() {
        previewId = undefined
        cameras.value.forEach((e) => {
            e.showPreviewModal = false
        })
    }

    onMounted(() => {
        connect()
    })
</script>

<style scoped>
    .stream {
        background-color: black;
        width: 720px;
    }
</style>
