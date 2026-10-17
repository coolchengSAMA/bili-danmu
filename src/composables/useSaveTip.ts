import { ref } from 'vue'

/**
 * 设置页的轻提示（**全应用单例**）：同一时刻只显示「已保存」或「操作失败」中的一条，
 * 到时自动消失。两个状态互斥（显示其一会清掉另一个），避免保存成功后错误提示还挂在那里。
 *
 * 状态放在模块作用域、`SaveTip` 只在 `App.vue` 挂载一次，任何组件（含 fork 自己写的
 * 弹幕窗 / 发送框标签页这类子组件）直接调用 `showSaved()` / `showError()` 即可，不需要
 * emit / prop 透传——否则又会退化成「有的页面用右下角浮层、有的页面内联、有的干脆没有」。
 */
const savedTip = ref(false)
const opError = ref('')
let timer: ReturnType<typeof setTimeout> | undefined

function clearTimer() {
  if (timer) clearTimeout(timer)
  timer = undefined
}

/** 显示「已应用并保存」（1.2s 后自动消失） */
function showSaved() {
  clearTimer()
  opError.value = ''
  savedTip.value = true
  timer = setTimeout(() => (savedTip.value = false), 1200)
}

/** 显示操作失败原因（3s 后自动消失） */
function showError(e: unknown) {
  clearTimer()
  savedTip.value = false
  opError.value = String(e)
  timer = setTimeout(() => (opError.value = ''), 3000)
}

// 单例不需要 onUnmounted 清理：组件卸载后提示仍会在 1.2s / 3s 后自己消失
export function useSaveTip() {
  return { savedTip, opError, showSaved, showError }
}
