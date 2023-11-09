<template>
    <div class="cards">
        <div class="cards-container grid grid-cols-12 items-start gap-6 wrap">
            <va-card v-for="video, index in videos" :key="index" 
                class="w-full col-span-12 sm:col-span-6 md:col-span-3">
                <va-image src="https://picsum.photos/300/200/?image=898" style="height: 200px">
                    <va-button class="m-0">
                        Play
                    </va-button>
                </va-image>
                <va-card-content>{{ video.id }}</va-card-content>
            </va-card>
        </div>

        <va-inner-loading class="w-full py-4 justify-center" :loading="isLoading">
            <va-button @click="loadVideos()">
                {{ t('cards.button.showMore') }}
            </va-button>
        </va-inner-loading>
    </div>
</template>

<script setup lang="ts">
    import { IVideo } from './Video'
    import { inject, onMounted, ref } from 'vue'
    import { useI18n } from 'vue-i18n'
    const { t } = useI18n()
    const $http: any = inject('$http')

    const isLoading = ref(false)
    const videos = ref([] as IVideo[])
    var ctoken = ''

    function loadVideos() {
        isLoading.value = true
        $http.get('/v1/videos', {...((ctoken!= '') && {params: {ctoken}})})
            .then((res: any) => {
                if (res.data == null) {
                    return
                }
                ctoken = res.data[1].token
                res.data[0].forEach((el: any) => {
                    let video: IVideo = {
                        id: el.stream.name,
                        name: 'name'
                    }
                    videos.value.push(video)
                });
            })
            .catch((e: any) => {
                console.error('Failed to get videos:', e)
            })
            .finally(() => {
                isLoading.value = false
            });
    }
    
    onMounted(() => {
        loadVideos()
    })
</script>
