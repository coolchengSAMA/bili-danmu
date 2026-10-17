<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'

import { DEFAULT_SENDER_CONFIG, type SenderConfig } from '../types/ipc'
import SettingRow from './SettingRow.vue'
import ToggleSwitch from './ToggleSwitch.vue'

// 「发送框」标签页：整条发送框的显示开关 + 全局快捷键录制。
//
// 场景：游戏里鼠标扫过发送框会误点（尤其右端那个「穿透」按钮），所以允许整条隐藏，
// 再用全局快捷键唤出回复弹幕。隐藏后有三个唤出入口：这里的快捷键、托盘菜单、本页开关。
// 快捷键必须系统级注册（前端 keydown 在游戏占前台时收不到），键位串由 Rust 解析。
const cfg = ref<SenderConfig>({ ...DEFAULT_SENDER_CONFIG })
// 录制态：点一下按钮进入，按下组合键即保存；Esc 取消；Backspace/Delete 清除
const recording = ref(false)
const pending = ref('')
const errMsg = ref('')
const savedTip = ref(false)
let unlistenVisible: UnlistenFn | undefined
let errTimer: ReturnType<typeof setTimeout> | undefined
let savedTimer: ReturnType<typeof setTimeout> | undefined

/** 只按了修饰键时也给个反馈，否则用户以为没录上 */
const MODIFIER_CODES = [
  'ControlLeft',
  'ControlRight',
  'ShiftLeft',
  'ShiftRight',
  'AltLeft',
  'AltRight',
  'MetaLeft',
  'MetaRight',
]

function showErr(e: unknown) {
  errMsg.value = String(e)
  if (errTimer) clearTimeout(errTimer)
  errTimer = setTimeout(() => (errMsg.value = ''), 4000)
}

function showSaved() {
  savedTip.value = true
  if (savedTimer) clearTimeout(savedTimer)
  savedTimer = setTimeout(() => (savedTip.value = false), 2000)
}

/** `Alt+KeyQ` → `Alt + Q`（给人看的写法；键码按 UI Events 名去掉 Key/Digit 前缀） */
function pretty(hotkey: string): string {
  if (!hotkey) return '未设置'
  return hotkey
    .split('+')
    .map(p => {
      if (p.startsWith('Key')) return p.slice(3)
      if (p.startsWith('Digit')) return p.slice(5)
      if (p === 'Space') return '空格'
      return p
    })
    .join(' + ')
}

/** 从键盘事件拼出与 Rust 解析约定一致的串：修饰键固定 Ctrl / Alt / Shift / Meta 顺序 + code */
function comboOf(e: KeyboardEvent): string {
  const parts: string[] = []
  if (e.ctrlKey) parts.push('Ctrl')
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  if (e.metaKey) parts.push('Meta')
  parts.push(e.code)
  return parts.join('+')
}

/** 回读 Rust 侧的快捷键注册失败原因；返回「当前是否处于注册失败态」 */
async function refreshHotkeyError(): Promise<boolean> {
  try {
    const e = await invoke<string | null>('sender_hotkey_error')
    if (e) {
      showErr(e)
      return true
    }
    return false
  } catch (e) {
    console.error('读取快捷键状态失败', e)
    return false
  }
}

/** 保存（Rust 先试注册快捷键，注册失败会整体不生效并把原因回传） */
async function save(next: SenderConfig) {
  errMsg.value = ''
  try {
    cfg.value = await invoke<SenderConfig>('sender_set_config', { sender: { ...next } })
    // 保存可能成功、但快捷键仍没注册上（例如键位在启动时就被占用、这次保存的可见性改动照常生效）：
    // 此时必须显示原因而不是「已保存」，否则用户会以为热键可用
    if (await refreshHotkeyError()) return
    showSaved()
  } catch (e) {
    showErr(e)
  }
}

/** 进入 / 退出录制：进出都清一次已按下的修饰键提示，避免显示上一次的残留 */
function startRecording() {
  recording.value = true
  pending.value = ''
}

function stopRecording() {
  recording.value = false
  pending.value = ''
}

