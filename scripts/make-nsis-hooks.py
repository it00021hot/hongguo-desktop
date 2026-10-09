#!/usr/bin/env python3
"""生成 NSIS 安装器钩子（Windows 侧的应用显示名本地化）。

## 为什么 Windows 要单独做一套

macOS 有 `.lproj/InfoPlist.strings` 这种**运行时**本地化机制（Dock/Finder 按
系统语言取显示名，包名不变）。Windows 没有对应机制：快捷方式显示名就是
`.lnk` 文件名、「应用和功能」里的名字是安装时写进注册表的字符串——都只能在
安装那一刻定死。所以这里按系统 UI 语言挑名字，一次改三处：

1. 开始菜单 / 桌面快捷方式名（`installer.nsi` 用 `$AppShortcutName` 创建）
2. 「应用和功能」的 `DisplayName`（POSTINSTALL 钩子写注册表）
3. 安装器自身标题 `$(^Name)`（下面的 `LangString ^Name`）

## 为什么用 GetUserDefaultUILanguage 而不是 $LANGUAGE

`$LANGUAGE` 是安装器界面用的语言：系统语言不在 `languages` 列表里时它回落成
**列表第一项**（本项目是 SimpChinese），于是法语/日语用户会把应用名拿成中文。
`GetUserDefaultUILanguage` 直接给系统 UI 语言 ID，我们只认中文的几种，其余一律
回落英文 `productName`——与 macOS 侧「没有 zh 就显示 Hongguo」的行为一致。

## 编码要求（重要）

NSIS 在 Windows 上默认按 ANSI 读脚本，含中文的 `.nsh` **必须** UTF-8 + BOM，
否则 makensis 会把中文字面量读成乱码。Tauri 自己落盘语言文件也是
`write_utf8_with_bom`（tauri-bundler 的 nsis/mod.rs）。所以这份文件由脚本生成
而非手写——编辑器很容易把 BOM 丢掉，而丢了 BOM 的表现是快捷方式名变乱码，
很难联想到编码。

## 与 installer.nsi 的分工

- 本文件：名字表 + 钩子宏（产品相关，改名只动这里）
- `src-tauri/nsis/installer.nsi`：官方模板 + 本地化补丁，由
  `scripts/sync-nsis-template.py` 生成（把写死的 `${PRODUCTNAME}.lnk` 换成
  `$AppShortcutName.lnk`，并在 un.onInit 里读回安装时记录的名字）

两者由 `tauri.conf.json` 的 `bundle.windows.nsis.template` /
`installerHooks` 接进构建。
"""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "nsis" / "installer-hooks.nsh"

# 安装器的语言列表（tauri.conf.json > bundle.windows.nsis.languages）。
# LangString 只能为**已插入**的语言定义：给列表外的语言定义 LangString 会牵出
# 一连串「LangString ... is not set in language table」告警。
INSTALLER_LANG_IDS = {
    "simpchinese": 2052,
    "tradchinese": 1028,
}

# 系统 UI 语言 → 应用显示名。左侧是 Windows 的 LANGID：
#   2052 zh-CN / 4100 zh-SG   简体
#   1028 zh-TW / 3076 zh-HK / 5124 zh-MO   繁体
# 未列出的语言（含英文）一律回落 productName。
NAME_BY_UI_LANG = {
    "2052": "红果播放器",
    "4100": "红果播放器",
    "1028": "紅果播放器",
    "3076": "紅果播放器",
    "5124": "紅果播放器",
}

# 安装器标题：只对语言列表里的两个中文语言定义（英文沿用 Name 指令的
# productName，即 Hongguo）。zh-HK/zh-MO 在列表外、会回落列表第一项，
# 所以这里不给它们定义（定义了对那些系统也不会被选中）。
TITLE_BY_LANG = {
    "simpchinese": "红果播放器",
    "tradchinese": "紅果播放器",
}

