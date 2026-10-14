// 「发送测试弹幕」：把样例弹幕 / 礼物 / 欢迎行通过**真实事件通道**推给弹幕窗。
//
// 三条设计约束（改这个文件前先读）：
//  1. **必须走真实事件通道**（与 Rust 下发用的是同一组 `danmaku` / `gift` / `welcome` 事件、
//     同一套组件与 CSS）：预览的意义就是「所见即直播所见」。另写一套预览渲染必然与真实渲染漂移
//     （徽章列宽 `role-slot`、换行续行对齐、描边都靠真实组件与共用 CSS），那预览就会骗人。
//  2. **不经过 B 站接口**：只 emit 本地事件，不会发到直播间；但行是真画在弹幕窗上的，
//     推流时会被 OBS 采集（按钮文案里要写明）。
//  3. **预览行带 `preview: true`**：① 绕过弹幕显示过滤——否则房间设了「只显示舰长」时，
//     普通观众那条会被整条丢掉，用户以为按钮坏了；② 每次点击先发 `preview-reset`
//     清掉上一组预览行，**只清带标记的行，真弹幕一条不动**（id 也带 `preview-` 前缀，不会撞）。
import { emit } from '@tauri-apps/api/event'

import type { BackingEvent, DanmakuEvent, WelcomeEvent } from './types/ipc'

/** 逐条推送的间隔（ms）：与真实弹幕一样逐条滚入，入场动画才会逐行播 */
const STEP_MS = 200

const now = () => Math.floor(Date.now() / 1000)

/** 一组覆盖全部徽章 / 样式分支的预览弹幕 */
export function buildPreviewDanmaku(roomId: number): DanmakuEvent[] {
  const t = now()
  let n = 0
  const mk = (patch: Partial<DanmakuEvent>): DanmakuEvent => ({
    id: `preview-${Date.now()}-${++n}`,
    username: '测试观众',
    content: '测试弹幕',
    timestamp: t,
    is_admin: false,
    preview: true,
    ...patch,
  })
  return [
    mk({ username: '测试观众', content: '普通观众：没有任何徽章' }),
    mk({ username: '测试房管', content: '房管徽章', is_admin: true }),
    mk({ username: '测试舰长', content: '舰长徽章', guard_level: 3 }),
    mk({ username: '测试提督', content: '提督徽章', guard_level: 2 }),
    mk({ username: '测试总督', content: '总督徽章', guard_level: 1 }),
    mk({ username: '测试高等级', content: '荣耀等级徽章', wealth_level: 42 }),
    mk({
      username: '测试本房牌',
      content: '本房粉丝牌（绿色）',
      medal_name: '测试牌',
      medal_level: 18,
      medal_room_id: roomId,
      is_room_medal: true,
    }),
    mk({
      username: '测试外房牌',
      content: '其它直播间粉丝牌（灰色）',
      medal_name: '别家牌',
      medal_level: 7,
      medal_room_id: roomId + 1,
      is_room_medal: false,
    }),
    mk({ username: '测试彩色', content: '彩色弹幕（正文颜色）', color: '#FF6B6B' }),
    mk({
      username: '测试长文本',
      content:
        '这是一条很长的测试弹幕，用来检查自动换行之后续行是否与正文列对齐、而不是顶到窗口最左边，以及描边在换行处的表现。',
    }),
  ]
}

/** 一组预览礼物行：普通礼物 → **同一个 id 的连击更新**（演示「按 id 覆盖、行不新增」）→ 醒目留言 → 上舰 */
export function buildPreviewGift(): BackingEvent[] {
  const t = now()
  const base = `preview-gift-${Date.now()}`
  // 第 1、2 条故意用同一个 id：第二条到达时 overlay 会覆盖第一行（数量 5 → 10），不新增行
  const comboId = `${base}-combo`
  return [
    {
      id: comboId,
      kind: 'gift',
      uid: 1,
      username: '测试观众',
      gift_name: '辣条',
      gift_id: 1,
      num: 5,
      amount_fen: 500,
      timestamp: t,
      guard_level: 0,
      wealth_level: 12,
      preview: true,
    },
    {
      id: comboId,
      kind: 'combo',
      uid: 1,
      username: '测试观众',
      gift_name: '辣条',
      gift_id: 1,
      num: 10,
      amount_fen: 1000,
      timestamp: t,
      guard_level: 0,
      wealth_level: 12,
      preview: true,
    },
    {
      id: `${base}-sc`,
      kind: 'super_chat',
      uid: 2,
      username: '测试醒目留言',
      gift_name: '醒目留言',
      gift_id: 0,
      num: 1,
      amount_fen: 3000,
      timestamp: t,
      message: '这是一条测试醒目留言的正文',
      preview: true,
    },
    {
      id: `${base}-guard`,
      kind: 'guard',
      uid: 3,
      username: '测试舰长',
      gift_name: '舰长',
      gift_id: 0,
      num: 1,
      amount_fen: 13800,
      timestamp: t,
      guard_level: 3,
      wealth_level: 20,
      preview: true,
    },
  ]
}

/** 一组预览欢迎行（进房 / 关注 / 舰长进场）：与真实欢迎信息共用同一形态 */
export function buildPreviewWelcome(): WelcomeEvent[] {
  const t = now()
  const base = `preview-welcome-${Date.now()}`
  return [
    { id: `${base}-1`, kind: 'enter', uid: 11, username: '测试观众', timestamp: t, preview: true },
    { id: `${base}-2`, kind: 'follow', uid: 12, username: '测试关注', timestamp: t, preview: true },
    {
      id: `${base}-3`,
      kind: 'guard_enter',
      uid: 13,
      username: '测试舰长',
      guard_level: 3,
      timestamp: t,
      preview: true,
    },
  ]
}

/**
 * 推一组预览：先 reset 清掉上一组预览行，再逐条发样例。
 *
 * 返回本次推送的行数（供界面提示）。`includeGift` / `includeWelcome` 由调用方按
 * 礼物区、欢迎信息的开关决定——预览不绕过样式开关，只在界面说明哪部分被跳过。
 */
export async function emitDanmakuPreview(opt: {
  roomId: number
  includeGift: boolean
  includeWelcome: boolean
}): Promise<number> {
  await emit('preview-reset')
  const rows: { name: string; payload: unknown }[] = buildPreviewDanmaku(opt.roomId).map(payload => ({
    name: 'danmaku',
    payload,
  }))
  if (opt.includeGift) {
    rows.push(...buildPreviewGift().map(payload => ({ name: 'gift', payload })))
  }
  if (opt.includeWelcome) {
    rows.push(...buildPreviewWelcome().map(payload => ({ name: 'welcome', payload })))
  }
  for (const row of rows) {
    await emit(row.name, row.payload)
    await new Promise(r => setTimeout(r, STEP_MS))
  }
  return rows.length
}
