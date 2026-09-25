<script lang="ts">
import { format } from './format';
export abstract class Shape<T> {
  readonly label = format(1, 2);
  abstract area(): T;
}
</script>

<template>
  <p>shape</p>
</template>
