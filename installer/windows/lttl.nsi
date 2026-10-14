Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"
Name "lttl ${VERSION}"
OutFile "${OUTPUT}"
InstallDir "$PROGRAMFILES64\lttl"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
VIProductVersion "0.1.0.6"
VIAddVersionKey "ProductName" "lttl"
VIAddVersionKey "FileDescription" "lttl Windows x64 installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "2026 lttl contributors"
Var RegisteredDirectory
Var HadExplorer

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\lttl"
!define MUI_ICON "${PAYLOAD}\assets\lttl.ico"
!define MUI_UNICON "${PAYLOAD}\assets\lttl.ico"
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
  ; Validate at installation time with an explanation, not by silently disabling
  ; the directory page's Install button. Registered installs support repair.
  ${If} $RegisteredDirectory != ""
  ${AndIf} $RegisteredDirectory != $INSTDIR
    MessageBox MB_OK|MB_ICONEXCLAMATION "lttl is installed in $RegisteredDirectory. Select that folder to upgrade, or uninstall it before changing the location." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  IfFileExists "$INSTDIR\lttl.exe" 0 preflight_done
  ${If} $RegisteredDirectory != $INSTDIR
    MessageBox MB_OK|MB_ICONEXCLAMATION "This folder contains an unregistered lttl.exe. Choose another folder or remove that copy first. No files were changed." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  ; Detect a running/locked executable before replacing any payload files.
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\lttl.exe", i 0x40000000, i 1, p 0, i 3, i 0, p 0) p .r0'
  ${If} $0 == -1
    MessageBox MB_OK|MB_ICONEXCLAMATION "Close lttl and try again. The installed executable cannot be replaced." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  System::Call 'kernel32::CloseHandle(p r0)'
preflight_done:
  IfFileExists "$INSTDIR\lttl-gui.exe" 0 desktop_preflight_done
  ${If} $RegisteredDirectory != $INSTDIR
    MessageBox MB_OK|MB_ICONEXCLAMATION "This folder contains an unregistered lttl-gui.exe. Choose another folder. No files were changed." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\lttl-gui.exe", i 0x40000000, i 1, p 0, i 3, i 0, p 0) p .r0'
  ${If} $0 == -1
    MessageBox MB_OK|MB_ICONEXCLAMATION "Close lttl and try again. The desktop app is still running or locked." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  System::Call 'kernel32::CloseHandle(p r0)'
desktop_preflight_done:
  SetOverwrite on
  SetOutPath "$INSTDIR"
  ClearErrors
  File /r /x gpu "${PAYLOAD}\*.*"
  ${If} ${Errors}
    SetErrorLevel 2
    Abort "Some files could not be installed. Close lttl and run setup again."
  ${EndIf}
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\lttl"
  CreateShortcut "$SMPROGRAMS\lttl\lttl.lnk" "$INSTDIR\lttl-gui.exe" "" "$INSTDIR\lttl-gui.exe" 0
  CreateShortcut "$SMPROGRAMS\lttl\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "lttl"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" '$\"$INSTDIR\lttl-gui.exe$\",0'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "lttl contributors"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section /o "NVIDIA Zstandard acceleration (NVIDIA terms)" Nvidia
  SetOutPath "$INSTDIR\gpu"
  File "${PAYLOAD}\gpu\*.*"
SectionEnd

Section /o "Windows Explorer context menu (all users)" Explorer
  ${If} $HadExplorer == 1
    ; Refresh owned commands on upgrade so older registrations use the GUI launcher.
    nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\uninstall-explorer.ps1$\" -Scope AllUsers'
    Pop $0
    Pop $1
    ${If} $0 != 0
      SetErrorLevel 3
      Abort "Explorer commands could not be upgraded safely. Resolve the conflict and retry setup."
    ${EndIf}
  ${EndIf}
  nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\install-explorer.ps1$\" -ExecutablePath $\"$INSTDIR\lttl.exe$\" -Scope AllUsers'
  Pop $0
  Pop $1
  ${If} $0 == 0
    WriteRegDWORD HKLM "${UNINSTALL_KEY}" "ExplorerInstalled" 1
  ${Else}
    DetailPrint "Explorer integration could not be installed; existing registrations were preserved."
    MessageBox MB_OK|MB_ICONEXCLAMATION "lttl installed, but Explorer integration could not be installed. Review existing lttl registrations before retrying." /SD IDOK
    SetErrorLevel 3
  ${EndIf}
