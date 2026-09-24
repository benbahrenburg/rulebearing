<script setup lang="ts" generic="T extends Record<string, unknown>">
import { format } from './format';
import Item from './item';
const props = defineProps<{ value: T }>();
const shown = format(props.value, Item);
</script>

<template>
  <p>{{ shown }}</p>
</template>
