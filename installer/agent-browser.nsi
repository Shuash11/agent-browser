!define APPNAME "Agent Browser"
!ifndef VERSION
  !define VERSION "0.1.1"
!endif
!define PUBLISHER "Shuash11"

Name "${APPNAME} ${VERSION}"
OutFile "output\AgentBrowser-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\Programs\Agent Browser"
InstallDirRegKey HKCU "Software\AgentBrowser" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show
AutoCloseWindow false

Page directory
Page instfiles

Section "Install (required)" SEC01
  SectionIn RO
  SetOutPath "$INSTDIR"
  File /r "staging\*.*"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\target\release\agent-browser.exe" "" "$INSTDIR\target\release\agent-browser.exe" 0
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\target\release\agent-browser.exe" "" "$INSTDIR\target\release\agent-browser.exe" 0

  WriteRegStr HKCU "Software\AgentBrowser" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser" "DisplayName" "${APPNAME} ${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser" "NoModificationWnd" 1
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\AgentBrowser"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentBrowser"
SectionEnd
