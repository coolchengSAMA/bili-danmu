<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import type { OverlayStyle, RoomStatusEvent } from '../types/ipc'

const danmakuText = ref('')
// 输入框元素：窗口被快捷键唤出时直接聚焦，做到「唤出即可打字」
const inputEl = ref<HTMLInputElement | null>(null)
const sending = ref(false)
const sendError = ref('')
const loggedIn = ref(false)
const connected = ref(false)
// 鼠标穿透是否开启：开关按钮在发送框右端，发送框是独立窗口，穿透开启后仍可点
const clickthrough = ref(false)
// 弹幕字号（与弹幕窗统一，界面元素按 em 相对缩放）
const fontSize = ref(17)

let unlistenRoom: UnlistenFn | undefined
let unlistenStyle: UnlistenFn | undefined
// 穿透从主界面那一侧被改动时由 Rust 广播过来（两个入口必须双向同步）
let unlistenClickthrough: UnlistenFn | undefined
// 发送框被快捷键 / 托盘唤出时由 Rust 广播，用于把光标放进输入框
let unlistenShown: UnlistenFn | undefined
let pollTimer: ReturnType<typeof setInterval> | undefined

const canSend = computed(() => loggedIn.value && connected.value)

// 未登录 / 未连接时输入框是禁用的（发弹幕要登录态 + 房间会话），
// 但禁用态原本没有任何文案，只有「点不动、打不了字」，很容易被当成输入框坏了，故补上原因
const inputPlaceholder = computed(() => {
  if (!loggedIn.value) return '登录后可发送弹幕'
  if (!connected.value) return '连接直播间后可输入'
  return '输入弹幕，回车发送'
})
const disabledHint = computed(() => {
  if (!loggedIn.value) return '未登录：请先在主界面登录 B 站账号'
  if (!connected.value) return '未连接直播间：请先在主界面点「连接」'
  return ''
})

// 输入法组合态：中文输入法里回车是「选词上屏」，不能当成发送
const composing = ref(false)

async function sendDanmaku() {
  const text = danmakuText.value.trim()
  if (!text || sending.value || !canSend.value) return
  sendError.value = ''
  sending.value = true
  try {
    await invoke('send_danmaku', { msg: text })
    danmakuText.value = ''
  } catch (e) {
    sendError.value = String(e)
  } finally {
    sending.value = false
  }
}

/**
 * 回车发送
 *
 * 必须挡掉输入法组合态：中文输入法里按回车是「选词上屏」，Chromium 派发顺序是
 * keydown(isComposing=true) → compositionend → keyup(isComposing 已是 false)，
 * 所以用 Vue 的 `@keyup.enter` 会把半成品拼音直接发出去（`v-model` 有组合态守卫，
 * 按键修饰符没有）。改成监听 keydown，再自己 + 事件两侧各判一次组合态。
 */
function onEnter(e: KeyboardEvent) {
  // 按住回车会重复派发 keydown：旧实现监听 keyup 不会，这里自己挡掉
  if (e.repeat || composing.value || e.isComposing) return
  void sendDanmaku()
}

/** 切换鼠标穿透（发送框隐藏时本按钮不可用，此时主界面「弹幕窗」标签 / 托盘仍可关掉穿透） */
async function toggleClickthrough() {
  try {
    clickthrough.value = await invoke<boolean>('overlay_set_clickthrough', { enabled: !clickthrough.value })
  } catch (e) {
    sendError.value = String(e)
  }
}

async function poll() {
  try {
    const info = await invoke<{ loggedIn: boolean }>('get_login_info')
    loggedIn.value = info.loggedIn
  } catch {
    /* ignore */
  }
  try {
    const st = await invoke<RoomStatusEvent>('get_connection_status')
    connected.value = st.state === 'connected'
  } catch {
    /* ignore */
  }
  try {
    clickthrough.value = await invoke<boolean>('overlay_get_clickthrough')
  } catch {
    /* ignore */
  }
}

onMounted(async () => {
  unlistenRoom = await listen<RoomStatusEvent>('room-status', e => {
    connected.value = e.payload.state === 'connected'
  })
  unlistenStyle = await listen<OverlayStyle>('overlay-style', e => {
    fontSize.value = e.payload.font_size
  })
  unlistenClickthrough = await listen<boolean>('overlay-clickthrough', e => {
    clickthrough.value = e.payload
  })
  // 快捷键/托盘唤出：窗口 show 之后再聚焦输入框（窗口隐藏期间前端一直在跑，事件收得到）
  unlistenShown = await listen('sender-shown', () => {
    inputEl.value?.focus()
  })
  try {
    const s = await invoke<OverlayStyle>('overlay_get_style')
    fontSize.value = s.font_size
  } catch {
    /* ignore */
  }
  await poll()
  pollTimer = setInterval(poll, 2000)
})

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer)
  unlistenRoom?.()
  unlistenStyle?.()
  unlistenClickthrough?.()
  unlistenShown?.()
})
</script>

