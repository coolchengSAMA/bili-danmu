<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, ref } from 'vue'

import DanmakuFilterPanel from '../components/DanmakuFilterPanel.vue'
import OverlayWindowPanel from '../components/OverlayWindowPanel.vue'
import SaveTip from '../components/SaveTip.vue'
import SenderWindowPanel from '../components/SenderWindowPanel.vue'
import SettingRow from '../components/SettingRow.vue'
import { useSaveTip } from '../composables/useSaveTip'
import { emitDanmakuPreview, clearDanmakuPreview } from '../danmaku-preview'
import {
  DEFAULT_DANMAKU_FILTER,
  DEFAULT_OVERLAY_STYLE,
  type DanmakuFilter,
  type GiftConfig,
  type OverlayStyle,
  type RoomStatusEvent,
  type WelcomeConfig,
} from '../types/ipc'

// 初值用共用默认值，挂载后从 Rust 拉真实配置
const style = ref<OverlayStyle>({ ...DEFAULT_OVERLAY_STYLE })

// 页内标签：style / color / window / sender / filter
const tab = ref<'style' | 'color' | 'window' | 'sender' | 'filter'>('style')

// 弹幕过滤配置（应用后 Rust 广播给 Overlay 实时生效并持久化）
const filter = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER })

const { savedTip, opError, showSaved, showError } = useSaveTip()

const FONT_CHOICES = [
  { label: '微软雅黑 UI', value: 'Microsoft YaHei UI' },
  { label: '微软雅黑', value: 'Microsoft YaHei' },
  { label: '黑体', value: 'SimHei' },
  { label: '宋体', value: 'SimSun' },
  { label: '楷体', value: 'KaiTi' },
  { label: '等线', value: 'DengXian' },
  { label: 'Segoe UI', value: 'Segoe UI' },
]

async function apply() {
  try {
    await invoke('overlay_set_style', { style: { ...style.value } })
    showSaved()
  } catch (e) {
    showError(e)
  }
}

async function applyFilter(next: DanmakuFilter) {
  filter.value = next
  try {
    await invoke('danmaku_set_filter', { filter: { ...next } })
    showSaved()
  } catch (e) {
    showError(e)
  }
}

// 「发送测试弹幕」的进行态与结果提示（详见 src/danmaku-preview.ts 里的三条设计约束）
const previewBusy = ref(false)
const previewHint = ref('')

/** 往弹幕窗推一组本地测试弹幕：不发到直播间，只是走真实事件通道让弹幕窗画出来 */
async function sendPreview() {
  if (previewBusy.value) return
  previewBusy.value = true
  previewHint.value = ''
  try {
    // 弹幕窗隐藏时先显示，否则点了看不到任何反应（预览的目的就是「看一眼」）
    if (!(await invoke<boolean>('overlay_is_visible'))) {
      await invoke('overlay_set_visible', { visible: true })
    }
    const st = await invoke<RoomStatusEvent>('get_connection_status')
    const roomId = 'roomId' in st && st.roomId ? st.roomId : 0
    const gift = await invoke<GiftConfig>('gift_get_config')
    const welcome = await invoke<WelcomeConfig>('welcome_get_config')
    // 预览不绕过样式开关：关掉的部分不推，并在提示里说明，避免「怎么没有礼物行」的困惑
    const skipped: string[] = []
    if (!gift.enabled) skipped.push('礼物区')
    if (!welcome.enabled) skipped.push('欢迎信息')
    const n = await emitDanmakuPreview({
      roomId,
      includeGift: gift.enabled,
      includeWelcome: welcome.enabled,
    })
    previewHint.value = skipped.length
      ? `已发送 ${n} 条（${skipped.join(' / ')}已关闭，未包含对应样例）`
      : `已发送 ${n} 条`
  } catch (e) {
    previewHint.value = String(e)
  } finally {
    previewBusy.value = false
  }
}

/** 清掉弹幕窗里的预览行（真弹幕不受影响） */
async function clearPreview() {
  previewHint.value = ''
  try {
    await clearDanmakuPreview()
    previewHint.value = '已清除测试弹幕（真弹幕不受影响）'
  } catch (e) {
    previewHint.value = String(e)
  }
}

onMounted(async () => {
  try {
    style.value = await invoke<OverlayStyle>('overlay_get_style')
  } catch (e) {
    console.error('读取样式失败', e)
  }
  // 读取过滤配置初始值
  try {
    filter.value = await invoke<DanmakuFilter>('danmaku_get_filter')
  } catch (e) {
    console.error('读取过滤配置失败', e)
  }
})
</script>

