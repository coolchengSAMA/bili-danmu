<script setup lang="ts">
import { computed } from 'vue'

// 舰队档位徽章（舰长 / 提督 / 总督）。
//
// 抽成组件是因为它现在有两个使用场景：弹幕行的 MetaBadges，以及弹幕窗右侧在线榜。
// 复制一份 CSS 会造成两处配色漂移——与「徽章列共用契约」同一条理由（见 Agents.md）。
const props = defineProps<{
  /** 3 舰长 / 2 提督 / 1 总督；0 或缺失时不渲染 */
  level?: number
}>()

const info = computed(() => {
  switch (props.level) {
    case 3:
      return { label: '舰长', cls: 'captain' }
    case 2:
      return { label: '提督', cls: 'admiral' }
    case 1:
      return { label: '总督', cls: 'governor' }
    default:
      return null
  }
})
</script>

<template>
  <span v-if="info" class="guard-chip" :class="info.cls">{{ info.label }}</span>
</template>

<style scoped>
/* 与 MetaBadges 的 .chip 同一套尺寸（em 为单位，跟弹幕字号一起缩放） */
.guard-chip {
  display: inline-block;
  vertical-align: middle;
  flex-shrink: 0;
  font-size: 0.74em;
  line-height: 1.7;
  font-weight: 700;
  padding: 0 0.45em;
  border-radius: 0.28em;
  text-shadow: none;
  text-align: center;
  white-space: nowrap;
  color: #fff;
  min-width: 2.6ch;
  transform: translateY(-1px);
}

/* 舰长：蓝色（比房管青绿深，避免与「房管」混淆） */
.guard-chip.captain {
  background: linear-gradient(180deg, #4facfe, #1f6fd0);
}

/* 提督：紫色（比本房粉丝牌绿拉开区分） */
.guard-chip.admiral {
  background: linear-gradient(180deg, #c084fc, #7e22ce);
}

/* 总督：金渐变红（最高档） */
.guard-chip.governor {
  background: linear-gradient(180deg, #ffcc4d, #e4413a);
}
</style>
