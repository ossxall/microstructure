<template>
  <section
    class="flex min-h-full flex-col gap-6 p-6 sm:p-8"
    aria-labelledby="chart-title"
  >
    <header>
      <p class="text-sm font-medium text-muted">
        Workspace
      </p>
      <h1
        id="chart-title"
        class="mt-1 text-2xl font-semibold text-highlighted"
      >
        Chart
      </h1>
    </header>

    <div class="flex min-h-96 flex-1 items-center justify-center rounded-xl border border-default bg-elevated/30">
      <div class="text-center">
        <UIcon
          name="i-lucide-chart-no-axes-combined"
          class="mx-auto mb-3 size-8 text-muted"
          aria-hidden="true"
        />
        <h2 class="text-base font-medium text-highlighted">
          Chart view
        </h2>
      </div>
    </div>
  </section>
</template>
