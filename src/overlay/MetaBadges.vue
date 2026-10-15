<script setup lang="ts">
import GuardChip from './GuardChip.vue'

import type { OverlayStyle } from '../types/ipc'

// 弹幕行 / 礼物行共用的身份徽章区：身份前缀（房管 / 舰长）+ 荣耀等级 + 粉丝牌。
// 单独成组件是为了让两区共用同一套列宽（role-slot 4.8em）——复制一份 CSS 后
// 只改单侧，两区的正文列就不在同一竖线上（role-slot 也决定 OverlayApp 的 --role-indent）。
//
// 粉丝牌由调用方以 medal 对象传入，不传就不渲染：弹幕行组装后传入，
// 礼物行不传（打赏行已有用户名与礼物名，粉丝牌只挤占横向空间）。
const props = defineProps<{
  /** 是否房管（礼物事件拿不到该字段，恒传 false） */
  isAdmin: boolean
  /** 舰队等级：3 舰长 / 2 提督 / 1 总督 */
  guardLevel?: number
  /** 荣耀等级（全站财富等级） */
  wealthLevel?: number
  /** 粉丝牌；不传 = 不渲染（礼物行就是这个情况）。isRoom 决定本房牌绿 / 其它房牌灰 */
  medal?: { name: string; level: number; isRoom: boolean }
  overlayStyle: OverlayStyle
}>()

/** 身份前缀列表（可叠加）：房管 + 舰长/提督/总督，按展示顺序 */
function rolesOf(): { kind: 'admin' | 'guard'; label: string; cls: string; level: number }[] {
  const roles: { kind: 'admin' | 'guard'; label: string; cls: string; level: number }[] = []
  if (props.isAdmin) {
    roles.push({ kind: 'admin', label: '房管', cls: 'admin', level: 0 })
  }
  const g = props.guardLevel ?? 0
  if (g >= 1 && g <= 3) {
    // 文案与三档配色统一由 GuardChip 提供（弹幕窗右侧在线榜也用它，两处不能各写一份）
    roles.push({ kind: 'guard', label: '', cls: 'guard', level: g })
  }
  return roles
}
</script>

<template>
  <!-- 徽章区：固定前缀列 + LV/粉丝牌等，单行不折行 -->
  <span class="meta">
    <!-- 身份前缀列（固定宽度，无前缀留空）→ LV/粉丝牌/文字各行垂直对齐 -->
    <span class="role-slot">
      <template v-if="overlayStyle.show_role">
        <template v-for="r in rolesOf()" :key="r.cls">
          <GuardChip v-if="r.kind === 'guard'" :level="r.level" />
          <span v-else class="chip role" :class="r.cls">{{ r.label }}</span>
        </template>
      </template>
    </span>
    <template v-if="overlayStyle.show_wealth && wealthLevel && wealthLevel > 0">
      <span class="chip lv">LV{{ wealthLevel }}</span>
    </template>
    <template v-if="medal && overlayStyle.show_medal">
      <span class="chip" :class="medal.isRoom ? 'medal-room' : 'medal-other'">{{ medal.name }}{{ medal.level }}</span>
    </template>
  </span>
</template>

<style scoped>
/* 徽章区：横向单行，不折行不压缩 */
.meta {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  white-space: nowrap;
  /* 与正文首行文字视觉中轴对齐。用 em 不用 px：
     徽章盒高是 0.74em x 1.7，正文行高 1.65em，两者都随字号变，
     固定 px 时字号一大就往下错开（字号 20 时 3px 已偏差 3px）。 */
  margin-top: 0.3em;
}

/* 身份前缀列：宽度 4.8em + 右间距 0.7em = 正文列起点。
   间距不能再小：面板背景左边缘要落在这一段的中间，
   否则荣耀等级 / 粉丝牌徽章会贴着背景边（见 OverlayApp 的 --panel-inset）。 */
.role-slot {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.2em;
  width: 4.8em;
  margin-right: 0.7em;
  overflow: hidden;
}

/* 前缀列内 chip 间距由 gap 控制，去掉全局右 margin（否则尾 chip 与 LV 间出现空隙） */
.role-slot .chip {
  margin-right: 0;
}

/* 身份徽章：inline-block 内联排版，数字等宽，最小宽度保证短内容对齐 */
.chip {
  display: inline-block;
  vertical-align: middle;
  font-size: 0.74em;
  line-height: 1.7;
  font-weight: 700;
  padding: 0 0.45em;
  margin-right: 0.35em;
  border-radius: 0.28em;
  text-shadow: none;
  text-align: center;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  transform: translateY(-1px);
}

/* 徽章区内 chip 不压缩（多 chip 叠加时保持自身宽度） */
.meta .chip {
  flex-shrink: 0;
}

/* 荣耀等级徽章：固定宽度（LV5 与 LV14 同宽） */
.chip.lv {
  min-width: 5.4ch;
  color: #6b4a00;
  background: linear-gradient(180deg, #ffe08a, #f0b429);
}

/* 粉丝牌徽章：较短牌名与“名+两位数”同宽 */
.chip.medal-room,
.chip.medal-other {
  min-width: 7.2ch;
}

/* 本房粉丝牌：绿色（与提督紫拉开区分） */
.chip.medal-room {
  color: #fff;
  background: linear-gradient(180deg, #4ade80, #16a34a);
}

/* 其它房粉丝牌 */
.chip.medal-other {
  color: #d6d9de;
  background: rgba(80, 80, 90, 0.9);
}

/* 单字身份标记（房/舰）方形居中 */
.chip.role {
  min-width: 2.6ch;
  color: #fff;
}

/* 房管：青绿色 teal（与舰长蓝拉开区分） */
.chip.role.admin {
  background: linear-gradient(180deg, #2dd4bf, #0d9488);
}
</style>