async function onRecordKey(e: KeyboardEvent) {
  if (!recording.value) return
  e.preventDefault()
  e.stopPropagation()
  if (e.code === 'Escape') {
    stopRecording()
    return
  }
  if (MODIFIER_CODES.includes(e.code)) {
    pending.value = e.key
    return
  }
  if (e.code === 'Backspace' || e.code === 'Delete') {
    stopRecording()
    await save({ ...cfg.value, hotkey: '' })
    return
  }
  const hasModifier = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey
  const isFunctionKey = /^F([1-9]|1[0-9]|2[0-4])$/.test(e.code)
  if (!hasModifier && !isFunctionKey) {
    // 单字母/数字做全局热键会把普通输入也吃掉，故要求≥1 个修饰键（功能键除外）
    errMsg.value = '请至少带一个修饰键（Ctrl / Alt / Shift），或使用 F1-F24'
    return
  }
  stopRecording()
  await save({ ...cfg.value, hotkey: comboOf(e) })
}

onMounted(async () => {
  // 快捷键与托盘也能改可见性，监听广播保持勾选同步
  unlistenVisible = await listen<boolean>('sender-visible', e => {
    cfg.value.visible = e.payload
  })
  try {
    cfg.value = await invoke<SenderConfig>('sender_get_config')
  } catch (e) {
    console.error('读取发送框配置失败', e)
  }
  // 启动时快捷键可能已被游戏/其它程序占用：把原因显示出来，别让用户以为设置生效了
  await refreshHotkeyError()
})

onUnmounted(() => {
  unlistenVisible?.()
  if (errTimer) clearTimeout(errTimer)
  if (savedTimer) clearTimeout(savedTimer)
})
</script>

<template>
  <div class="setting-card">
    <p v-if="errMsg" class="panel-error">{{ errMsg }}</p>

    <SettingRow label="显示发送框">
      <ToggleSwitch :model-value="cfg.visible" @change="save({ ...cfg, visible: !cfg.visible })" />
    </SettingRow>

    <SettingRow label="显示/隐藏快捷键">
      <div class="hotkey-box">
        <button
          type="button"
          class="hotkey-btn"
          :class="{ recording }"
          @click="startRecording"
          @keydown="onRecordKey"
          @blur="stopRecording">
          {{ recording ? (pending ? pending + ' + …' : '请按下组合键…') : pretty(cfg.hotkey) }}
        </button>
        <button v-if="cfg.hotkey" type="button" class="clear-btn" @click="save({ ...cfg, hotkey: '' })">清除</button>
      </div>
    </SettingRow>

    <p class="tip">
      游戏里鼠标扫过发送框容易误点，可以把它整条隐藏，再用快捷键唤出来回复弹幕。
      唤出入口有三个：上面的快捷键、托盘右键菜单里的「显示/隐藏发送框」、以及本页的开关。
      点按钮后直接按组合键即可录制（Esc 取消、Backspace 清除）。
    </p>

    <p class="tip">
      快捷键默认不设置：全局热键会被系统级占用，其它程序（包括游戏）就收不到这个组合了，
      所以请自己挑一个不和游戏冲突的键。若提示注册失败，说明该组合已被别的程序占用，换一个即可； 想重录现在这一个，先按
      Backspace 清除再录（否则那一下会被本应用截走）。
    </p>

    <p class="tip">
      注意：隐藏发送框会连右端的「穿透」按钮一起隐藏。穿透的开关仍在主界面「弹幕窗」标签里，
      穿透开着时用托盘左键唤出主窗口即可关闭。
    </p>

    <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
  </div>
</template>

<style scoped>
/* 只在本标签页内提示，不与页面顶部的保存提示混用（同 OverlayWindowPanel） */
.panel-error {
  color: var(--red);
  font-size: 15px;
  padding: 6px 0 0;
}

.saved {
  color: var(--green);
  font-size: 14px;
  padding: 2px 0 0;
}

.hotkey-box {
  display: flex;
  align-items: center;
  gap: 8px;
}

.hotkey-btn {
  min-width: 11em;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-family: inherit;
  font-size: 15px;
  padding: 6px 10px;
  cursor: pointer;
}

.hotkey-btn:hover {
  border-color: var(--accent);
}

.hotkey-btn.recording {
  border-color: var(--accent);
  color: var(--accent);
}

.clear-btn {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 14px;
  padding: 2px 6px;
  border-radius: 4px;
  cursor: pointer;
}

.clear-btn:hover {
  background: var(--hover);
}
</style>
