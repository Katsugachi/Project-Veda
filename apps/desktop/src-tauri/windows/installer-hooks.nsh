!include "LogicLib.nsh"
!include "FileFunc.nsh"

; Palor's installer is small, but the application immediately downloads models,
; indexes and selected docsets. Refuse installation when the target drive cannot
; provide the product's 10 GiB working-space floor.
!macro NSIS_HOOK_PREINSTALL
  ${GetRoot} "$INSTDIR" $R0
  ${DriveSpace} "$R0\" "/D=F /S=M" $R1
  ${If} ${Errors}
    MessageBox MB_ICONEXCLAMATION|MB_OK "Palor could not verify free space on $R0. Installation cannot continue."
    Abort
  ${EndIf}
  ${If} $R1 < 10240
    MessageBox MB_ICONSTOP|MB_OK "Palor requires at least 10 GB of free SSD space before installation. The selected drive has approximately $R1 MB free."
    Abort
  ${EndIf}
!macroend
