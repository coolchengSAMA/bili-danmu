<script setup lang="ts">
// 设置行的开关（替代原生 checkbox）
//
// 原生 17px 方框有两个问题：命中区太小（点旁边的文字毫无反应，所以 SettingRow 又做了整行可点）、
// 观感也不像系统设置页。这里做成 Windows / iOS 设置页那种滑动开关：轨道 + 滑块，
// 命中区 40×22，只动 transform / background-color。
//
// 交互与原生 input 对齐：点一下先 emit `update:modelValue`（父组件的 v-model 立刻更新），
// 再 emit `change`——所以父组件写 `v-model="x" @change="apply()"` 与用原生 input 时一模一样，
// 且 apply() 里读到的是**新值**。
const props = defineProps<{
  modelValue: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [boolean]
  change: []
}>()

function toggle() {
  emit('update:modelValue', !props.modelValue)
  emit('change')
}
</script>

<template>
  <button
    type="button"
    class="toggle"
    :class="{ on: modelValue }"
    :disabled="disabled"
    role="switch"
    :aria-checked="modelValue"
    @click="toggle">
    <span class="knob" />
  </button>
</template>

<style scoped>
.toggle {
  flex: 0 0 auto;
  position: relative;
  width: 40px;
  height: 22px;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: var(--border);
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.toggle:hover:not(:disabled) {
  background: var(--gray);
}

.toggle.on {
  background: var(--accent);
}

.toggle.on:hover:not(:disabled) {
  background: var(--accent-hover);
}

/* 滑块：只动 transform（合成层），不碰 left/margin，避免每帧重排 */
.knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
  transition: transform 0.15s ease;
}

.toggle.on .knob {
  transform: translateX(18px);
}

.toggle:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
