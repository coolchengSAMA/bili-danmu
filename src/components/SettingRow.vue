<script setup lang="ts">
import { onMounted, ref } from 'vue'

// 设置页的一行：左侧标签 + 右侧控件（默认插槽）。
//
// 自身不写样式：.setting-row 与 .label 的规则在被引用页面的 scoped 样式里
// （子组件根节点会带上父作用域标记，故仍然生效）。
defineProps<{ label: string }>()

const row = ref<HTMLElement | null>(null)
// 行内是开关时整行可点（见 onRowClick）
const toggleable = ref(false)
onMounted(() => {
  toggleable.value = !!switchOf(row.value)
})

/**
 * 本行的开关；不满足「整行唯一控件」时返回 null
 *
 * 只有整行唯一控件就是这个开关时才让整行可点：行里还有别的控件时
 * （弹幕过滤页的荣耀等级行，标签里嵌着数字输入框），点歪到输入框旁边会误切开关。
 */
function switchOf(el: HTMLElement | null): HTMLElement | null {
  if (!el) return null
  const controls = el.querySelectorAll('input, select, textarea, button')
  if (controls.length !== 1) return null
  const only = controls[0] as HTMLElement
  return only.getAttribute('role') === 'switch' ? only : null
}

/**
 * 点行内空白 / 标签文字 = 点这一行的开关
 *
 * 起因：开关原来只有 17px 的方框能点，点「字号」「显示粉丝牌」这些文字毫无反应，
 * 用户会以为开关坏了。这里只接管「没点在控件上」的点击：
 * 点在滑块 / 下拉 / 按钮 / 输入框上时直接放行——那些行里有点一下就会真的执行操作的
 * 按钮（试听、发送测试弹幕、录制快捷键），误触发的后果比不切换开关更糟。
 * 开关换成 40×22 的滑动开关后命中区本身够大，所以不再把「行内空白」当成一个大按钮。
 */
function onRowClick(e: MouseEvent) {
  const el = e.target as HTMLElement | null
  if (!el || el.closest('input, select, textarea, button, a, label')) return
  switchOf(row.value)?.click()
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
.row-toggleable [role='switch'] {
  cursor: pointer;
}
</style>
