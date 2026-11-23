<script setup lang="ts">
const navigationStore = useNavigationStore()
</script>

<template>
  <div class="flex min-h-screen bg-default text-default">
    <AppSidebar />

    <main class="min-w-0 flex-1">
      <ChartView v-if="navigationStore.activeView === 'chart'" />
    </main>
  </div>
</template>
