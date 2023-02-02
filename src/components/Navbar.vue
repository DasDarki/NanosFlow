<script setup lang="ts">

</script>

<template>
  <div class="navbar card">
    <ul>
      <li><a href="">{{$t('nav.file')}}</a>
        <ul>
          <li><a href="">Resource</a></li>
          <li><a href="">Links</a></li>
        </ul>
      </li>
    </ul>
  </div>
</template>

<style scoped lang="scss">
.navbar {
  display: flex;
  flex-direction: column;
  height: 1.5rem;
}
</style>