# 占位符替换而不是 str.format：正文里全是 NSIS 的 ${...} 花括号，用 format
# 得把每一处都写成 {{...}}，极易漏（漏一处就是编译期 KeyError 或错字面量）。
BODY = """\
; 红果播放器 —— NSIS 安装器钩子（由 scripts/make-nsis-hooks.py 生成，勿手改）
;
; Windows 侧应用显示名本地化：快捷方式名 /「应用和功能」DisplayName /
; 安装器标题。macOS 侧对应机制见 scripts/make-macos-lproj.py。
;
; 依赖 installer.nsi（scripts/sync-nsis-template.py 生成）里的：
;   Var AppShortcutName     —— 本次安装该用的显示名（模板里所有 .lnk 都用它）
;   un.onInit 从注册表读回安装时记录的名字
; 把本文件单独 !include 到官方原版模板上会因为缺 $AppShortcutName 而报错。

; 本文件自用变量（$AppShortcutName 由 installer.nsi 声明）
Var HG_OldName

@@TITLE_LANGSTRINGS@@

; 系统 UI 语言 → 应用显示名。安装器界面语言（$LANGUAGE）只在用户**手选过**时
; 才作数，所以拆成两步：先按系统语言定，用户选过再按选的那门定。不能只看
; $LANGUAGE——系统语言不在 languages 列表里时它会回落成列表第一项（English），
; 于是中文 Windows 上装出来是英文名；也不能只看系统语言——那样手选语言后又
; 会出现「界面中文、快捷方式叫英文」的错位。两步合起来两种情况都对。
!macro HG_NAME_FOR_LANG_ID ID
  StrCpy $AppShortcutName "${PRODUCTNAME}"
@@LANG_BRANCHES@@
!macroend

!macro HG_RESOLVE_SHORTCUT_NAME
  StrCpy $AppShortcutName "${PRODUCTNAME}"
  ClearErrors
  System::Call 'kernel32::GetUserDefaultUILanguage() i .r0'
  ${IfNot} ${Errors}             ; 取不到系统语言就保持默认（英文名），不阻断安装
    IntOp $0 $0 & 0xFFFF         ; 返回类型是 WORD，高 16 位按 ABI 未定义，掩掉
    !insertmacro HG_NAME_FOR_LANG_ID $0
  ${EndIf}

  ; 语言选择器开着时，用户选的语言优先（没选过则 $LANGUAGE 就是系统语言）。
  ; 这个 !if 必须待在宏体内——本文件在模板很靠前的位置被 include，那时
  ; DISPLAYLANGUAGESELECTOR 还没定义；宏体是「插入时才求值」，PREINSTALL
  ; 插入点在模板定义之后，这里才看得到真值（写顶层会恒为假、静默失效）。
  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro HG_NAME_FOR_LANG_ID $LANGUAGE
  !endif
!macroend

; 安装开始（Section Install 首行）：此时语言早已选定、$INSTDIR 也定了，
; 在这里定下快捷方式显示名，供同 section 里后续的快捷方式创建使用。
!macro NSIS_HOOK_PREINSTALL
  !insertmacro HG_RESOLVE_SHORTCUT_NAME
!macroend

; 安装收尾：把「上次安装用的显示名」记的快捷方式改成本次的名字，再记录本次
; 名字供卸载读回（卸载时系统语言可能已经变了，不能重算）、刷新「应用和功能」
; 显示名。
;
; 为什么按「记下来的老名字」而不是写死对比 productName：系统语言可能从中文改成
; 英文（或反过来），也可能只是简体↔繁体切换——只有拿上一次实际用的名字做基准，
; 才能双向都改对。没记过（从没跑过本钩子的旧版本）就按 productName 算，那正是
; 旧版本用的名字。
;
; 升级（尤其是自更新的 /UPDATE 静默安装）不会重建快捷方式，不改名就会一直
; 留着旧名字的图标，甚至新旧两个并存。
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $HG_OldName SHCTX "${UNINSTKEY}" "ShortcutName"
  ${If} $HG_OldName == ""
    StrCpy $HG_OldName "${PRODUCTNAME}"
  ${EndIf}
  ${If} $HG_OldName != $AppShortcutName
    ; $AppStartMenuFolder 为空时路径会变成双反斜杠，Windows 一样认
    !insertmacro HG_RENAME_SHORTCUT "$SMPROGRAMS\\$AppStartMenuFolder" "$HG_OldName"
    !insertmacro HG_RENAME_SHORTCUT "$DESKTOP" "$HG_OldName"
  ${EndIf}

  WriteRegStr SHCTX "${UNINSTKEY}" "ShortcutName" "$AppShortcutName"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "$AppShortcutName"
!macroend

; 把 <目录>\\<旧名>.lnk 换成当前显示名的同名快捷方式。
; 只动「指向本程序 exe」的快捷方式：同名但目标不是本程序的 .lnk 不碰；
; 用户本来就没有某个快捷方式时（FileExists 为假）也不凭空给他建一个。
; 调用方已保证 <旧名> != 当前显示名。
!macro HG_RENAME_SHORTCUT DIR OLDNAME
  ${If} ${FileExists} "${DIR}\\${OLDNAME}.lnk"
    !insertmacro IsShortcutTarget "${DIR}\\${OLDNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      CreateShortcut "${DIR}\\$AppShortcutName.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "${DIR}\\$AppShortcutName.lnk"
      Delete "${DIR}\\${OLDNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend
"""


def build() -> str:
    titles = "\n".join(
        f'LangString ^Name {INSTALLER_LANG_IDS[lang]} "{name}"' for lang, name in TITLE_BY_LANG.items()
    )
    # 一张 ID → 名字的判定链：${If} $0 == <id> / ${ElseIf} ...（末尾自己闭合）
    branches = []
    for i, (lang_id, name) in enumerate(NAME_BY_UI_LANG.items()):
        kw = "${If}" if i == 0 else "${ElseIf}"
        branches.append(f'  {kw} ${{ID}} == {lang_id}\n    StrCpy $AppShortcutName "{name}"')
    branches.append("  ${EndIf}")
    body = BODY.replace("@@TITLE_LANGSTRINGS@@", titles).replace(
        "@@LANG_BRANCHES@@", "\n".join(branches)
    )
    assert "@@" not in body, "还有没替换的占位符"
    return body


def main() -> None:
    body = build()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    # 必须 UTF-8 + BOM：见文件头「编码要求」
    OUT.write_bytes(b"\xef\xbb\xbf" + body.encode("utf-8"))
    print(f"wrote {OUT.relative_to(ROOT)}")
    print("  安装器标题 LangString：" + "、".join(f"{k}({INSTALLER_LANG_IDS[k]})" for k in TITLE_BY_LANG))
    print("  快捷方式显示名：")
    for lang_id, name in NAME_BY_UI_LANG.items():
        print(f"    UI 语言 {lang_id} → {name}")
    print("    其它 → ${PRODUCTNAME}（英文）")


if __name__ == "__main__":
    main()
