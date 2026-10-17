/* 首帧前应用界面配色，避免「系统是浅色、应用内选了深色」时先闪一下浅色再变。
 *
 * 为什么存在这个文件（而不是在 Vue 里做）：CSS 只认 <html data-theme>，
 * 而 data-theme 必须在本文件之后、Vue 挂载之前写进去，否则第一帧就是错的。
 *
 * 为什么用 localStorage 而不是 config.json：这个脚本要在任何 IPC / Vue 之前**同步**执行，
 * 而 Rust 的配置只能异步读。主题是纯前端外观偏好，丢了也只是回到「跟随系统」，不影响功能。
 *
 * CSP 是 `script-src 'self'`，内联脚本会被拦，所以放 public/ 走同源外链（Vite 原样拷到 dist 根）。
 * 键名必须与 `src/composables/useTheme.ts` 的 THEME_KEY 一致。 */
;(function () {
  var dark = false
  try {
    var mode = localStorage.getItem('bili-danmu:theme')
    var systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches
    // 只有明确选了 light 才是浅色；dark / system / 没有值（null）都按深色条件判断
    dark = mode === 'dark' || (mode !== 'light' && systemDark)
  } catch {
    // localStorage 被禁（隐私模式等）：交给系统偏好，取不到就浅色
    try {
      dark = window.matchMedia('(prefers-color-scheme: dark)').matches
    } catch {
      dark = false
    }
  }
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light')
})()
