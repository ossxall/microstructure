import { defineStore } from 'pinia'
import { ref } from 'vue'

export type AppView = 'chart'

export const useNavigationStore = defineStore('navigation', () => {
  const activeView = ref<AppView>('chart')

  function setActiveView(view: AppView) {
    activeView.value = view
  }

  return {
    activeView,
    setActiveView
  }
})