<template>
  <div class="sender-root" :style="{ fontSize: fontSize + 'px' }">
    <!-- 悬停提示挂在行容器上：disabled 的表单元素在 Chromium 里不派发鼠标事件，挂在 input 自己身上不出提示 -->
    <div class="input-row" :title="disabledHint || undefined">
      <input
        ref="inputEl"
        v-model="danmakuText"
        class="send-input"
        type="text"
        :placeholder="inputPlaceholder"
        maxlength="100"
        :disabled="!canSend"
        @compositionstart="composing = true"
        @compositionend="composing = false"
        @keydown.enter="onEnter" />
      <button
        type="button"
        class="ct-btn"
        :class="{ on: clickthrough }"
        :title="clickthrough ? '鼠标穿透：开（点击关闭）' : '鼠标穿透：关（点击开启）'"
        @click="toggleClickthrough">
        穿透
      </button>
    </div>
    <div v-if="sendError" class="status-line">
      <span class="err">{{ sendError }}</span>
    </div>
  </div>
</template>

<style scoped>
.sender-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  box-sizing: border-box;
  overflow: hidden;
  padding: 0.25em 0;
}

.input-row {
  display: flex;
  /* stretch 而非 center：两个子元素天然等高（字号/padding 一致），stretch 再上一层保险 */
  align-items: stretch;
  gap: 0.25em;
}

/* 输入区字号比弹幕小一档（0.9em）：发送框宽度受「不能挡太多游戏画面」约束
   （面板宽度的 50%、上限 260px），字号 1em 时 210px 的框只放得下 6 个中文字，
   连 placeholder 都会截尾。要再宽只能改 window.rs 的 SENDER_WIDTH_RATIO。 */
.send-input {
  flex: 1 1 auto;
  min-width: 0;
  appearance: none;
  -webkit-appearance: none;
  background: rgba(0, 0, 0, 0.55);
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 0.4em;
  color: #fff;
  padding: 0.3em;
  font-size: 0.9em;
  outline: none;
  user-select: text;
}

.send-input::placeholder {
  color: rgba(255, 255, 255, 0.5);
}

.send-input:focus {
  border-color: rgba(120, 180, 255, 0.95);
  border-radius: 0.4em;
}

/* 未登录 / 未连接时的禁用态：底色与描边一起变淡，明确「现在不能输入」而不是坏了
   （原样式显式写了 color / background / border，不加这条时禁用态与可用态几乎一样） */
.send-input:disabled {
  color: rgba(255, 255, 255, 0.4);
  border-color: rgba(255, 255, 255, 0.15);
  cursor: not-allowed;
}

.send-input:disabled::placeholder {
  color: rgba(255, 255, 255, 0.32);
}

/* 穿透开关：发送框是独立窗口，穿透开启后本按钮仍可点击，是「关掉穿透」的唯一入口
   （发送框整条隐藏时则用主界面「弹幕窗」标签或托盘）。字号 / 上下 padding 与 .send-input
   保持一致：em 是相对自身字号的，字号小一档行高框与 padding 会跟着缩，
   按钮就比输入框矮（0.8em 时矮 3.5px，看着不齐）。 */
.ct-btn {
  flex: 0 0 auto;
  appearance: none;
  -webkit-appearance: none;
  cursor: pointer;
  white-space: nowrap;
  font-size: 0.9em;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(0, 0, 0, 0.55);
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 0.4em;
  padding: 0.3em 0.45em;
  transition:
    background 0.12s ease,
    color 0.12s ease,
    border-color 0.12s ease;
}

.ct-btn:hover {
  border-color: rgba(255, 255, 255, 0.6);
}

.ct-btn.on {
  color: #fff;
  background: rgba(120, 180, 255, 0.55);
  border-color: rgba(120, 180, 255, 0.95);
}

.status-line {
  padding: 0.15em 0.15em 0;
  font-size: 0.75em;
  flex-shrink: 0;
  min-height: 1.2em;
  color: rgba(255, 255, 255, 0.85);
  text-shadow:
    0 0 2px rgba(0, 0, 0, 0.9),
    0 0 2px rgba(0, 0, 0, 0.9);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.err {
  color: #ff7b72;
}
</style>
