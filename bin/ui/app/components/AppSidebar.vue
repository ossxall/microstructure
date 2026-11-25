<script setup lang="ts">
import type { AppView } from '~/stores/navigation'

const navigationStore = useNavigationStore()

const items: { view: AppView, label: string, icon: string }[] = [{
  view: 'chart',
  label: 'Chart',
  icon: 'i-lucide-chart-no-axes-combined'
}]
</script>

<template>
  <aside class="flex min-h-screen w-16 shrink-0 flex-col border-r border-default bg-default px-2 py-4">
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
  </aside>
</template>
