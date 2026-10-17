import { ref } from 'vue'

/**
 * 界面配色模式：跟随系统 / 强制浅色 / 强制深色
 *
 * 只影响**主窗口**（`style.css` 由 main.ts 引入；overlay / sender 用各自的 CSS，
 * 它们的颜色是固定的，不跟着主题变——弹幕窗要压在游戏画面上，发送框要吸附在它下面）。
 */
export type ThemeMode = 'system' | 'light' | 'dark'

/** localStorage 键名：必须与 `public/theme-boot.js` 一致（那个脚本在首帧前读同一个键） */
export const THEME_KEY = 'bili-danmu:theme'

const media = window.matchMedia('(prefers-color-scheme: dark)')

function readMode(): ThemeMode {
  try {
    const v = localStorage.getItem(THEME_KEY)
    return v === 'light' || v === 'dark' ? v : 'system'
  } catch {
    return 'system'
  }
}

/**
 * 把模式解析成实际配色并写到 `<html data-theme>`（CSS 里 `:root[data-theme='dark']` 覆盖变量）。
 *
 * 不在 CSS 里用 `@media (prefers-color-scheme: dark)`：那样只能在 Windows 设置里切，
 * 应用内没法切。现在「跟随系统」由这里解析，系统换深浅色时通过下面的监听跟上。
 */
function apply(next: ThemeMode) {
  const dark = next === 'dark' || (next === 'system' && media.matches)
  document.documentElement.dataset.theme = dark ? 'dark' : 'light'
}

const mode = ref<ThemeMode>(readMode())
// 启动时补一次：正常情况下 public/theme-boot.js 已经在首帧前写过同一属性，这里只是对齐内存态
apply(mode.value)

media.addEventListener('change', () => {
  if (mode.value === 'system') apply('system')
})

export function useTheme() {
  function setTheme(next: ThemeMode) {
    mode.value = next
    try {
      localStorage.setItem(THEME_KEY, next)
    } catch {
      // 存不进去不影响本次切换，只是下次启动回到跟随系统
    }
    apply(next)
  }
  return { mode, setTheme }
}
