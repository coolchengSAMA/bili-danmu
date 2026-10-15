<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

import {
  DEFAULT_DANMAKU_FILTER,
  DEFAULT_GIFT_CONFIG,
  DEFAULT_OVERLAY_STYLE,
  type AudienceSnapshot,
  type BackingEvent,
  type DanmakuEvent,
  type DanmakuFilter,
  type DisplayDanmaku,
  type GiftConfig,
  type OverlayStyle,
  type RoomStatusEvent,
  type WelcomeConfig,
  type WelcomeEvent,
} from '../types/ipc'
import AudienceColumn from './AudienceColumn.vue'
import DanmakuRow from './DanmakuRow.vue'
import GiftRow from './GiftRow.vue'

const danmakuList = ref<DisplayDanmaku[]>([])
// 礼物列表：Rust 侧已做完连击合并与金额门槛，这里只负责按 id 覆盖与条数上限
const giftList = ref<BackingEvent[]>([])
const giftCfg = ref<GiftConfig>({ ...DEFAULT_GIFT_CONFIG })
const currentRoomId = ref(0)
// 连接状态（空窗时显示初始化提示，避免无界面窗口）
const connState = ref<'disconnected' | 'connected'>('disconnected')
// 弹幕过滤配置（设置页变更 → Rust 广播 danmaku-filter 事件 → 实时生效）
const filter = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER })
// 弹幕样式（字号/字体/是否用原色）；初值用共用默认值，挂载后从 Rust 拉真实配置
const style = ref<OverlayStyle>({ ...DEFAULT_OVERLAY_STYLE })
// 「在看 N 人 + 实时在线榜」：Rust 每 30 秒推一次 `audience-update`
const audience = ref<AudienceSnapshot | null>(null)
// 「发送测试弹幕」推的预览快照：有它就盖住真实数据，清除预览后自动回落到真实值
const audiencePreview = ref<AudienceSnapshot | null>(null)
const shownAudience = computed(() => audiencePreview.value ?? audience.value)

/**
 * 右侧在线列是否占位（决定面板右留白）
 *
 * 三种情况都要让列出现：
 * - 有预览快照：没连接也要能看（「发送测试弹幕」的用途就是不上播也能验证样式）
 * - 已连接但数据还没到：先占位，否则数字一到面板宽度会跳一下
 * - 未连接且只剩旧的真实快照：不显示（那是上一场的数字，留着会误导）
 */
const audienceOn = computed(() => {
  if (!(style.value.show_online || style.value.show_rank)) return false
  if (audiencePreview.value !== null) return true
  return connState.value === 'connected'
})

// 弹幕区 / 礼物区共用的半透明黑（背景不透明度 0-100 → alpha 0-1）
const bgColor = computed(() => `rgba(0, 0, 0, ${(style.value.bg_opacity / 100).toFixed(2)})`)

const MAX_ITEMS = 120

/** 连接状态 → 界面状态（事件与轮询共用，去重） */
function applyConnStatus(st: RoomStatusEvent) {
  if (st.state === 'connected') {
    const was = connState.value === 'connected'
    const prevRoom = currentRoomId.value
    connState.value = 'connected'
    currentRoomId.value = st.roomId
    // 进入（或换房）时：清空旧提示并以系统行提示入场
    if (!was || prevRoom !== st.roomId) {
      danmakuList.value = []
      giftList.value = []
      pushSystem(`已进入直播间 ${st.roomId}，等待弹幕…`)
    }
  } else if (st.state === 'connecting') {
    if (connState.value !== 'connecting') {
      connState.value = 'connecting'
      danmakuList.value = []
      giftList.value = []
      pushSystem('连接中…')
    }
  } else {
    if (connState.value !== 'disconnected') {
      connState.value = 'disconnected'
      danmakuList.value = []
      giftList.value = []
      pushSystem('未连接 · 请在主界面连接直播间')
    }
  }
}

/** 追加一行并裁到上限（弹幕 / 系统提示 / 欢迎信息共用同一份列表与上限） */
function pushRow(row: DisplayDanmaku) {
  const list = danmakuList.value
  list.push(row)
  if (list.length > MAX_ITEMS) {
    list.splice(0, list.length - MAX_ITEMS)
  }
}

