<template>
    <va-button class="mb-8" @click="addCameraModal.show()"> Add camera </va-button>

    <camera-modal ref="addCameraModal" title="New Camera" @ok="addCamera" />
    <camera-modal ref="editCameraModal" title="Edit Camera" @ok="editCamera" />

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
                    <va-popover placement="top" message="Preview">
                        <va-button preset="plain" icon="preview" @click="onPreview(rowData)" />
                    </va-popover>
                    <va-popover placement="top" message="Edit">
                        <va-button
                            preset="plain"
                            icon="edit"
                            class="ml-3"
                            @click="editCameraModal.show(rowData.id, rowData.name, rowData.location, rowData.url)"
                        />
                    </va-popover>
                    <va-popover placement="top" message="Delete">
                        <va-button preset="plain" icon="delete" class="ml-3" />
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
    import CameraModal from './CameraModal.vue'
    import { Webrtc } from './Webrtc'
    import { onMounted, ref } from 'vue'

    const addCameraColumns = ref([
        { key: 'id' },
        { key: 'name', sortable: true },
        { key: 'location' },
        { key: 'url' },
        { key: 'status' },
        { key: 'actions', width: 100 },
    ])

    const addCameraModal = ref()
    const editCameraModal = ref()
    const cameras = ref([] as ICamera[])

    function setStatus(val: string) {
        val
    }

    function addCamera(_: string, name: string, location: string, url: string) {
        const msg = {
            type: 'addCamera',
            name,
            location,
            url,
        }
        wsConn?.send(JSON.stringify(msg))
    }

    function editCamera(id: string, name: string, location: string, url: string) {
        const msg = {
            type: 'editCamera',
            id,
            name,
            location,
            url,
        }
        wsConn?.send(JSON.stringify(msg))
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
            let array = []
            for (let i = 0; i < msg.cameras.length; i++) {
                let val = msg.cameras[i]
                const oldVal = cameras.value.find((v) => v.id === val.id)
                let camera: ICamera = {
                    id: val.id,
                    name: val.name,
                    location: val.location,
                    url: val.url,
                    status: 'INFO',
                    showPreviewModal: oldVal? oldVal.showPreviewModal: false,
                }
                array.push(camera)
            }
            cameras.value = array
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
