Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"

!define APP_KEY "Software\HongguoNSH"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\HongguoNSH"
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\Hongguo.exe"
!define MUI_LANGDLL_REGISTRY_ROOT HKCU
!define MUI_LANGDLL_REGISTRY_KEY "${APP_KEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Language"

Name "$(ProductName)"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\HongguoNSH"
InstallDirRegKey HKCU "${APP_KEY}" "InstallDir"
VIProductVersion "${PRODUCT_VERSION}.0"
VIAddVersionKey "ProductName" "Hongguo NSH"
VIAddVersionKey "FileDescription" "Hongguo NSH installer"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "LegalCopyright" "hongguo-desktop contributors"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

LangString ProductName ${LANG_SIMPCHINESE} "红果 NSH"
LangString ProductName ${LANG_ENGLISH} "Hongguo NSH"
LangString UnsupportedOS ${LANG_SIMPCHINESE} "需要 64 位 Windows 10 1809 或更新版本。"
LangString UnsupportedOS ${LANG_ENGLISH} "Requires 64-bit Windows 10 version 1809 or later."

Function .onInit
  !insertmacro MUI_LANGDLL_DISPLAY
  SetRegView 64
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "$(UnsupportedOS)"
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "$(UnsupportedOS)"
    Abort
  ${EndIf}
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${If} $0 < 17763
    MessageBox MB_ICONSTOP "$(UnsupportedOS)"
    Abort
  ${EndIf}
FunctionEnd

Section
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${BUNDLE_DIR}/*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\HongguoNSH"
  CreateShortcut "$SMPROGRAMS\HongguoNSH\$(ProductName).lnk" "$INSTDIR\Hongguo.exe"
  CreateShortcut "$DESKTOP\$(ProductName).lnk" "$INSTDIR\Hongguo.exe"
  WriteRegStr HKCU "${APP_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "$(ProductName)"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "hongguo-desktop"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\Hongguo.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Function un.onInit
  SetRegView 64
  !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  !include "${UNINSTALL_FILES}"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\HongguoNSH\$(ProductName).lnk"
  RMDir "$SMPROGRAMS\HongguoNSH"
  Delete "$DESKTOP\$(ProductName).lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${APP_KEY}"
  RMDir "$INSTDIR"
SectionEnd