/** 推送系统提示行（无用户名，纯文本，与弹幕同样式） */
function pushSystem(text: string) {
  pushRow({
    id: `sys-${Date.now()}-${danmakuList.value.length}`,
    username: '',
    content: text,
    timestamp: Date.now() / 1000,
    is_admin: false,
    isRoomMedal: false,
  })
}

/** 追加一条弹幕，超出上限丢弃最旧的 */
function pushDanmaku(d: DisplayDanmaku) {
  pushRow(d)
}

/** 舰队档位名（guard_level：3 舰长 / 2 提督 / 1 总督） */
const GUARD_LABEL: Record<number, string> = { 0: '大航海', 1: '总督', 2: '提督', 3: '舰长' }

/**
 * 欢迎信息文案：`▸` 前缀让它在弹幕里一眼可辨
 *
 * 文字颜色改成与弹幕同亮度的淡青后，层次靠色相；再给个前缀符号，
 * 即使有人把弹幕正文色也改成青色也能分得出来。
 */
function welcomeText(w: WelcomeEvent): string {
  switch (w.kind) {
    case 'enter':
      return `▸ ${w.username} 进入直播间`
    case 'follow':
      return `▸ ${w.username} 关注了直播间`
    case 'share':
      return `▸ ${w.username} 分享了直播间`
    case 'like':
      return `▸ ${w.username} 点赞了直播间`
    default:
      return `▸ ${w.username} 乘坐${GUARD_LABEL[w.guard_level ?? 0]}进入直播间`
  }
}

/**
 * 推送一条欢迎提示行（无用户名、不显示徽章，与系统提示行同一形态）
 *
 * 去重与限速都在 Rust 侧做完（`welcome.rs`），这里只渲染收到的行；
 * 开关关掉时 Rust 直接不广播，所以无需在前端再判一次配置。
 */
function pushWelcome(w: WelcomeEvent) {
  pushRow({
    id: w.id,
    username: '',
    content: welcomeText(w),
    timestamp: w.timestamp,
    is_admin: false,
    isRoomMedal: false,
    isWelcome: true,
    isPreview: !!w.preview,
  })
}

/**
 * 清掉上一组预览行（设置页再次点「发送测试弹幕」时由 `preview-reset` 触发）
 *
 * **只删带预览标记的行**：真弹幕、真礼物、真欢迎行一条都不动。预览行 id 也带 `preview-` 前缀，
 * 与真弹幕不会撞。测试弹幕只是「本地多画几行」，不参与任何真实数据。
 */
function clearPreviewRows() {
  danmakuList.value = danmakuList.value.filter(d => !d.isPreview)
  giftList.value = giftList.value.filter(b => !b.preview)
}

/**
 * 关掉欢迎信息时清掉列表里还没滚出去的欢迎行
 *
 * 欢迎行与弹幕混在同一份列表里，不像礼物区那样有个 `v-if` 能整块隐藏，
 * 不主动清的话最多会残留 120 条。
 */
function dropWelcomeRows() {
  danmakuList.value = danmakuList.value.filter(d => !d.isWelcome)
}

/**
 * 写入一条礼物行：同一连击分组的更新复用同一 id，按 id 覆盖（行位置不变），
 * 超出「礼物区最多显示条数」时丢弃最旧的。
 */
function upsertGift(b: BackingEvent) {
  const list = giftList.value
  const i = list.findIndex(x => x.id === b.id)
  if (i >= 0) {
    list[i] = b
  } else {
    list.push(b)
  }
  const max = giftMaxRows()
  if (list.length > max) {
    list.splice(0, list.length - max)
  }
}

/** 补上「粉丝牌是否来自当前房间」的展示标记（决定本房牌绿 / 其它房牌灰）
 *
 * 预览行例外：测试弹幕在未连接直播间时也要能看到「本房牌」配色，此时 currentRoomId 为 0，
 * 正常判定会把所有牌都判成「其它房」，故预览行直接用它自带 is_room_medal 标记。
 *
 * isPreview 一起在这里补上（而不是调用方再展开一次）：这是每条弹幕都要走的路径，
 * 高频直播间每秒几十条，能省一次对象拷贝就省一次。 */
