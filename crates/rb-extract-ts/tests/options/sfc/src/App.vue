<template>
  <p>héllo {{ name }}</p>
</template>
<script lang="ts">
import { shared } from "./shared";
</script>
<script setup lang="ts">
import type { Props } from "./props";
import helper from "./helper.js";
</script>
<style>p { color: red }</style>
