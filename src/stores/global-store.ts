import { defineStore } from 'pinia'

export const useGlobalStore = defineStore('global', {
    state: () => {
        return {
            isSidebarMinimized: false,
            userName: 'Luis Liu',
        }
    },

    actions: {
        toggleSidebar() {
            this.isSidebarMinimized = !this.isSidebarMinimized
        },

        changeUserName(userName: string) {
            this.userName = userName
        },
    },
})