function markRoomMedal<T extends { medal_room_id?: number; preview?: boolean; is_room_medal?: boolean }>(
  d: T,
): T & { isRoomMedal: boolean; isPreview: boolean } {
  const isRoom =
    d.preview && d.is_room_medal !== undefined
      ? d.is_room_medal
      : d.medal_room_id !== undefined && currentRoomId.value !== 0 && d.medal_room_id === currentRoomId.value
  return { ...d, isRoomMedal: isRoom, isPreview: !!d.preview }
}

/** 礼物区行数上限（至少 1，防止配置被改成 0 时列表全空） */
function giftMaxRows(): number {
  return Math.max(1, giftCfg.value.max_rows)
}

// 调小「最多显示条数」时立刻裁掉多出来的行，否则要等下一条礼物才生效
watch(giftMaxRows, max => {
  const list = giftList.value
  if (list.length > max) {
    list.splice(0, list.length - max)
  }
})

let unlistenDanmu: UnlistenFn | undefined
let unlistenRoom: UnlistenFn | undefined
let unlistenStyle: UnlistenFn | undefined
let unlistenFilter: UnlistenFn | undefined
let unlistenGift: UnlistenFn | undefined
let unlistenGiftCfg: UnlistenFn | undefined
let unlistenWelcome: UnlistenFn | undefined
let unlistenWelcomeCfg: UnlistenFn | undefined
// 设置页再点「发送测试弹幕」时先清上一组预览行（只清带 preview 标记的）
let unlistenPreviewReset: UnlistenFn | undefined
// 在线人数 / 实时在线榜（Rust 每 30 秒推一次）
let unlistenAudience: UnlistenFn | undefined

/// 弹幕过滤判定：
/// 身份规则（舰长/房管、有粉丝牌、荣耀等级）为「或」关系——任一开启的规则命中即显示；
/// 身份规则全关 = 不过滤；敏感词屏蔽独立叠加——开关开启且命中词表时整条丢弃（不豁免）。
function shouldShowDanmaku(d: DanmakuEvent, f: DanmakuFilter): boolean {
  if (f.enable_sensitive && f.sensitive_words.some(w => w && d.content.toLowerCase().includes(w.toLowerCase()))) {
    return false
  }
  const anyIdentityOn = f.enable_guard_admin || f.enable_medal || f.enable_wealth
  if (!anyIdentityOn) return true
  return (
    (f.enable_guard_admin && (d.is_admin || (d.guard_level ?? 0) >= 1)) ||
    (f.enable_medal && (d.medal_level ?? 0) > 0) ||
    (f.enable_wealth && (d.wealth_level ?? 0) >= f.wealth_min)
  )
}

/** 从面板底色左边缘开始拖拽缩放（左边缘是头衔列，见模板里的 .resize-handle-west）
 *
 * 交给系统做：DPI、min_inner_size、贴边吸附都走窗口自己的逻辑，与拖系统边缘等价。
 * 必须 preventDefault，否则会命中根元素的 data-tauri-drag-region，变成拖动窗口。 */
async function startResizeWest(e: MouseEvent) {
  e.preventDefault()
  try {
    await getCurrentWindow().startResizeDragging('West')
  } catch (err) {
    console.error('从面板左边缘缩放失败', err)
  }
}

/** 从窗口右缘拖拽缩放（弹幕窗右侧现在是「在线列」，没有可见的面板边缘可抓，
 *  所以手柄放在窗口右缘——系统热区只有 3px，这里放宽到 9px 更好抓） */
async function startResizeEast(e: MouseEvent) {
  e.preventDefault()
  try {
    await getCurrentWindow().startResizeDragging('East')
  } catch (err) {
    console.error('从窗口右缘缩放失败', err)
  }
}

