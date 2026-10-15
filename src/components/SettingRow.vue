<script setup lang="ts">
import { onMounted, ref } from 'vue'

// 设置页的一行：左侧标签 + 右侧控件（默认插槽）。
//
// 自身不写样式：.setting-row 与 .label 的规则在被引用页面的 scoped 样式里
// （子组件根节点会带上父作用域标记，故仍然生效）。
defineProps<{ label: string }>()

const row = ref<HTMLElement | null>(null)
// 行内是复选框时整行可点（见 onRowClick）
const toggleable = ref(false)
onMounted(() => {
  toggleable.value = !!row.value?.querySelector('input[type="checkbox"]')
})

/**
 * 点行内空白 / 标签文字 = 点这一行的开关
 *
 * 起因：开关只有 17px 的方框能点，点「字号」「显示粉丝牌」这些文字毫无反应，
 * 用户会以为开关坏了。这里只接管「没点在控件上」的点击：
 * 点在滑块 / 下拉 / 按钮 / 输入框上时直接放行——那些行里有点一下就会真的执行操作的
 * 按钮（试听、发送测试弹幕、录制快捷键），误触发的后果比不切换开关更糟。
 * 只找复选框：滑块、下拉这些行不做「整行可点」，避免点一下就把值改了。
 */
function onRowClick(e: MouseEvent) {
  const el = e.target as HTMLElement | null
  if (!el || el.closest('input, select, textarea, button, a, label')) return
  row.value?.querySelector<HTMLInputElement>('input[type="checkbox"]')?.click()
}
</script>

<template>
  <div ref="row" class="setting-row" :class="{ 'row-toggleable': toggleable }" @click="onRowClick">
    <span class="label">{{ label }}</span>
    <slot />
  </div>
</template>

<style scoped>
/* 可点行给出鼠标提示：整行空白与标签文字都能切换开关 */
.row-toggleable,
.row-toggleable .switch {
  cursor: pointer;
}
</style>
