; Arbiter installer hooks. The background service keeps running after the
; window closes, so stop it before files are replaced or removed. Agents it
; was running stop too; the next start recovers and pushes memory first.
!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'taskkill /IM arbiterd.exe /F'
  nsExec::Exec 'taskkill /IM arbiter-mcp.exe /F'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /IM arbiterd.exe /F'
  nsExec::Exec 'taskkill /IM arbiter-mcp.exe /F'
!macroend