onMounted(async () => {
  unlistenRoom = await listen<RoomStatusEvent>('room-status', e => {
    applyConnStatus(e.payload)
  })
  unlistenDanmu = await listen<DanmakuEvent>('danmaku', e => {
    const d = e.payload as DisplayDanmaku
    // 过滤：不满足配置规则（敏感词命中/身份不匹配）的弹幕整条丢弃。
    // 预览行（设置页「发送测试弹幕」）绕过过滤：它的用途是看徽章/样式，被内容规则拦掉会让人以为按钮坏了
    if (!d.preview && !shouldShowDanmaku(d, filter.value)) {
      return
    }
    pushDanmaku(markRoomMedal(d))
  })
  unlistenGift = await listen<BackingEvent>('gift', e => {
    upsertGift(e.payload)
  })
  unlistenGiftCfg = await listen<GiftConfig>('gift-config', e => {
    giftCfg.value = e.payload
  })
  unlistenWelcome = await listen<WelcomeEvent>('welcome', e => {
    pushWelcome(e.payload)
  })
  // 关掉开关时把残留的欢迎行清掉（开的那一侧不用管：Rust 会开始广播）
  unlistenWelcomeCfg = await listen<WelcomeConfig>('welcome-config', e => {
    if (!e.payload.enabled) {
      dropWelcomeRows()
    }
  })
  // 测试弹幕：每次推送新一组前先清掉上一组预览行（真弹幕不受影响）
  unlistenPreviewReset = await listen('preview-reset', () => {
    clearPreviewRows()
    // 预览的在线人数 / 榜单也一起撤掉，回落到真实快照（未连接时列会整条消失）
    audiencePreview.value = null
  })
  // 在线人数 / 实时在线榜：每 30 秒一次；带 preview 标记的是「发送测试弹幕」推的
  unlistenAudience = await listen<AudienceSnapshot>('audience-update', e => {
    if (e.payload.preview) {
      audiencePreview.value = e.payload
    } else {
      audience.value = e.payload
    }
  })
  unlistenStyle = await listen<OverlayStyle>('overlay-style', e => {
    style.value = e.payload
  })
  unlistenFilter = await listen<DanmakuFilter>('danmaku-filter', e => {
    filter.value = e.payload
  })
  // 初始同步样式
  try {
    style.value = await invoke<OverlayStyle>('overlay_get_style')
  } catch {
    /* ignore */
  }
  // 强制触发字体渲染重计算，避免首次加载时字体像素化
  await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))
  // 初始同步过滤配置
  try {
    filter.value = await invoke<DanmakuFilter>('danmaku_get_filter')
  } catch {
    /* ignore */
  }
  // 初始同步在线人数 / 在线榜（Rust 此前可能已经拉过；还没有就等下一次事件）
  try {
    audience.value = await invoke<AudienceSnapshot | null>('audience_get')
  } catch {
    /* ignore */
  }
  // 初始同步礼物列表配置（条数上限）
  try {
    giftCfg.value = await invoke<GiftConfig>('gift_get_config')
  } catch {
    /* ignore */
  }
  // 轮询兑底：事件丢失时也能同步连接状态（每 2s）
  const poll = async () => {
    try {
      const st = await invoke<RoomStatusEvent>('get_connection_status')
      applyConnStatus(st)
    } catch {
      /* overlay 独立打开等场景忽略 */
    }
  }
  await poll()
  const timer = setInterval(poll, 2000)
  onUnmounted(() => clearInterval(timer))
})

onUnmounted(() => {
  unlistenDanmu?.()
  unlistenRoom?.()
  unlistenStyle?.()
  unlistenFilter?.()
  unlistenGift?.()
  unlistenGiftCfg?.()
  unlistenWelcome?.()
  unlistenWelcomeCfg?.()
  unlistenPreviewReset?.()
  unlistenAudience?.()
})
</script>

