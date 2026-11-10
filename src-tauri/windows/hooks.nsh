; Uninstall hygiene for the Windows (NSIS) installer. Referenced from tauri.windows.conf.json
; (bundle.windows.nsis.installerHooks). UNTESTED: written on macOS.
;
; $UpdateMode and $DeleteAppDataCheckboxState are variables of Tauri's NSIS template:
;   $UpdateMode = 1                  the uninstaller is running as part of an app update
;   $DeleteAppDataCheckboxState = 1  the user ticked "Delete the application data" on the
;                                    uninstaller's confirmation page
;
; On a real uninstall (never during an update):
;   * the "start at login" entry (HKCU\...\Run, value "Thoughts") is always removed;
;   * %USERPROFILE%\.thoughts (settings, chats, customisations) is removed only when the user
;     ticked "Delete the application data". The default keeps it.
; The library (the user's own notes folder) is never touched.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Thoughts"
    ${If} $DeleteAppDataCheckboxState = 1
      ; One explicit folder. Skipped if the profile path is unknown, so this can never become "\.thoughts".
      ${If} $PROFILE != ""
      ${AndIf} ${FileExists} "$PROFILE\.thoughts\*.*"
        RMDir /r "$PROFILE\.thoughts"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
