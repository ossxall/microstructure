<script setup lang="ts">
const navigationStore = useNavigationStore()
</script>

<template>
  <div class="flex h-screen overflow-hidden bg-default text-default">
    <AppSidebar />

    <main class="h-screen min-h-0 min-w-0 flex-1 overflow-y-auto">
      <ChartView v-if="navigationStore.activeView === 'chart'" />
    </main>
  </div>
</template>
