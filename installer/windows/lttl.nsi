Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"
Name "lttl ${VERSION}"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\lttl"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "0.1.0.0"
VIAddVersionKey "ProductName" "lttl"
VIAddVersionKey "FileDescription" "lttl Windows x64 installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "2026 lttl contributors"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\lttl"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${PAYLOAD}\licenses\INSTALLER-LICENSES.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "lttl (required)" Core
  SectionIn RO
  SetOutPath "$INSTDIR"
  File /r /x gpu "${PAYLOAD}\*.*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\lttl"
  CreateShortcut "$SMPROGRAMS\lttl\lttl.lnk" "$INSTDIR\lttl.exe" "gui"
  CreateShortcut "$SMPROGRAMS\lttl\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "lttl"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "lttl contributors"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section /o "NVIDIA Zstandard acceleration (NVIDIA terms)" Nvidia
  SetOutPath "$INSTDIR\gpu"
  File "${PAYLOAD}\gpu\*.*"
SectionEnd

Section /o "Windows Explorer context menu" Explorer
  nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\install-explorer.ps1$\" -ExecutablePath $\"$INSTDIR\lttl.exe$\"'
  Pop $0
  Pop $1
  ${If} $0 == 0
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "ExplorerInstalled" 1
  ${Else}
    DetailPrint "Explorer integration could not be installed; existing registrations were preserved."
    MessageBox MB_OK|MB_ICONEXCLAMATION "lttl installed, but Explorer integration could not be installed. Review existing lttl registrations before retrying." /SD IDOK
    SetErrorLevel 3
  ${EndIf}
SectionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK "This installer requires Windows x64." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK "Windows 10 or newer is required." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != ""
    MessageBox MB_OK "Uninstall the existing lttl installation before installing this version. Your archives and identity profile will be preserved." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/NVIDIA" $1
  ${IfNot} ${Errors}
    !insertmacro SelectSection ${Nvidia}
  ${EndIf}
FunctionEnd

Function .onVerifyInstDir
  IfFileExists "$INSTDIR\lttl.exe" 0 +2
    Abort
FunctionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != $INSTDIR
    MessageBox MB_OK "Installation ownership does not match. Nothing was removed." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  ReadRegDWORD $0 HKCU "${UNINSTALL_KEY}" "ExplorerInstalled"
  ${If} $0 == 1
    nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\uninstall-explorer.ps1$\"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      MessageBox MB_OK "Explorer entries could not be removed safely. Resolve the conflict and retry uninstall." /SD IDOK
      SetErrorLevel 3
      Abort
    ${EndIf}
  ${EndIf}
  !include "${DELETE_MANIFEST}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\lttl\lttl.lnk"
  Delete "$SMPROGRAMS\lttl\Uninstall.lnk"
  RMDir "$SMPROGRAMS\lttl"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${Core} "Desktop/CLI, CPU compression, portable GPU LZ4, licenses and user documentation."
  !insertmacro MUI_DESCRIPTION_TEXT ${Nvidia} "Optional NVIDIA nvCOMP/CUDA runtime component for compatible NVIDIA GPUs. NVIDIA licenses are installed alongside the DLLs."
  !insertmacro MUI_DESCRIPTION_TEXT ${Explorer} "Per-user Create archive, Extract here and Extract to folder actions. No default file associations are changed."
!insertmacro MUI_FUNCTION_DESCRIPTION_END
