<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'

import SettingRow from './SettingRow.vue'

// 「弹幕窗」标签页：显示 / 鼠标穿透 / 始终置顶 / 宽高。
//
// 状态全部留在本组件内（与弹幕样式、弹幕过滤无耦合），父页面只管挂载。
// 设置卡片、设置行与开关的样式来自全局 styles/settings.css；
// 失败提示用独立的 .panel-error，避免与页面的 .error 样式互相影响。
const winSize = ref({ width: 480, height: 240 })
// 三个开关的初值与 Rust 侧 OverlayState 的默认值一致（创建窗口时就是这三态），
// 避免读到真实状态前先闪一下错误勾选；挂载后再用命令读一次对齐
const ov = ref({ visible: true, clickthrough: false, alwaysOnTop: true })
// 右侧在线列的两个显示开关（默认开，与 config/types.rs 的 OverlayStyle::default 一致）
const online = ref({ showOnline: true, showRank: true })
const errMsg = ref('')
let errTimer: ReturnType<typeof setTimeout> | undefined
// 窗口被手动拖动边缘缩放时由 Rust 广播 overlay-size 同步过来
let unlistenSize: UnlistenFn | undefined
// 开关从发送框那一侧被改动时由 Rust 广播过来（两个入口必须双向同步）
let unlistenVisible: UnlistenFn | undefined
let unlistenClickthrough: UnlistenFn | undefined

/** 操作失败就地提示（3s 后自动消失），不打断页面其它区域的保存提示 */
function fail(e: unknown) {
  errMsg.value = String(e)
  if (errTimer) clearTimeout(errTimer)
  errTimer = setTimeout(() => (errMsg.value = ''), 3000)
}

async function setVisible(v: boolean) {
  errMsg.value = ''
  try {
    ov.value.visible = await invoke<boolean>('overlay_set_visible', { visible: v })
  } catch (e) {
    fail(e)
  }
}

async function setClickthrough(v: boolean) {
  errMsg.value = ''
  try {
    ov.value.clickthrough = await invoke<boolean>('overlay_set_clickthrough', {
      enabled: v,
    })
  } catch (e) {
    fail(e)
  }
}

async function setAlwaysOnTop(v: boolean) {
  errMsg.value = ''
  try {
    ov.value.alwaysOnTop = await invoke<boolean>('overlay_set_always_on_top', {
      enabled: v,
    })
  } catch (e) {
    fail(e)
  }
}

/** 提交窗口宽高（滑块拖动结束后才调用，避免拖动中反复改窗口） */
async function commitSize() {
  try {
    await invoke('overlay_set_size', {
      width: winSize.value.width,
      height: winSize.value.height,
    })
  } catch (e) {
    fail(e)
  }
}

/**
 * 切换右侧在线列的显示
 *
 * 只提交这两个字段（Rust 侧做 load-modify-save），不提交整份样式：
 * 否则会把「弹幕样式」页正在改的其它字段一起覆盖掉。
 */
async function setAudience(patch: Partial<{ showOnline: boolean; showRank: boolean }>) {
  errMsg.value = ''
  const next = { ...online.value, ...patch }
  try {
    await invoke('overlay_set_audience', {
      showOnline: next.showOnline,
      showRank: next.showRank,
    })
    online.value = next
  } catch (e) {
    fail(e)
  }
}

/**
 * 把弹幕窗搬回主屏中央
 *
 * 换显示器 / 改分辨率 / 拔掉副屏之后，记忆的位置可能落在不存在的坐标上：
 * 弹幕窗看不见、也拖不回来（设置页只能改宽高，没有位置入口），这是那条兜底路径的入口。
 */
async function resetPosition() {
  errMsg.value = ''
  try {
    await invoke('overlay_reset_position')
  } catch (e) {
    fail(e)
  }
}

onMounted(async () => {
  // 先在监听：读取尺寸与手动缩放都走同一条路，不必区分先后
  unlistenSize = await listen<{ width: number; height: number }>('overlay-size', e => {
    winSize.value = e.payload
  })
  unlistenVisible = await listen<boolean>('overlay-visible', e => {
    ov.value.visible = e.payload
  })
  unlistenClickthrough = await listen<boolean>('overlay-clickthrough', e => {
    ov.value.clickthrough = e.payload
  })
  try {
    winSize.value = await invoke<{ width: number; height: number }>('overlay_get_size')
  } catch (e) {
    console.error('读取尺寸失败', e)
  }
  try {
    ov.value.visible = await invoke<boolean>('overlay_is_visible')
    ov.value.clickthrough = await invoke<boolean>('overlay_get_clickthrough')
  } catch (e) {
    console.error('读取弹幕窗状态失败', e)
  }
  // 右侧在线列的两个开关存在 OverlayStyle 里，读一次对齐（与「弹幕样式」页共用同一份配置）
  try {
    const st = await invoke<{ show_online: boolean; show_rank: boolean }>('overlay_get_style')
    online.value = { showOnline: st.show_online, showRank: st.show_rank }
  } catch (e) {
    console.error('读取弹幕样式失败', e)
  }
})

onUnmounted(() => {
  unlistenSize?.()
  unlistenVisible?.()
  unlistenClickthrough?.()
  if (errTimer) clearTimeout(errTimer)
})
</script>

<template>
  <div class="setting-card">
    <p v-if="errMsg" class="panel-error">{{ errMsg }}</p>

    <SettingRow label="显示弹幕窗">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.visible"
        @change="setVisible(($event.target as HTMLInputElement).checked)" />
    </SettingRow>

    <SettingRow label="鼠标穿透">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.clickthrough"
        @change="setClickthrough(($event.target as HTMLInputElement).checked)" />
    </SettingRow>

    <SettingRow label="始终置顶">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.alwaysOnTop"
        @change="setAlwaysOnTop(($event.target as HTMLInputElement).checked)" />
    </SettingRow>

    <SettingRow label="宽度">
      <div class="size-control">
        <input v-model.number="winSize.width" type="range" min="240" max="1600" step="10" @change="commitSize" />
        <span class="value">{{ winSize.width }}px</span>
      </div>
    </SettingRow>

    <SettingRow label="高度">
      <div class="size-control">
        <input v-model.number="winSize.height" type="range" min="120" max="900" step="10" @change="commitSize" />
        <span class="value">{{ winSize.height }}px</span>
      </div>
    </SettingRow>

    <SettingRow label="窗口位置">
      <button type="button" class="ghost-btn" @click="resetPosition">移回主屏中央</button>
    </SettingRow>

    <SettingRow label="显示在看人数">
      <input
        type="checkbox"
        class="switch"
        :checked="online.showOnline"
        @change="setAudience({ showOnline: ($event.target as HTMLInputElement).checked })" />
    </SettingRow>

    <SettingRow label="显示实时在线榜">
      <input
        type="checkbox"
        class="switch"
        :checked="online.showRank"
        @change="setAudience({ showRank: ($event.target as HTMLInputElement).checked })" />
    </SettingRow>

    <p class="tip">
      「在看 N 人」与「实时在线榜（前 5）」显示在弹幕窗右侧那一列，每 30 秒刷新一次； 榜单只统计今天在房间里互动过（投喂
      / 点赞 / 弹幕）的观众，潜水观众不在榜上。 两个开关都关掉时右列整列收起，弹幕区恢复原宽。
    </p>

    <p class="tip">
      关闭穿透后，可在弹幕窗上按住拖动、边缘调整大小；开启穿透后鼠标点击会落到弹幕窗下方的窗口。
      位置和大小自动保存，下次启动恢复。换了显示器、改了分辨率或拔掉副屏之后，若弹幕窗跑到看不见的地方，
      点「移回主屏中央」把它搬回来（尺寸保持原样，只有比屏幕还大时才收窄）。
      注意「鼠标穿透」与「始终置顶」是本次运行的状态，不写进配置：重启后回到默认（不穿透、置顶开）。
    </p>
  </div>
</template>

<style scoped>
/* 滑块与数值标签见全局 src/styles/settings.css（.size-control / .value） */

/* 次级按钮（与朗读页的「刷新」同一套观感）：位置兜底这类一次性动作用它，不抢主操作的视线 */
.ghost-btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 14px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  white-space: nowrap;
}

.ghost-btn:hover {
  color: var(--text);
  border-color: var(--accent);
}

/* 只在本标签页内提示，不与页面顶部的样式/过滤保存提示混用 */
.panel-error {
  color: var(--red);
  font-size: 15px;
  padding: 6px 0 0;
}
</style>
