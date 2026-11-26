<script setup lang="ts">
import type { AppView } from '~/stores/navigation'

const navigationStore = useNavigationStore()
const colorMode = useColorMode()
const colorModeReady = ref(false)

onMounted(() => {
  colorModeReady.value = true
})

const items: { view: AppView, label: string, icon: string }[] = [{
  view: 'chart',
  label: 'Chart',
  icon: 'i-lucide-chart-no-axes-combined'
}]

function toggleColorMode() {
  colorMode.preference = colorMode.value === 'dark' ? 'light' : 'dark'
}
</script>

<template>
  <aside class="flex h-screen w-16 shrink-0 flex-col border-r border-default bg-default px-2 py-4">
    <nav
      aria-label="Main navigation"
      class="flex flex-col gap-2"
    >
      <button
        v-for="item in items"
        :key="item.view"
        type="button"
        :aria-current="navigationStore.activeView === item.view ? 'page' : undefined"
        :aria-label="item.label"
        class="flex w-full flex-col items-center gap-2 rounded-lg px-2 py-3 text-muted transition-colors hover:bg-elevated hover:text-default focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        :class="navigationStore.activeView === item.view ? 'bg-primary/10 text-primary' : undefined"
        @click="navigationStore.setActiveView(item.view)"
      >
        <UIcon
          :name="item.icon"
          class="size-5"
          aria-hidden="true"
        />
        <span class="text-xs font-medium">{{ item.label }}</span>
      </button>
    </nav>

    <button
      type="button"
      :aria-label="!colorModeReady
        ? 'Toggle color mode'
        : colorMode.value === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'"
      :title="!colorModeReady
        ? 'Toggle color mode'
        : colorMode.value === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'"
      class="mt-auto flex w-full flex-col items-center gap-2 rounded-lg px-2 py-3 text-muted transition-colors hover:bg-elevated hover:text-default focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
      @click="toggleColorMode"
    >
      <UIcon
        :name="!colorModeReady
          ? 'i-lucide-sun'
          : colorMode.value === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon'"
        class="size-5"
        aria-hidden="true"
      />
      <span class="text-xs font-medium">{{
        !colorModeReady
          ? 'Theme'
          : colorMode.value === 'dark' ? 'Light' : 'Dark'
      }}</span>
    </button>
  </aside>
</template>