<template>
  <div
    class="overlay-root"
    :class="{ 'with-audience': audienceOn }"
    :style="{
      fontSize: style.font_size + 'px',
      fontFamily: style.font_family,
      rowGap: style.row_gap + 'px',
    }"
    data-tauri-drag-region>
    <!-- 常驻透明拖拽条：不在弹幕流内，弹幕高速刷新时也能稳定拖动窗口；移入时显示操作提示 -->
    <div class="drag-strip" data-tauri-drag-region>
      <span class="drag-hint">按住此区域可拖动弹幕窗位置</span>
    </div>
    <!-- 面板：礼物区 + 弹幕区共用同一个圆角矩形与底色。
         圆角与背景放在外层，而不是给两个区各写一半：礼物区没内容时不渲染，
         把顶部圆角写在 .gift-area 上时顶部就会退化成直角。 -->
    <div class="panel" :style="{ backgroundColor: bgColor }">
      <!-- 礼物区：老板打赏，固定在弹幕区上方，与弹幕行共用徽章列（左边缘对齐） -->
      <div v-if="giftCfg.enabled && giftList.length > 0" class="gift-area">
        <GiftRow v-for="b in giftList" :key="b.id" :b="b" :overlay-style="style" />
      </div>
      <!-- 弹幕区：自带裁剪的独立容器，礼物区不会被高频弹幕顶出可视区 -->
      <div class="danmaku-area">
        <DanmakuRow v-for="d in danmakuList" :key="d.id" :d="d" :overlay-style="style" />
      </div>
    </div>

    <!-- 头衔列的边界提示：面板底色左侧那条竖带是留给舰长 / 房管 / 粉丝牌的，
         平时完全透明（常驻线条会被 OBS 一起录进画面），鼠标进入弹幕窗时浮出淡底与虚线。 -->
    <div class="title-band" />
    <!-- 右侧在线列：在看人数 + 实时在线榜（两个开关独立控制；整列显示时面板右侧让出等宽留白）。
         提示带与内容分开：内容必须常显（主角），提示底与虚线只做边界示意、鼠标移入才浮出。 -->
    <template v-if="audienceOn">
      <!-- 右列底色：与弹幕窗同一份背景色（同一个 bg_opacity），右边圆角、左边直角，
           面板在右列显示时也去掉右侧圆角 → 两块拼成一个连续的底，中间只留一条淡分隔线 -->
      <div class="audience-bg" :style="{ backgroundColor: bgColor }" />
      <div class="audience-band" />
      <div v-if="shownAudience" class="audience-slot">
        <AudienceColumn
          :snapshot="shownAudience"
          :overlay-style="style"
          :show-online="style.show_online"
          :show-rank="style.show_rank" />
      </div>
      <!-- 右侧现在是内容列，没有可见的面板边缘可抓，所以手柄放在窗口右缘（系统热区只有 3px） -->
      <div class="resize-handle-east" @mousedown="startResizeEast" />
    </template>
    <!-- 缩放手柄：把「能缩放的位置」挪到看得见的底色边缘上。
         系统给的缩放热区固定在窗口最外缘（实测只有 3px），离这条边有 5.15em 远，
         用户按直觉抓可见边缘时只能拖动窗口、以为什么都抓不到。 -->
    <div class="resize-handle-west" @mousedown="startResizeWest" />
  </div>
</template>

<style scoped>
/* 正文列起点：role-slot 4.8em + 0.7em 间距（与 MetaBadges.vue 的 .role-slot 必须一致）。
   面板背景再往左让 0.35em（--panel-inset），正好落在身份前缀列与第一个徽章中间，
   两边各留 0.35em，荣耀等级 / 粉丝牌不会贴着背景边。
   --panel-inset 同时是发送弹幕框的左缩进（见 window.rs 的 sender_layout_metrics）。 */
.overlay-root {
  --role-indent: 5.5em;
  --panel-inset: calc(var(--role-indent) - 0.35em);
  /* 右侧在线列：宽度与左侧头衔列对称（同 5.5em 起步，取 6.5em 是为了放下「在看 9999+」
     与「舰长牌 + 昵称」，昵称放不下时按弹幕那样折行）。--audience-inset 是面板底色实际
     让出的宽度，比整列窄 0.35em（与 --panel-inset 同一套算法）。
     两个开关都关时 .with-audience 不生效，inset 为 0，面板恢复满宽。 */
  --audience-indent: 6.5em;
  --audience-inset: 0px;
  position: relative;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  padding: 6px;
  box-sizing: border-box;
  overflow: hidden;
  background: transparent;
  user-select: none;
}

