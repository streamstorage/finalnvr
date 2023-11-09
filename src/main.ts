import App from './App.vue'
import i18n from './i18n'
import router from './router'
import vuesticGlobalConfig from './services/vuestic-ui/global-config'
import stores from './stores'
import axios from 'axios'
import { createApp } from 'vue'
import { createVuestic } from 'vuestic-ui'

const app = createApp(App)

app.use(stores)
app.use(router)
app.use(i18n)
app.use(createVuestic({ config: vuesticGlobalConfig }))

//axios.defaults.baseURL = 'http://127.0.0.1:8080';
app.provide('$http', axios);

app.mount('#app')