<template>
  <div class="danmaku-page">
    <SaveTip :saved="savedTip" :error="opError" />
    <div class="tabs">
      <button
        v-for="t in [
          { key: 'style', label: '弹幕样式' },
          { key: 'color', label: '颜色与描边' },
          { key: 'window', label: '弹幕窗' },
          { key: 'sender', label: '发送框' },
          { key: 'filter', label: '弹幕过滤' },
        ]"
        :key="t.key"
        class="tab-btn"
        :class="{ active: tab === t.key }"
        @click="tab = t.key as 'style' | 'color' | 'window' | 'sender' | 'filter'">
        {{ t.label }}
      </button>
    </div>

    <section v-show="tab === 'style'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="字号">
          <div class="size-control">
            <input v-model.number="style.font_size" type="range" min="13" max="36" step="1" @change="apply()" />
            <span class="value">{{ style.font_size }}px</span>
          </div>
        </SettingRow>

        <SettingRow label="行间距">
          <div class="size-control">
            <input v-model.number="style.row_gap" type="range" min="0" max="30" step="1" @change="apply()" />
            <span class="value">{{ style.row_gap }}px</span>
          </div>
        </SettingRow>

        <SettingRow label="背景不透明度">
          <div class="size-control">
            <input v-model.number="style.bg_opacity" type="range" min="0" max="100" step="5" @change="apply()" />
            <span class="value">{{ style.bg_opacity }}%</span>
          </div>
        </SettingRow>

        <SettingRow label="字体">
          <select v-model="style.font_family" class="select" @change="apply()">
            <option v-for="f in FONT_CHOICES" :key="f.value" :value="f.value">
              {{ f.label }}
            </option>
          </select>
        </SettingRow>

        <SettingRow label="显示舰长/房管标记">
          <input v-model="style.show_role" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <SettingRow label="显示粉丝牌">
          <input v-model="style.show_medal" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <SettingRow label="显示荣耀等级">
          <input v-model="style.show_wealth" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <p class="tip">
          弹幕文字统一白色（黑色描边），任意背景下清晰。 背景不透明度作用于弹幕区与礼物区整块面板：0% = 完全透明，100% =
          纯黑。
        </p>

        <SettingRow label="测试弹幕">
          <div class="preview-box">
            <button type="button" class="save-btn" :disabled="previewBusy" @click="sendPreview">
              {{ previewBusy ? '发送中…' : '发送测试弹幕' }}
            </button>
            <button type="button" class="preview-clear" :disabled="previewBusy" @click="clearPreview">
              清除测试弹幕
            </button>
            <span v-if="previewHint" class="preview-hint">{{ previewHint }}</span>
          </div>
        </SettingRow>

        <p class="tip">
          在弹幕窗里按真实渲染路径画一组样例：普通观众 / 房管 / 舰长 / 提督 / 总督 / 荣耀等级 / 本房与其它房粉丝牌 /
          彩色弹幕 / 长文本换行；礼物区与欢迎信息开启时还会带上礼物、连击更新、醒目留言、上舰与欢迎行。
          只在本地弹幕窗显示，不会发到直播间；但行是真画上去的，推流时会出现在直播画面里。
          再点一次会替换上一组，「清除测试弹幕」把预览行全部去掉 —— 两种情况真弹幕一条都不会被删。
        </p>
      </div>
    </section>

    <section v-show="tab === 'color'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="用户名颜色">
          <input v-model="style.username_color" type="color" class="color-picker" @change="apply()" />
        </SettingRow>

        <SettingRow label="弹幕内容颜色">
          <input v-model="style.content_color" type="color" class="color-picker" @change="apply()" />
        </SettingRow>

        <SettingRow label="文字加粗">
          <input v-model="style.bold" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <SettingRow label="文字描边">
          <input v-model="style.outline" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <SettingRow label="描边颜色">
          <input v-model="style.outline_color" type="color" class="color-picker" @change="apply()" />
        </SettingRow>

        <SettingRow label="描边宽度">
          <div class="size-control">
            <input v-model.number="style.outline_width" type="range" min="0" max="5" step="1" @change="apply()" />
            <span class="value">{{ style.outline_width }}px</span>
          </div>
        </SettingRow>
      </div>
    </section>

    <section v-show="tab === 'window'" class="tab-pane">
      <OverlayWindowPanel />
    </section>

    <section v-show="tab === 'sender'" class="tab-pane">
      <SenderWindowPanel />
    </section>

    <section v-show="tab === 'filter'" class="tab-pane">
      <DanmakuFilterPanel :model-value="filter" verb="显示" @change="applyFilter" />
    </section>
  </div>
</template>

<style scoped>
.danmaku-page {
  display: flex;
  flex-direction: column;
}

/* 设置卡片 / 标签栏 / 开关 / 下拉 / 提示 / 滑块 / 数值见全局 src/styles/settings.css */
.select {
  min-width: 150px;
}

/* 测试弹幕：发送（主按钮，复用全局 .save-btn）+ 清除（次级按钮）+ 就地结果提示 */
.preview-box {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.preview-clear {
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-family: inherit;
  font-size: 15px;
  padding: 6px 12px;
  cursor: pointer;
}

.preview-clear:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--accent);
}

.preview-clear:disabled {
  opacity: 0.5;
  cursor: default;
}

.preview-hint {
  color: var(--text-faint);
  font-size: 14px;
}

.color-picker {
  width: 44px;
  height: 26px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: none;
  cursor: pointer;
}

.color-picker::-webkit-color-swatch-wrapper {
  padding: 2px;
}

.color-picker::-webkit-color-swatch {
  border: none;
  border-radius: 3px;
}
</style>
