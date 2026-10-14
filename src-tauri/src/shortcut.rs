//! 发送框的全局快捷键：字符串 ↔ Shortcut、注册与注销
//!
//! 为什么必须系统级注册：前端 keydown 和菜单快捷键只在应用有焦点时有效，
//! 游戏占着前台时收不到，只有 `RegisterHotKey` 这条路（tauri-plugin-global-shortcut）能响应。
//! 代价是注册成功后该组合键被**系统级占用**——其它程序（包括游戏）都收不到它，
//! 所以默认不注册，由用户在设置页自己录一个。
//!
//! 快捷键串格式：`修饰键 + 一个键码`，如 `Alt+KeyQ` / `Ctrl+Shift+Digit1` / `F8`；
//! 键码用 UI Events 的 code 名，`Code::from_str` 直接认。空串 = 不注册。
//! 注册可能失败（被其它程序占用），错误原样向上抛，由设置页提示用户换一个。

use std::str::FromStr;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

/// 解析快捷键串；失败返回可直接展示给用户的中文原因
pub(crate) fn parse_hotkey(s: &str) -> Result<Shortcut, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("快捷键为空".into());
    }
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in s.split('+') {
        let p = part.trim();
        match p {
            "Ctrl" | "Control" => mods |= Modifiers::CONTROL,
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            "Meta" | "Super" | "Win" => mods |= Modifiers::SUPER,
            other => {
                if code.is_some() {
                    return Err(format!("只能有一个主键：{s}"));
                }
                code = Some(
                    Code::from_str(other).map_err(|_| format!("无法识别的按键：{other}"))?,
                );
            }
        }
    }
    let code = code.ok_or_else(|| format!("缺少主键：{s}"))?;
    Ok(Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, code))
}

/// 把注册从 `old` 换成 `new`（空串表示只注销）
///
/// 顺序是「先注册新的、成功后再注销旧的」，失败时保留原来能用的那个：早先写法是
/// `unregister_all()` 打头，注册失败就等于把用户已有的可用键位一并丢掉；而且
/// `unregister_all` 会把将来新增的其它快捷键（roadmap 里的 TTS / 穿透）也一起清掉，
/// 所以这里只动这两个具体的 Shortcut。事件处理在 lib.rs 注册插件时用
/// `Builder::with_handler` 挂上，本函数只负责生命周期。
pub(crate) fn apply_hotkey(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    let old = old.trim();
    let new = new.trim();
    if old == new {
        return Ok(());
    }
    let gs = app.global_shortcut();
    if !new.is_empty() {
        let sc = parse_hotkey(new)?;
        gs.register(sc)
            .map_err(|e| format!("注册失败（可能已被其它程序占用）：{e}"))?;
    }
    if !old.is_empty() {
        if let Ok(sc) = parse_hotkey(old) {
            // 旧键位注销失败不影响新键位已生效
            if let Err(e) = gs.unregister(sc) {
                log::warn!("[sender] 注销旧快捷键 {old} 失败: {e}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析保留修饰键与主键() {
        let sc = parse_hotkey("Alt+KeyQ").unwrap();
        assert_eq!(sc.mods, Modifiers::ALT);
        assert_eq!(sc.key, Code::KeyQ);
    }

    #[test]
    fn 无修饰键的功能键也可以() {
        assert!(parse_hotkey("F8").is_ok());
    }

    #[test]
    fn 空串缺主键多主键与非法键名都被拒绝() {
        assert!(parse_hotkey("").is_err());
        assert!(parse_hotkey("Alt+").is_err());
        assert!(parse_hotkey("Alt+KeyQ+KeyW").is_err());
        assert!(parse_hotkey("NotAKey").is_err());
    }
}