/* 在线列显示时才让出右留白（与 window.rs 的 AUDIENCE_INDENT_EM 必须一致） */
.overlay-root.with-audience {
  --audience-inset: calc(var(--audience-indent) - 0.35em);
}

/* 右列有自己的底色，面板就不该再有右侧圆角：两块拼起来才是一个连续圆角矩形 */
.overlay-root.with-audience .panel {
  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
}

/* 面板：礼物区 + 弹幕区共用的圆角矩形（底色由「背景不透明度」控制）。
   gap: inherit 让内部两区继续用弹幕窗根节点下发的 rowGap。

   背景左边缘要让出身份前缀列（房管 / 舰长徽章），但行内容必须留在原位，
   所以面板自身 margin-left 右移、内部两区再用等量负 margin 拉回来：
   只有背景走了，文字坐标不变。缩进取 --panel-inset（比 --role-indent 少 0.35em），
   背景与第一个徽章之间才有一段空隙。
   overflow 必须是 visible：徽章被拉到面板左侧之外后，若在这里裁剪就没了。 */
.panel {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: inherit;
  margin-left: var(--panel-inset);
  /* 右侧让出在线列的位置：背景右边缘左移，内部两区不做反向拉回，
     于是弹幕文字的右边界就落在面板底色内侧，右侧那条竖带留给在线列 */
  margin-right: var(--audience-inset);
  border-radius: 15px;
  overflow: visible;
}

/* 礼物区：行间距跟随弹幕窗的 rowGap。
   flex-shrink: 0 = 不允许被弹幕挤掉：礼物是「值得看一眼」的信息，得一直在。 */
.gift-area {
  display: flex;
  flex-direction: column;
  gap: inherit;
  flex-shrink: 0;
  padding: 3px 0;
  margin-bottom: 3px;
  /* 拉回面板右移的那段，让行内容回到窗口原位（与面板 margin-left 等量反向） */
  margin-left: calc(-1 * var(--panel-inset));
}

/* 弹幕区：自带裁剪的滚动容器。
   min-height: 0 是关键——flex 子项默认 min-height: auto，会被内容撑到全高，
   结果连同上方礼物区一起溢出容器顶部被裁掉（就是「礼物看不见」的原因）。
   justify-content: flex-end 让新弹幕贴底、旧的从顶部裁掉，与改动前观感一致；
   padding-bottom 是给最新一条留的呼吸，不然文字直接顶着面板下边缘。 */
.danmaku-area {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: inherit;
  overflow: hidden;
  padding-bottom: 0.4em;
  /* 同 .gift-area：行回到窗口原位，只有背景面板右移 */
  margin-left: calc(-1 * var(--panel-inset));
}

/* 右列显示时给正文留一点右边距：否则文字会贴着两块底色之间的分隔线 */
.overlay-root.with-audience .danmaku-area,
.overlay-root.with-audience .gift-area {
  padding-right: 0.35em;
}

/* 顶部常驻拖拽条：占位独立行（不压内容），平时透明；鼠标移入悬浮窗即浮出提示 */
.drag-strip {
  /* 不参与压缩，否则弹幕一多就被挤成一条线 */
  flex: 0 0 auto;
  /* 左端对齐面板背景左边缘（也避开了房管/舰长徽章）；右端同样让出在线列 */
  margin-left: var(--panel-inset);
  margin-right: var(--audience-inset);
  height: 1.4em;
  cursor: move;
  background: transparent;
  border-bottom: 1px solid transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  transition:
    background 0.12s ease,
    border-color 0.12s ease;
}

/* 操作提示：默认淡出，鼠标移入悬浮窗时显示；pointer-events 穿透保证整条可拖动 */
.drag-hint {
  font-size: 0.8em;
  line-height: 1;
  color: rgba(255, 255, 255, 0.92);
  opacity: 0;
  pointer-events: none;
  white-space: nowrap;
  transition: opacity 0.12s ease;
}

.overlay-root:hover .drag-strip {
  background: rgba(0, 0, 0, 0.55);
  border-bottom-color: rgba(255, 255, 255, 0.35);
}

