!include "LogicLib.nsh"
!include "FileFunc.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; The helper only registers a consumer for an already matching managed package.
  ; A clean Splitwave installation therefore does not install or elevate for VB-CABLE.
  ExecWait '"$INSTDIR\Splitwave.exe" --vb-cable-helper register-consumer "$INSTDIR"' $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Tauri invokes the old uninstaller during updates with /UPDATE.
  ${GetOptions} "$CMDLINE" "/UPDATE" $0
  ${IfNot} ${Errors}
    Goto vb_cable_done
  ${EndIf}

  ; There is no safe confirmation path in silent mode.
  IfSilent vb_cable_done

  ExecWait '"$INSTDIR\Splitwave.exe" --vb-cable-helper unregister "$INSTDIR"' $0
  ${If} $0 != 20
    Goto vb_cable_done
  ${EndIf}

  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "VB-CABLE was installed by Splitwave and may also be used by other applications.$\r$\n$\r$\nRemove VB-CABLE as well?" IDYES vb_cable_remove
  ExecWait '"$INSTDIR\Splitwave.exe" --vb-cable-helper retain "$INSTDIR"' $0
  Goto vb_cable_done

  vb_cable_remove:
    ExecWait '"$INSTDIR\Splitwave.exe" --vb-cable-helper remove "$INSTDIR"' $0

  vb_cable_done:
!macroend
