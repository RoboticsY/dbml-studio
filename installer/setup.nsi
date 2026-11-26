Unicode true
!include "MUI2.nsh"
!ifndef APP_EXE
  !error "Pass /DAPP_EXE=<release binary>"
!endif
!ifndef OUTPUT_FILE
  !error "Pass /DOUTPUT_FILE=<installer path>"
!endif
!ifndef NOTICES
  !error "Pass /DNOTICES=<third-party notices>"
!endif
!ifndef VERSION
  !error "Pass /DVERSION=<version>"
!endif
Name "DBML Studio"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\DBML Studio"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "DBML Studio"
VIAddVersionKey "FileDescription" "DBML Studio Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "RoboticsY"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\RoboticsY.DBMLStudio"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Japanese"
Section "DBML Studio"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /oname=DBML-Studio.exe "${APP_EXE}"
  File /oname=THIRD-PARTY-NOTICES.txt "${NOTICES}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\DBML Studio.lnk" "$INSTDIR\DBML-Studio.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "DBML Studio"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "RoboticsY"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\DBML-Studio.exe"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd
Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\DBML Studio.lnk"
  Delete "$INSTDIR\DBML-Studio.exe"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
