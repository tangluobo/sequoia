; Match the original Java installer pages, language, location and shortcuts.
Unicode True
!include "MUI2.nsh"
!include "${OWNED_FILES}"
Name "Tomato ${VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$PROGRAMFILES64\Tomato"
InstallDirRegKey HKLM "Software\tangluobo\Tomato" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
Icon "${ICON_FILE}"
!define MUI_ABORTWARNING
!define MUI_ICON "${ICON_FILE}"
!define MUI_UNICON "${ICON_FILE}"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Tomato" MainSection
  SetShellVarContext all
  SetRegView 64
  !insertmacro InstallPayload
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Tomato"
  CreateShortcut "$SMPROGRAMS\Tomato\Tomato.lnk" "$INSTDIR\tomato.exe" "" "$INSTDIR\tomato.exe"
  CreateShortcut "$DESKTOP\Tomato.lnk" "$INSTDIR\tomato.exe" "" "$INSTDIR\tomato.exe"
  WriteRegStr HKLM "Software\tangluobo\Tomato" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "DisplayName" "Tomato"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "Publisher" "tangluobo"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "DisplayIcon" "$INSTDIR\tomato.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64
  Delete "$DESKTOP\Tomato.lnk"
  Delete "$SMPROGRAMS\Tomato\Tomato.lnk"
  RMDir "$SMPROGRAMS\Tomato"
  ; Remove only packaged files. User additions and ~/.tomato remain intact.
  !insertmacro UninstallPayload
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\tangluobo\Tomato"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tomato"
SectionEnd
