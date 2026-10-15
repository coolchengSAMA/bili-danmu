<script setup lang="ts">
import { computed } from 'vue'

import GuardChip from './GuardChip.vue'
import { rowShadow } from './row-style'

import type { AudienceSnapshot, OverlayStyle } from '../types/ipc'

// 弹幕窗右侧那一列：上面「在看 N 人」，下面「实时在线榜」前 5 名。
//
// 口径（实测）：榜单 = 此刻在房间里 ∩ 今天有贡献值的人，人离开直播间 ≤20 秒就掉榜；
// 潜水观众永远不在榜上（B 站隐私边界），所以这里只当「有没有人、谁在互动」的快速一瞥，
// 不是完整观众名单。昵称沿用弹幕行的颜色/字重/描边（同源 OverlayStyle），
// 看起来与弹幕是同一套文字。
const props = defineProps<{
  snapshot: AudienceSnapshot
  overlayStyle: OverlayStyle
  /** 是否显示「在看 N 人」 */
  showOnline: boolean
  /** 是否显示「实时在线榜」 */
  showRank: boolean
}>()

/** 榜上昵称与弹幕用户名同一套样式（颜色 + 字重 + 描边来自同一份 OverlayStyle） */
const nameStyle = computed(() => ({
  color: props.overlayStyle.username_color,
  fontWeight: props.overlayStyle.bold ? 800 : 600,
}))

/** 描边与弹幕行共用 rowShadow（写两份必然漂移） */
const shadow = computed(() => rowShadow(props.overlayStyle))
</script>

<template>
  <div class="audience-column" :style="{ textShadow: shadow }">
    <div v-if="showOnline" class="online-line">
      <span class="online-label">在看</span>
      <span class="online-num">{{ snapshot.online_text }}</span>
    </div>
    <div v-if="showRank" class="rank" :class="{ 'rank-solo': !showOnline }">
      <p v-if="snapshot.users.length === 0" class="rank-empty">暂无人在榜</p>
      <div v-for="u in snapshot.users" :key="u.uid" class="rank-row">
        <span class="rank-name" :style="nameStyle">{{ u.name }}</span>
        <GuardChip :level="u.guard_level" />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 右列内容整体靠左贴住面板底色右边缘（与左侧头衔列贴左边缘对称），
   字号比弹幕正文小一档：这一列只需扫一眼，不该和弹幕抢注意力。 */
.audience-column {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.35em;
  font-size: 0.82em;
  line-height: 1.5;
  overflow: hidden;
}

.online-line {
  display: flex;
  align-items: baseline;
  gap: 0.3em;
  white-space: nowrap;
}

.online-label {
  font-size: 0.9em;
  color: rgba(255, 255, 255, 0.75);
}

/* 人数比标签大一点、亮一点：一眼先看到的就是它 */
.online-num {
  font-size: 1.1em;
  font-weight: 700;
  color: #fff;
}

.rank {
  display: flex;
  flex-direction: column;
  gap: 0.15em;
  width: 100%;
  padding-top: 0.35em;
  border-top: 1px solid rgba(255, 255, 255, 0.25);
}

/* 只显示榜单（不显示人数）时不需要那条分隔线 */
.rank-solo {
  padding-top: 0;
  border-top: none;
}

.rank-row {
  display: flex;
  align-items: center;
  gap: 0.25em;
  /* 昵称放不下就换行（与弹幕一样折行，不裁掉文字） */
  flex-wrap: wrap;
}

.rank-name {
  word-break: break-word;
}

.rank-empty {
  color: rgba(255, 255, 255, 0.6);
}
</style>