SectionEnd

Section "-Remove deselected optional components"
  ${IfNot} ${SectionIsSelected} ${Explorer}
  ${AndIf} $HadExplorer == 1
    nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\uninstall-explorer.ps1$\" -Scope AllUsers'
    Pop $0
    Pop $1
    ${If} $0 != 0
      SetErrorLevel 3
      Abort "Explorer commands could not be removed. Resolve the conflict and retry setup."
    ${EndIf}
    DeleteRegValue HKLM "${UNINSTALL_KEY}" "ExplorerInstalled"
  ${EndIf}
  ${IfNot} ${SectionIsSelected} ${Nvidia}
    Delete "$INSTDIR\gpu\lttl_gpu.dll"
    Delete "$INSTDIR\gpu\nvcomp64_5.dll"
    Delete "$INSTDIR\gpu\nvcomp_cpu64_5.dll"
    Delete "$INSTDIR\gpu\cudart64_12.dll"
    Delete "$INSTDIR\gpu\LICENSE-nvcomp.txt"
    Delete "$INSTDIR\gpu\LICENSE-cuda-runtime.txt"
    Delete "$INSTDIR\gpu\NOTICE-nvcomp.txt"
    RMDir "$INSTDIR\gpu"
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
  SetShellVarContext all
  ReadRegStr $RegisteredDirectory HKLM "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $RegisteredDirectory != ""
  ${AndIf} $INSTDIR == "$PROGRAMFILES64\lttl"
    StrCpy $INSTDIR $RegisteredDirectory
  ${EndIf}
  ReadRegDWORD $HadExplorer HKLM "${UNINSTALL_KEY}" "ExplorerInstalled"
  ${If} $HadExplorer == 1
    !insertmacro SelectSection ${Explorer}
  ${EndIf}
  ${If} $RegisteredDirectory != ""
  ${AndIf} ${FileExists} "$RegisteredDirectory\gpu\lttl_gpu.dll"
    !insertmacro SelectSection ${Nvidia}
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/NVIDIA" $1
  ${IfNot} ${Errors}
    !insertmacro SelectSection ${Nvidia}
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/EXPLORER" $1
  ${IfNot} ${Errors}
    !insertmacro SelectSection ${Explorer}
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext all
  ReadRegStr $0 HKLM "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 != $INSTDIR
    MessageBox MB_OK "Installation ownership does not match. Nothing was removed." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  ReadRegDWORD $0 HKLM "${UNINSTALL_KEY}" "ExplorerInstalled"
  ${If} $0 == 1
    nsExec::ExecToStack '$\"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe$\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $\"$INSTDIR\scripts\uninstall-explorer.ps1$\" -Scope AllUsers'
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
  DeleteRegKey HKLM "${UNINSTALL_KEY}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${Core} "Desktop/CLI, CPU compression, portable GPU LZ4, licenses and user documentation."
  !insertmacro MUI_DESCRIPTION_TEXT ${Nvidia} "Optional NVIDIA nvCOMP/CUDA runtime component for compatible NVIDIA GPUs. NVIDIA licenses are installed alongside the DLLs."
  !insertmacro MUI_DESCRIPTION_TEXT ${Explorer} "All-users Create archive, Extract here and Extract to folder actions. No default file associations are changed."
!insertmacro MUI_FUNCTION_DESCRIPTION_END
