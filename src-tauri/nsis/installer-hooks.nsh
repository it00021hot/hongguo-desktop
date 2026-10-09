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

LangString ^Name 2052 "红果播放器"
LangString ^Name 1028 "紅果播放器"

; 系统 UI 语言 → 应用显示名。安装器界面语言（$LANGUAGE）只在用户**手选过**时
; 才作数，所以拆成两步：先按系统语言定，用户选过再按选的那门定。不能只看
; $LANGUAGE——系统语言不在 languages 列表里时它会回落成列表第一项（English），
; 于是中文 Windows 上装出来是英文名；也不能只看系统语言——那样手选语言后又
; 会出现「界面中文、快捷方式叫英文」的错位。两步合起来两种情况都对。
!macro HG_NAME_FOR_LANG_ID ID
  StrCpy $AppShortcutName "${PRODUCTNAME}"
  ${If} ${ID} == 2052
    StrCpy $AppShortcutName "红果播放器"
  ${ElseIf} ${ID} == 4100
    StrCpy $AppShortcutName "红果播放器"
  ${ElseIf} ${ID} == 1028
    StrCpy $AppShortcutName "紅果播放器"
  ${ElseIf} ${ID} == 3076
    StrCpy $AppShortcutName "紅果播放器"
  ${ElseIf} ${ID} == 5124
    StrCpy $AppShortcutName "紅果播放器"
  ${EndIf}
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
    !insertmacro HG_RENAME_SHORTCUT "$SMPROGRAMS\$AppStartMenuFolder" "$HG_OldName"
    !insertmacro HG_RENAME_SHORTCUT "$DESKTOP" "$HG_OldName"
  ${EndIf}

  WriteRegStr SHCTX "${UNINSTKEY}" "ShortcutName" "$AppShortcutName"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "$AppShortcutName"
!macroend

; 把 <目录>\<旧名>.lnk 换成当前显示名的同名快捷方式。
; 只动「指向本程序 exe」的快捷方式：同名但目标不是本程序的 .lnk 不碰；
; 用户本来就没有某个快捷方式时（FileExists 为假）也不凭空给他建一个。
; 调用方已保证 <旧名> != 当前显示名。
!macro HG_RENAME_SHORTCUT DIR OLDNAME
  ${If} ${FileExists} "${DIR}\${OLDNAME}.lnk"
    !insertmacro IsShortcutTarget "${DIR}\${OLDNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      CreateShortcut "${DIR}\$AppShortcutName.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "${DIR}\$AppShortcutName.lnk"
      Delete "${DIR}\${OLDNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend
