<script setup lang="ts">
import { computed } from 'vue'
import { statusClass, statusLabel } from '@/lib/format'

const props = withDefaults(
  defineProps<{
    status?: string | null
    /** 是否显示状态点 */
    dot?: boolean
    /** 自定义文案，缺省时用中文状态名 */
    label?: string
  }>(),
  { status: 'UNKNOWN', dot: true, label: '' },
)

const text = computed(() => props.label || statusLabel(props.status))
</script>

<template>
  <span class="badge" :class="statusClass(props.status)">
    <span v-if="props.dot" class="dot"></span>{{ text }}
  </span>
</template>
