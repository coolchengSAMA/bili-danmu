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
        // 修饰键名大小写不敏感（config.json 可能被手改），键码保持原样交给 Code::from_str
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            "meta" | "super" | "win" => mods |= Modifiers::SUPER,
            _ => {
                if code.is_some() {
                    return Err(format!("只能有一个主键：{s}"));
                }
                code = Some(Code::from_str(p).map_err(|_| format!("无法识别的按键：{p}"))?);
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
    // 比解析后的 Shortcut 而不是字符串：`Ctrl+KeyQ` 与 `Control+KeyQ`、大小写差异
    // 都是同一个键位，按字符串比会走到 register 撞 AlreadyRegistered 并误报「已被占用」
    let old_sc = parse_hotkey(old).ok().filter(|_| !old.is_empty());
    let new_sc = parse_hotkey(new).ok().filter(|_| !new.is_empty());
    if new.is_empty() {
        // 只注销
        if let Some(sc) = old_sc {
            if let Err(e) = app.global_shortcut().unregister(sc) {
                log::warn!("[sender] 注销快捷键 {old} 失败: {e}");
            }
        }
        return Ok(());
    }
    let new_sc = new_sc.ok_or_else(|| format!("无法识别的快捷键：{new}"))?;
    if old_sc == Some(new_sc) {
        return Ok(());
    }
    let gs = app.global_shortcut();
    // 事件处理在 lib.rs 注册插件时用 Builder::with_handler 挂上，本函数只负责生命周期
    gs.register(new_sc)
        .map_err(|e| format!("注册失败（可能已被其它程序占用）：{e}"))?;
    if let Some(sc) = old_sc {
        // 旧键位注销失败不影响新键位已生效
        if let Err(e) = gs.unregister(sc) {
            log::warn!("[sender] 注销旧快捷键 {old} 失败: {e}");
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

    /// 与 `SenderWindowPanel.vue` 的 `comboOf()` 约定对齐：修饰键顺序 Ctrl/Alt/Shift/Meta + UI Events 键码。
    /// 两边各自手写这份约定，所以用字面量把契约钉住——改一侧的写法就会在这里红。
    #[test]
    fn 前端产出的组合串都能解析() {
        for s in [
            "Ctrl+KeyQ",
            "Alt+KeyQ",
            "Shift+Digit1",
            "Meta+KeyQ",
            "Ctrl+Shift+Digit1",
            "F1",
            "F24",
        ] {
            assert!(parse_hotkey(s).is_ok(), "{s} 应能解析");
        }
    }

    /// 同一键位的不同写法必须等价：`apply_hotkey` 靠这条避免把「同一键位」当成新键位去
    /// register，从而误报「可能已被其它程序占用」。修饰键名大小写不敏感；键码仍按
    /// UI Events 的 code 名精确匹配（`keyq` 不是合法键码，会被拒绝）
    #[test]
    fn 同一键位的不同写法等价() {
        assert_eq!(
            parse_hotkey("Control+KeyQ").unwrap(),
            parse_hotkey("Ctrl+KeyQ").unwrap()
        );
        assert_eq!(
            parse_hotkey("alt+KeyQ").unwrap(),
            parse_hotkey("Alt+KeyQ").unwrap()
        );
        assert!(parse_hotkey("Alt+keyq").is_err());
    }
}
