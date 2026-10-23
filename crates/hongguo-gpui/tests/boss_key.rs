//! 老板键组合的纯逻辑断言（对照 Tauri `boss-key-card.test.ts` + 注册参数）。
//!
//! accel.rs 按源文件原样引入——壳是纯 bin（无 lib 目标，集成测试进不了
//! crate 内部；bin 又 `test = false`，模块内 `#[cfg(test)]` 永远不跑），
//! 源文件级引入是唯一通路，accel.rs 因此保持自洽不引 `super::*`。
//!
//! 关键不变量：**捕获产出的串必须能反解成注册参数**——否则就是「设置里存
//! 下了、注册时才发现不认识」。
//!
//! 注册参数（`parse` / `MOD_*`）两端都编译（macOS 用它在捕获阶段校验组合，
//! 注册本身由平台层报「未实现」），所以这些断言在全部目标上跑。

#[path = "../src/i18n.rs"]
mod i18n;

#[path = "../src/boss_key/accel.rs"]
mod accel;

#[test]
fn i18n_is_available_to_accel_tests() {
    i18n::set_locale(i18n::Locale::ZhCn);
    assert_eq!(i18n::Locale::ALL.len(), 2);
    assert_eq!(i18n::Locale::ZhCn.tag(), "zh-CN");
    assert_eq!(i18n::Locale::EnUs.label_key(), "search.langEn");
    assert_eq!(
        i18n::app_error_text(&hongguo_common::AppError::Cancelled),
        "操作已取消"
    );
}

#[test]
fn core_key_is_last_segment_lowercased() {
    assert_eq!(accel::core_key("CommandOrControl+Alt+H"), "h");
    assert_eq!(accel::core_key("Alt+Z"), "z");
    assert_eq!(accel::core_key(""), "");
    assert_eq!(accel::core_key("   "), "");
}

#[test]
fn display_maps_command_or_control() {
    assert_eq!(
        accel::display("CommandOrControl+Alt+H"),
        "Ctrl/Cmd + Alt + H"
    );
    assert_eq!(accel::display("Alt+Up"), "Alt + Up");
    assert_eq!(accel::display(""), "");
}

#[test]
fn parse_needs_ctrl_or_alt() {
    assert!(accel::parse("Shift+H").is_err(), "只有 Shift 不算全局组合");
    assert!(accel::parse("H").is_err());
    assert!(accel::parse("").is_err());
    let hk = accel::parse("CommandOrControl+Alt+H").expect("标准组合");
    assert_eq!(hk.vk, 'H' as u32);
    assert_eq!(hk.mods & accel::MOD_CONTROL, accel::MOD_CONTROL);
    assert_eq!(hk.mods & accel::MOD_ALT, accel::MOD_ALT);
    // 按住不连发（切换语义）
    assert_eq!(hk.mods & 0x4000, 0x4000);
}

#[test]
fn parse_maps_named_keys() {
    assert_eq!(accel::parse("Alt+Space").expect("空格").vk, 0x20);
    assert_eq!(accel::parse("Alt+Up").expect("↑").vk, 0x26);
    assert_eq!(accel::parse("Alt+F5").expect("F5").vk, 0x74);
    assert_eq!(accel::parse("Alt+Num3").expect("小键盘").vk, 0x63);
    assert_eq!(accel::parse("Alt+1").expect("数字").vk, 0x31);
    // 标点正反两态归到同一物理键
    assert_eq!(
        accel::parse("Alt+-").expect("减号").vk,
        accel::parse("Alt+_").expect("下划线").vk
    );
}

#[test]
fn capture_produces_registrable_accelerators() {
    let cases = [
        ("h", true, true, false, "CommandOrControl+Alt+H"),
        ("space", false, true, false, "Alt+Space"),
        ("left", true, false, true, "CommandOrControl+Shift+Left"),
        ("f5", true, false, false, "CommandOrControl+F5"),
        ("5", false, true, true, "Alt+Shift+5"),
    ];
    for (key, ctrl, alt, shift, want) in cases {
        let spec = accel::accelerator_from_keystroke(key, ctrl, alt, shift)
            .unwrap_or_else(|| panic!("{key} 应可捕获"));
        assert_eq!(spec, want);
        assert!(accel::parse(&spec).is_ok(), "{spec} 必须能反解成注册参数");
    }
    // 缺 Ctrl/Cmd 与 Alt：不成立（Tauri acceleratorFromEvent 同口径）
    assert_eq!(
        accel::accelerator_from_keystroke("h", false, false, true),
        None
    );
}

#[test]
fn gpui_key_of_feeds_conflict_check() {
    assert_eq!(accel::gpui_key_of("CommandOrControl+Alt+H"), "h");
    assert_eq!(accel::gpui_key_of("Alt+Space"), "space");
    assert_eq!(accel::gpui_key_of("Alt+Up"), "up");
    assert_eq!(accel::gpui_key_of(""), "");
}

// Carbon 换算只在 macOS 目标编译（Windows 上没有调用点），覆盖由 macOS job 承担。
#[cfg(target_os = "macos")]
#[test]
fn mac_carbon_params_cover_captured_combinations() {
    // 捕获产出的串 → Win32 注册参数 → Carbon（修饰位 + ANSI 键码）
    let hk = accel::parse("CommandOrControl+Alt+H").expect("H 组合可解析");
    assert_eq!(accel::mac_keycode(hk.vk), Some(0x04)); // kVK_ANSI_H
    assert_eq!(
        accel::carbon_mods(hk.mods),
        accel::CARBON_CMD | accel::CARBON_OPTION
    );

    // CommandOrControl 在 macOS 落 Command；Shift 单列
    let hk = accel::parse("CommandOrControl+Shift+Space").expect("空格组合可解析");
    assert_eq!(accel::mac_keycode(hk.vk), Some(0x31)); // kVK_Space
    assert_eq!(
        accel::carbon_mods(hk.mods),
        accel::CARBON_CMD | accel::CARBON_SHIFT
    );

    // 键码表覆盖设置页能捕获的键（字母/数字/功能键/导航/标点）
    for spec in [
        "CommandOrControl+A",
        "CommandOrControl+9",
        "CommandOrControl+F12",
        "CommandOrControl+Left",
        "CommandOrControl+PageDown",
        "CommandOrControl+Escape",
        "CommandOrControl+;",
        "CommandOrControl+Num7",
    ] {
        let hk = accel::parse(spec).expect("应可解析");
        assert!(accel::mac_keycode(hk.vk).is_some(), "{spec} 缺 macOS 键码");
    }

    // 没有对应物理键的键返回 None（注册时报失败，不会静默无效）
    assert_eq!(accel::mac_keycode(0x2D), None); // Insert
    assert_eq!(accel::mac_keycode(0x87), None); // F24
}