.overlay-root:hover .drag-hint {
  opacity: 1;
}

/* 头衔列（窗口左缘到面板底色左缘这条竖带）：鼠标进入弹幕窗才浮出，
   平时完全透明——直播时鼠标扫过弹幕窗不会留下常驻线条。
   提示写法与 .drag-strip 同一套机制，但**只过渡 opacity**：淡底与虚线写成常值，
   靠 opacity 显隐，动画就只走合成层（与 .row-enter 同一条规矩，见 Agents.md 的性能约定）。 */
.title-band {
  position: absolute;
  left: 6px;
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: var(--panel-inset);
  background: rgba(255, 255, 255, 0.05);
  /* 右侧那条虚线就是面板底色的左边界 */
  border-right: 1px dashed rgba(255, 255, 255, 0.35);
  opacity: 0;
  /* 不接管任何鼠标行为：这一段仍然由根节点负责拖动 */
  pointer-events: none;
  transition: opacity 0.12s ease;
}

.overlay-root:hover .title-band {
  opacity: 1;
}

/* 缩放手柄：中心对齐面板底色的可见左边缘，抓取宽度放宽到 9px
   （系统热区在窗口最外缘、只有 3px，离这条边 5.15em，按直觉抓不到）。
   位置用同一个 --panel-inset 推导，字号变大时仍贴在边界上。 */
.resize-handle-west {
  position: absolute;
  /* -4.5px + 9px 宽 = 手柄中心正好落在面板底色左边缘上（6px + --panel-inset） */
  left: calc(6px + var(--panel-inset) - 4.5px);
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: 9px;
  cursor: ew-resize;
}

/* 右侧在线列的边界提示带：与左侧 .title-band 同一套做法（鼠标移入才浮出）。
   底色已经由 .audience-bg 常显，这里只负责那条虚线边界 + 移入时轻微提亮。
   宽度 = 整列宽 − 0.35em：起点正好落在面板底色右边缘（与 .panel 的 margin-right 互补）。 */
.audience-band {
  position: absolute;
  right: 6px;
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: calc(var(--audience-indent) - 0.35em);
  background: transparent;
  border-left: 1px dashed rgba(255, 255, 255, 0.35);
  opacity: 0;
  /* 不接管鼠标：这一段仍由根节点负责拖动 */
  pointer-events: none;
  transition: opacity 0.12s ease;
}

.overlay-root:hover .audience-band {
  opacity: 1;
}

/* 右列底色：与弹幕窗同一份 bgColor（由模板内联下发），右边圆角、左边直角。
   面板在右列显示时去掉右侧圆角（见 .overlay-root.with-audience .panel），
   两块因此拼成一个连续的底，中间只有一条淡分隔线，看起来仍是"一个弹幕窗"。 */
.audience-bg {
  position: absolute;
  right: 6px;
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: calc(var(--audience-indent) - 0.35em);
  border-radius: 0 15px 15px 0;
  /* 与面板底色之间的分隔线：1px 淡白，不抢视线 */
  border-left: 1px solid rgba(255, 255, 255, 0.12);
  pointer-events: none;
}

/* 在线列内容：从面板底色右边缘起、留 0.35em 空隙，右端到窗口 padding 为止。
   起点必须与面板底色右边缘对齐——这正是右列的「列」含义（左侧头衔列同理由）。 */
.audience-slot {
  position: absolute;
  right: 6px;
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: calc(var(--audience-indent) - 0.35em);
  padding-left: 0.35em;
  box-sizing: border-box;
  overflow: hidden;
  /* 内容压在底色之上（DOM 顺序已保证，这里只为语义明确） */
  z-index: 1;
}

/* 右侧缩放热区：放在窗口右缘（那里才是系统认可的缩放位置），只把 3px 放宽到 9px。
   在线列出现后没有可见的面板边缘可抓，所以不做成"贴着虚线"的手柄——
   那会压在「在看 123」上，点文字就变成缩放。 */
.resize-handle-east {
  position: absolute;
  right: 0;
  top: calc(6px + 1.4em);
  bottom: 6px;
  width: 9px;
  cursor: ew-resize;
}
</style>
