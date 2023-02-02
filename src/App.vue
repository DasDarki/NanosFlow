<script setup lang="ts">
import Navbar from "@/components/Navbar.vue";
</script>

<template>
  <div class="app-container">
    <Navbar />
  </div>
</template>

<style scoped lang="scss">
.app-container {
  width: 100%;
  height: 100vh;
  position: absolute;
  top: 0;
  left: 0;
  display: flex;
  flex-direction: column;
  overflow-x: hidden;
  overflow-y: auto;
}
</style>
