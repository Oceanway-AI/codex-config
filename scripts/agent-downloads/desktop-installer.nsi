Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
Name "${APP_NAME} 离线安装"
OutFile "${OUTPUT_FILE}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
SetCompressorDictSize 32
ShowInstDetails show
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_TITLE "安装完成"
!define MUI_FINISHPAGE_TEXT "请回到 Oceanway Config，点击“重新检测”，然后继续配置。"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Install"
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "此安装包需要 64 位 Windows。" /SD IDOK
    Abort
  ${EndIf}
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\package"
  File /r "${PAYLOAD_DIR}/*"
  DetailPrint "正在校验官方安装包并安装，请稍候…"
  ${DisableX64FSRedirection}
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\package\install-desktop.ps1" -Elevated'
  Pop $0
  ${EnableX64FSRedirection}
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "安装未完成。请查看上方详细信息，关闭正在运行的目标应用后重试。" /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
SectionEnd
