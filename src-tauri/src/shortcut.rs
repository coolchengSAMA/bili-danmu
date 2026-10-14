//! 发送框的全局快捷键：字符串 ↔ Shortcut、注册与注销
//!
//! 为什么必须系统级注册：前端 keydown 和菜单快捷键只在应用有焦点时有效，
//! 游戏占着前台时收不到，只有 `RegisterHotKey` 这条路（tauri-plugin-global-shortcut）能响应。
//!
//! 快捷键串格式：`修饰键 + 一个键码`，如 `Alt+KeyQ` / `Ctrl+Shift+Digit1` / `F8`；
//! 键码用 UI Events 的 code 名，`Code::from_str` 直接认。空串 = 不注册。
//! 注册可能失败（被其它程序占用），错误原样向上抛，由设置页提示用户换一个。

use std::str::FromStr;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

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

/// 注销旧快捷键并注册新的（空串 = 只注销）
pub(crate) fn apply_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    gs.unregister_all()
        .map_err(|e| format!("注销旧快捷键失败：{e}"))?;
    if hotkey.trim().is_empty() {
        return Ok(());
    }
    let sc = parse_hotkey(hotkey)?;
    gs.on_shortcut(sc, |app, _sc, event| {
        // 只认按下：否则按下与松开各触发一次，等于没切
        if event.state == ShortcutState::Pressed {
            crate::commands::toggle_sender(app);
        }
    })
    .map_err(|e| format!("注册失败（可能已被其它程序占用）：{e}"))
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
