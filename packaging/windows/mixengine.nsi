; MixLab — a per-user installer.
;
; `RequestExecutionLevel user` is the whole point: nothing here asks for UAC, so an update needs no
; administrator. The one file that must live somewhere an ordinary account cannot rewrite,
; `mixengine-elevate.exe`, is *not* placed by this installer — MixEngine installs it itself, inside
; the elevation prompt first-run setup already costs. See ADR 0015 and the T85 design, D1.
;
; Driven by packaging/windows/build.sh, which defines VERSION, STAGE, OUTFILE and INSTALL_SUBDIR —
; the last of them packaging/common.sh's MIX_INSTALL_WINDOWS, which mixengine-platform's
; `install::program_dirs` reads too, so the daemon and the window look where this writes (T107).
; ICON is the window's own icon, for the setup, the uninstaller and Installed apps (T182, D10).
;
; **The uninstaller removes MixEngine and MixLab completely, or changes nothing** — T182, design
; docs/specs/2026-09-24-t182-removing-mixlab-is-one-act-design.md. Two promises hold it together:
; everything that could stop it half-way is checked on the choices page while nothing has changed
; (P2), and `uninstall.exe` with its Installed apps entry goes last, so a run that stops can always
; be run again (P1).

Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma
; Common controls 6, which draw the progress bar as a marquee (T182b) and give every page the
; system's own look.
XPStyle on

!include "WinMessages.nsh"
!include "LogicLib.nsh"
!include "nsDialogs.nsh"

; **HEADLESS builds the setup without the window** — T182b, D5, which replaces the headless zip. The
; same directory, uninstall entry and helper: one install or the other, and installing either
; replaces the other.
!ifdef HEADLESS
  !define NAME "MixEngine"
!else
  !define NAME "MixLab"
!endif
!define PUBLISHER "MixLab"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\MixEngine"

; Past this, `ReadRegStr` may have handed back a truncated value — see `AddToPath`.
!define PATH_LIMIT 1000

Name "${NAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${INSTALL_SUBDIR}"
InstallDirRegKey HKCU "Software\MixEngine" "InstallDir"
ShowInstDetails show
ShowUninstDetails show
Icon "${ICON}"
UninstallIcon "${ICON}"

; **A components page, for one optional thing** — T105. The desktop shortcut is the only choice this
; installer offers, and `/S` (which `packaging/windows/probe.sh` uses) takes the defaults, so an
; unattended install still writes exactly what it wrote before plus the window itself.
Page components
Page directory
Page instfiles
UninstPage uninstConfirm "" un.ConfirmShow
UninstPage custom un.ChoicesPage un.ChoicesLeave
; What is in the way, on a page of its own before anything changes — T182e, D6. Skipped when nothing is.
UninstPage custom un.InUsePage
UninstPage instfiles "" un.InstFilesShow

; The uninstaller's two choices (T182, D7). "1" keeps. Both default to keeping, which is also what
; `/S` gets: an unattended uninstall never deletes somebody's databases.
Var KeepHome
Var KeepRelocated
; What `mix uninstall --dry-run --relocated` printed: the folders `[paths]` moved out of the home.
Var Relocated
Var HomeBox
Var RelocatedBox
; Collected by the checks: files that cannot be opened, and files that could not be deleted.
Var Locked
Var Stuck
; 1 while un.onInit's banner is still up, for un.ConfirmShow to take down once the window is in front.
Var BannerUp
; What `mix uninstall --dry-run --blocked` printed, one program per line; empty when nothing is in the
; way. And the label on the in-use page that shows it, so Check again can rewrite it.
Var InUse
Var InUseList

; "Is $1 somewhere inside $0?" — leaves 1 in $2 when it is and 0 when it is not.
;
; **A macro and not a function, and both of these are.** The NSIS convention for a function is to
; take its arguments on the stack and hand the result back the same way, which is four `Exch`es
; whose ordering is easy to get subtly wrong and impossible to test from here. Nothing outside this
; file calls either of these, so there is no convention to keep: expanded inline, they are ordinary
; straight-line code over `$0`–`$5` and a reader can check them by reading them.
!macro StrFind
  StrLen $3 $1
  StrCpy $4 0
  StrCpy $2 0
  ${Do}
    StrCpy $5 $0 $3 $4
    ${If} $5 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $5 == $1
      StrCpy $2 1
      ${ExitDo}
    ${EndIf}
    IntOp $4 $4 + 1
  ${Loop}
!macroend

; "$0 with the first occurrence of $1 removed" — leaves the result in $0.
!macro StrCut
  StrLen $3 $1
  StrCpy $4 0
  ${Do}
    StrCpy $5 $0 $3 $4
    ${If} $5 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $5 == $1
      StrCpy $6 $0 $4
      IntOp $7 $4 + $3
      StrCpy $7 $0 "" $7
      StrCpy $0 "$6$7"
      ${ExitDo}
    ${EndIf}
    IntOp $4 $4 + 1
  ${Loop}
!macroend

; Delete `Software\Classes\<scheme>` — **only if it is still ours.**
;
; Reads the command back and looks for our own `$INSTDIR` inside it: a machine where another
; program registered the scheme after us keeps that program's handler, which is the correct
; outcome and the quiet one. A macro so the installer and the uninstaller can both expand it.
!macro RemoveSchemeIfOurs SCHEME
  ReadRegStr $0 HKCU "Software\Classes\${SCHEME}\shell\open\command" ""
  ${If} $0 != ""
    StrCpy $1 "$INSTDIR"
    !insertmacro StrFind
    ${If} $2 == 1
      DeleteRegKey HKCU "Software\Classes\${SCHEME}"
    ${Else}
      DetailPrint "${SCHEME}:// points somewhere else; leaving it alone."
    ${EndIf}
  ${EndIf}
!macroend

; Add ${FILE} to $Locked when it is there and cannot be opened for writing. A running program's
; image is mapped, and a mapped file cannot be opened for writing, so this finds whatever started it.
!macro CheckWritable FILE
  ${If} ${FileExists} "${FILE}"
    ClearErrors
    FileOpen $0 "${FILE}" a
    ${If} ${Errors}
      StrCpy $Locked "$Locked$\r$\n${FILE}"
    ${Else}
      FileClose $0
    ${EndIf}
  ${EndIf}
!macroend

; Delete ${FILE}, and add it to $Stuck when it was there and could not be deleted. `Delete` sets the
; error flag only for a file that exists and stayed.
!macro RemoveChecked FILE
  ClearErrors
  Delete "${FILE}"
  ${If} ${Errors}
    StrCpy $Stuck "$Stuck$\r$\n${FILE}"
  ${EndIf}
!macroend

; The progress bar runs as a marquee while one long command runs — T182b. A section runs in a
; thread of its own and the window keeps pumping messages, so the bar animates by itself while
; `mix uninstall` removes a gigabyte, instead of standing still for a minute. 1004 is the bar on the
; progress page; PBS_MARQUEE is 0x08 and PBM_SETMARQUEE is WM_USER + 10. Silent, there is no window,
; and every call below lands on nothing.
!macro MarqueeOn
  FindWindow $R8 "#32770" "" $HWNDPARENT
  GetDlgItem $R9 $R8 1004
  System::Call "user32::GetWindowLongW(p R9, i -16) i .R7"
  IntOp $R7 $R7 | 0x08
  System::Call "user32::SetWindowLongW(p R9, i -16, i R7)"
  SendMessage $R9 0x40A 1 30
!macroend

!macro MarqueeOff
  SendMessage $R9 0x40A 0 0
  System::Call "user32::GetWindowLongW(p R9, i -16) i .R7"
  IntOp $R7 $R7 & 0xFFFFFFF7
  System::Call "user32::SetWindowLongW(p R9, i -16, i R7)"
!macroend

; How much larger than NSIS's own the uninstaller's window is, in dialog units — T182b. The log is
; what a person reads when an uninstall stops, and at NSIS's size its lines were cut off after
; sixty characters. Dialog units, so the growth follows the display's scaling and font.
!define GROW_X 160
!define GROW_Y 100

; Move and size the window $0 by the rectangle in $1..$4 (left, top, width, height), in the client
; coordinates of its parent. 0x14 is SWP_NOZORDER | SWP_NOACTIVATE.
!macro PlaceWindow
  System::Call "user32::SetWindowPos(p r0, p 0, i r1, i r2, i r3, i r4, i 0x14)"
!macroend

; The rectangle of window $0 in the client coordinates of window $5, into $1..$4 as left, top,
; right, bottom. $9 is scratch for the RECT.
!macro RectIn
  System::Call "*(i 0, i 0, i 0, i 0) p .r9"
  System::Call "user32::GetWindowRect(p r0, p r9)"
  System::Call "user32::MapWindowPoints(p 0, p r5, p r9, i 2)"
  System::Call "*$9(i .r1, i .r2, i .r3, i .r4)"
  System::Free $9
!macroend

; The client size of window ${WINDOW} into $6 (width) and $7 (height). $9 is scratch.
!macro ClientSize WINDOW
  System::Call "*(i 0, i 0, i 0, i 0) p .r9"
  System::Call "user32::GetClientRect(p ${WINDOW}, p r9)"
  System::Call "*$9(i, i, i .r6, i .r7)"
  System::Free $9
!macroend

; Stretch the page on show to fill the area the enlarged window gives it — T182b. NSIS creates each
; page at its template's size and only moves it into place, so a control that reaches the page's
; right edge is widened with it and one that reaches its bottom is made taller: the log, the
; progress bar and the text above them, and nothing else. $0-$9 and $R0-$R2 are scratch.
!macro FitPage
  FindWindow $R0 "#32770" "" $HWNDPARENT
  GetDlgItem $R1 $HWNDPARENT 1018
  !insertmacro ClientSize $R1
  StrCpy $R2 $6
  StrCpy $8 $7
  !insertmacro ClientSize $R0
  IntOp $R2 $R2 - $6
  IntOp $8 $8 - $7
  ${If} $R2 > 0
  ${OrIf} $8 > 0
    ; The page's old size, kept in $6/$7 for the edge tests below.
    StrCpy $5 $R0
    System::Call "user32::GetWindow(p R0, i 5) p .r0"
    ${DoWhile} $0 != 0
      !insertmacro RectIn
      IntOp $3 $3 - $1
      IntOp $4 $4 - $2
      IntOp $9 $1 + $3
      IntOp $9 $9 + 4
      ${If} $9 >= $6
        IntOp $3 $3 + $R2
      ${EndIf}
      IntOp $9 $2 + $4
      IntOp $9 $9 + 4
      ${If} $9 >= $7
        IntOp $4 $4 + $8
      ${EndIf}
      !insertmacro PlaceWindow
      System::Call "user32::GetWindow(p r0, i 2) p .r0"
    ${Loop}
    IntOp $6 $6 + $R2
    IntOp $7 $7 + $8
    ; 0x16 is SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE.
    System::Call "user32::SetWindowPos(p R0, p 0, i 0, i 0, i r6, i r7, i 0x16)"
  ${EndIf}
!macroend

; Size the log's one column to its longest line, and never narrower than the log — T182b. A column
; as wide as the list cut every longer line off with an ellipsis; one as wide as its text gives the
; list a horizontal scroll bar instead. 0x101E is LVM_SETCOLUMNWIDTH, 0x101D LVM_GETCOLUMNWIDTH;
; -1 sizes to the text and -2 to the list. $5-$7 are scratch, so the section's $0 survives it.
!macro FitLog
  FindWindow $5 "#32770" "" $HWNDPARENT
  GetDlgItem $5 $5 1016
  SendMessage $5 0x101E 0 -1
  SendMessage $5 0x101D 0 0 $6
  SendMessage $5 0x101E 0 -2
  SendMessage $5 0x101D 0 0 $7
  ${If} $6 > $7
    SendMessage $5 0x101E 0 $6
  ${EndIf}
!macroend

; RemoveChecked, tried once a second for 30 seconds — T182b, D8. For `mixengined.exe` only: `mix
; uninstall` waits for the daemon's process to end, and this covers a daemon something else started
; a moment after it returned, whose image stays mapped until it has stopped.
!macro RemoveWithRetry FILE
  StrCpy $R2 0
  ${Do}
    ClearErrors
    Delete "${FILE}"
    ${IfNot} ${Errors}
      ${ExitDo}
    ${EndIf}
    IntOp $R2 $R2 + 1
    ${If} $R2 >= 30
      StrCpy $Stuck "$Stuck$\r$\n${FILE}"
      ${ExitDo}
    ${EndIf}
    Sleep 1000
  ${Loop}
!macroend

; Is this install's window running? Asks, then closes it — T182, D8. Leaves 1 in $R0 to go on, and
; 0 to stop.
;
; **Only a process whose image is this file**, so a development build running from a checkout is
; left alone. The path reaches PowerShell through an environment variable rather than inside a quoted
; string, so an install directory holding an apostrophe cannot break the command. The window keeps
; no state of its own that a forced close loses: the daemon holds it.
!macro CloseMixLab UN
Function ${UN}CloseMixLab
  StrCpy $R0 1

  ; **Asked of the file, not of PowerShell** — T182b. A running image cannot be opened for writing,
  ; so this answers in no time, where starting PowerShell to count processes cost a second on every
  ; uninstall. PowerShell is kept for the rare case it is needed: closing a window that is running.
  ${IfNot} ${FileExists} "$INSTDIR\mixlab.exe"
    Return
  ${EndIf}
  ClearErrors
  FileOpen $0 "$INSTDIR\mixlab.exe" a
  ${IfNot} ${Errors}
    FileClose $0
    Return
  ${EndIf}

  MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "MixLab is running. Click OK to close it and continue, or Cancel to stop without changing anything." /SD IDOK IDOK closeit
  StrCpy $R0 0
  Return

  closeit:
  System::Call 'Kernel32::SetEnvironmentVariable(t "MIXLAB_EXE", t "$INSTDIR\mixlab.exe")'
  nsExec::ExecToStack `powershell -NoProfile -NonInteractive -Command "Get-Process mixlab -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq $$env:MIXLAB_EXE } | Stop-Process -Force"`
  Pop $0
  Pop $1
  ; The image is unmapped a moment after the process has gone.
  Sleep 1000
FunctionEnd
!macroend
!insertmacro CloseMixLab ""
!insertmacro CloseMixLab "un."

Section "MixLab" SecCore
  SectionIn RO

  ; **An update over a running copy** — T182, D8. The window is asked about and closed, then the
  ; daemon stops its services and itself; `mix daemon stop` does nothing when none is running.
  ${If} ${FileExists} "$INSTDIR\mixengined.exe"
    Call CloseMixLab
    ${If} $R0 == 0
      Abort "Nothing was changed."
    ${EndIf}
    ; ASCII into the log, which `nsExec` reads in the ANSI code page (T182e).
    System::Call 'Kernel32::SetEnvironmentVariable(t "MIXENGINE_PLAIN_TEXT", t "1")'
    nsExec::ExecToLog '"$INSTDIR\mix.exe" daemon stop'
    Pop $0
  ${EndIf}

  SetOutPath "$INSTDIR"
  File "${STAGE}\mix.exe"
  File "${STAGE}\mixengined.exe"
  ; Beside `mixengined.exe`, which is the only place `core::shims::source` looks. Without it the
  ; daemon starts, answers `status`, and `<root>\bin` stays empty — T85c.
  File "${STAGE}\mixengine-shim.exe"
  ; What `<root>\bin` holds per command name since T185; it runs the shim above to resolve.
  File "${STAGE}\mixengine-trampoline.exe"
  File "${STAGE}\mixengine-elevate.exe"
  ; MixLab, the window — T105. One name on every operating system: `updates::apply::swap` looks a
  ; payload's name up as `directory.join(binary_name(name))` and `binary_name` appends `.exe` and
  ; nothing else, so an install file spelled `MixLab.exe` is one every future update would skip.
  ; What a user actually clicks is the shortcut below, and that is named MixLab.
  !ifndef HEADLESS
    File "${STAGE}\mixlab.exe"
  !endif

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\MixEngine" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  ; The window's icon: `mix.exe` carries none, which left Installed apps with a blank (T182, D10).
  ; Headless, the uninstaller's own, which carries the same icon.
  !ifdef HEADLESS
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\uninstall.exe,0"
  !else
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\mixlab.exe,0"
  !endif
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1

  ; **A flat Start Menu entry and not a one-item folder.** `SetShellVarContext` is left at its
  ; default, which under `RequestExecutionLevel user` is this account's own Start Menu — nothing
  ; here asks for UAC. The `SetOutPath` at the top of this section is also the shortcut's working
  ; directory.
  !ifndef HEADLESS
    CreateShortcut "$SMPROGRAMS\MixLab.lnk" "$INSTDIR\mixlab.exe"

    ; `mixlab://`, per user — ADR 0047. An earlier release registered `mixdb://` to this same
    ; binary, which the window no longer answers; that key goes, but only while it still points
    ; here — a standalone MixDB that holds it is left alone.
    !insertmacro RemoveSchemeIfOurs "mixdb"
    WriteRegStr HKCU "Software\Classes\mixlab" "" "URL:MixLab Protocol"
    WriteRegStr HKCU "Software\Classes\mixlab" "URL Protocol" ""
    WriteRegStr HKCU "Software\Classes\mixlab\DefaultIcon" "" "$INSTDIR\mixlab.exe,0"
    WriteRegStr HKCU "Software\Classes\mixlab\shell\open\command" "" '"$INSTDIR\mixlab.exe" "%1"'
  !endif

  Call AddToPath
SectionEnd

; **Unselected by default**, which is what `/o` means and what makes this optional in the sense the
; roadmap asks for: a person who wants an icon on their desktop ticks a box, and nobody else grows
; one. A silent install takes the defaults, so `probe.sh`'s readings are unchanged.
!ifndef HEADLESS
  Section /o "Desktop shortcut for MixLab" SecDesktop
    SetOutPath "$INSTDIR"
    CreateShortcut "$DESKTOP\MixLab.lnk" "$INSTDIR\mixlab.exe"
  SectionEnd
!endif

; Append $INSTDIR to this user's PATH — and refuse rather than risk it.
;
; **The guard is not decoration.** NSIS's `ReadRegStr` silently truncates at `NSIS_MAX_STRLEN`, so
; writing back what it read can cut a long PATH in half. A PATH that was not extended is an
; inconvenience; a PATH that was truncated is somebody's afternoon. See the T85 design, D10.
;
; `<root>/bin` — the directory of runtime shims — is deliberately *not* written here. That one
; belongs to `path.install`, which writes it when somebody asks and takes it back off again; the two
; therefore own different segments of one value, which is what makes two authors safe.
Function AddToPath
  ReadRegStr $0 HKCU "Environment" "Path"
  StrLen $8 $0

  ${If} $8 >= ${PATH_LIMIT}
    DetailPrint "This account's PATH is too long for the installer to edit safely."
    DetailPrint "Add $INSTDIR to it by hand, or run: mix path install"
    Return
  ${EndIf}

  StrCpy $1 "$INSTDIR"
  !insertmacro StrFind

  ${If} $2 == 1
    Return
  ${EndIf}

  ${If} $0 == ""
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR"
  ${Else}
    WriteRegExpandStr HKCU "Environment" "Path" "$0;$INSTDIR"
  ${EndIf}

  ; So a shell started from Explorer afterwards picks it up without a logout.
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
FunctionEnd

; Take exactly our own segment back out, leaving the rest of the value as it was.
;
; The separator is removed with the directory rather than after it, so a PATH that held only this
; entry does not end up as a lone `;` — and the second pass covers the case where the entry was
; first in the value and had no separator in front of it.
Function un.RemoveFromPath
  ReadRegStr $0 HKCU "Environment" "Path"
  StrLen $8 $0

  ${If} $8 >= ${PATH_LIMIT}
    DetailPrint "This account's PATH is too long to edit safely; $INSTDIR was left on it."
    Return
  ${EndIf}

  StrCpy $1 ";$INSTDIR"
  !insertmacro StrCut

  StrCpy $1 "$INSTDIR"
  !insertmacro StrCut

  WriteRegExpandStr HKCU "Environment" "Path" "$0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
FunctionEnd

; Take `mixlab://` back — **only if it is still ours**. `mixdb://` too, for an install an earlier
; release made and this one's installer never ran over.
Function un.RemoveScheme
  !insertmacro RemoveSchemeIfOurs "mixlab"
  !insertmacro RemoveSchemeIfOurs "mixdb"
FunctionEnd

; **The slow question is asked before any window exists** — T182b, D9. Listing the relocated folders
; starts a daemon and reads the machine, which takes seconds; asked while the choices page was being
; drawn, it left a blank page whose Uninstall button already took clicks. Here nothing can be clicked
; yet, and the banner says what is happening.
Function un.onInit
  StrCpy $KeepHome 1
  StrCpy $KeepRelocated 1

  ; Every `mix` below inherits it: `nsExec` decodes what it captures in the ANSI code page, and a
  ; UTF-8 dash in the plan reached the log as three other characters (T182e).
  System::Call 'Kernel32::SetEnvironmentVariable(t "MIXENGINE_PLAIN_TEXT", t "1")'

  StrCpy $Relocated ""
  StrCpy $BannerUp 0
  ${IfNot} ${Silent}
    Banner::show /NOUNLOAD "Checking MixLab's folders..."
    StrCpy $BannerUp 1
  ${EndIf}
  nsExec::ExecToStack '"$INSTDIR\mix.exe" uninstall --dry-run --relocated'
  Pop $0
  Pop $1
  ${If} $0 == 0
    StrCpy $Relocated $1
  ${EndIf}
  ; The banner stays up until the window is in front of it — see un.ConfirmShow.
FunctionEnd

; **A larger window** — T182b. Every control below the page area moves down with the bottom edge,
; the buttons on the right move right with it, anything reaching within 20 pixels of the right edge
; is widened — the line beside the branding stops short of it — and the page area itself grows by
; the whole amount. Then the window grows about its centre. $0-$9 and $R3-$R8 are scratch; nothing
; is set yet this early.
Function un.onGUIInit
  System::Call "*(i 0, i 0, i ${GROW_X}, i ${GROW_Y}) p .r9"
  System::Call "user32::MapDialogRect(p $HWNDPARENT, p r9)"
  System::Call "*$9(i, i, i .R3, i .R4)"
  System::Free $9

  !insertmacro ClientSize $HWNDPARENT
  StrCpy $R5 $6
  IntOp $R8 $R5 / 2
  StrCpy $5 $HWNDPARENT
  GetDlgItem $0 $HWNDPARENT 1018
  !insertmacro RectIn
  StrCpy $R6 $4

  System::Call "user32::GetWindow(p $HWNDPARENT, i 5) p .r0"
  ${DoWhile} $0 != 0
    !insertmacro RectIn
    StrCpy $R7 $3
    IntOp $3 $3 - $1
    IntOp $4 $4 - $2
    IntOp $9 $R7 + 20
    System::Call "user32::GetDlgCtrlID(p r0) i .r8"
    ${If} $8 == 1018
      IntOp $3 $3 + $R3
      IntOp $4 $4 + $R4
    ${ElseIf} $2 >= $R6
      IntOp $2 $2 + $R4
      ${If} $1 > $R8
        IntOp $1 $1 + $R3
      ${ElseIf} $9 >= $R5
        IntOp $3 $3 + $R3
      ${EndIf}
    ${ElseIf} $9 >= $R5
      ${If} $1 > $R8
        IntOp $1 $1 + $R3
      ${Else}
        IntOp $3 $3 + $R3
      ${EndIf}
    ${EndIf}
    !insertmacro PlaceWindow
    System::Call "user32::GetWindow(p r0, i 2) p .r0"
  ${Loop}

  System::Call "*(i 0, i 0, i 0, i 0) p .r9"
  System::Call "user32::GetWindowRect(p $HWNDPARENT, p r9)"
  System::Call "*$9(i .r1, i .r2, i .r3, i .r4)"
  System::Free $9
  IntOp $3 $3 - $1
  IntOp $4 $4 - $2
  IntOp $3 $3 + $R3
  IntOp $4 $4 + $R4
  IntOp $R3 $R3 / 2
  IntOp $R4 $R4 / 2
  IntOp $1 $1 - $R3
  IntOp $2 $2 - $R4
  StrCpy $0 $HWNDPARENT
  !insertmacro PlaceWindow
FunctionEnd

; **In front, and only then the banner goes** — T182b, twice over. The banner from un.onInit is this
; process's window in the foreground, which is what lets this process put another window in front:
; destroyed first, as it was, the foreground passed to whatever was under it, and Windows then
; refused the window's own BringToFront — it opened behind other programs and had to be found on
; the taskbar. So the window is shown and brought forward while the banner still holds the
; foreground, and the banner goes after.
Function un.ConfirmShow
  !insertmacro FitPage
  ${If} $BannerUp == 1
    ShowWindow $HWNDPARENT ${SW_SHOW}
    BringToFront
    Banner::destroy
    StrCpy $BannerUp 0
  ${EndIf}
FunctionEnd

Function un.InstFilesShow
  !insertmacro FitPage
  !insertmacro FitLog
FunctionEnd

; The choices page — T182, D7. Two boxes, both unticked: keeping is what nobody regrets.
;
; The second box exists only when `[paths]` moved something out of the home, and it lists what. If
; `mix` cannot answer, the page offers the home alone and the checks that open the uninstall say why nothing
; can be removed.
Function un.ChoicesPage
  ; $Relocated was read in un.onInit, before any page existed.
  nsDialogs::Create 1018
  Pop $0

  ${NSD_CreateCheckbox} 0 0 100% 12u "Also delete MixLab's data"
  Pop $HomeBox
  ; The window's saved connections and history go with the data (T182b); its cache always goes.
  !ifdef HEADLESS
    ${NSD_CreateLabel} 12u 14u -12u 36u "$LOCALAPPDATA\MixEngine$\r$\nYour databases, certificates and project records. Leave this unticked to keep them for a later install."
  !else
    ${NSD_CreateLabel} 12u 14u -12u 36u "$LOCALAPPDATA\MixEngine$\r$\n$APPDATA\io.github.mixnz.mixlab$\r$\nYour databases, certificates and project records, and MixLab's saved connections and history. Leave this unticked to keep them for a later install."
  !endif
  Pop $0

  ${If} $Relocated != ""
    ${NSD_CreateCheckbox} 0 58u 100% 12u "Also delete the folders you moved out of it"
    Pop $RelocatedBox
    ${NSD_CreateLabel} 12u 72u -12u 60u "$Relocated"
    Pop $0
  ${EndIf}

  nsDialogs::Show
FunctionEnd

; Read the boxes, then check. **Checked here, on the page**: a failure keeps the person on it, so
; they close what was named and click Uninstall again. Nothing has been changed.
Function un.ChoicesLeave
  StrCpy $KeepHome 1
  ${NSD_GetState} $HomeBox $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $KeepHome 0
  ${EndIf}

  StrCpy $KeepRelocated 1
  ${If} $Relocated != ""
    ${NSD_GetState} $RelocatedBox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $KeepRelocated 0
    ${EndIf}
  ${EndIf}

  ; **Nothing slow here** — T182b. This runs on the window's own thread, so the checks it used to
  ; make froze the page for a second after Uninstall was pressed. They run first thing in the
  ; section instead, with the progress bar moving, and still before anything is changed.
FunctionEnd

; The flags both `mix uninstall` calls take, in $R1, each with its leading space.
Function un.Flags
  StrCpy $R1 ""
  ${If} $KeepHome == 1
    StrCpy $R1 "$R1 --keep-home"
  ${EndIf}
  ${If} $KeepRelocated == 1
    StrCpy $R1 "$R1 --keep-relocated"
  ${EndIf}
FunctionEnd

; What is in the way of the choices made, into $InUse: one program per line with Windows line
; endings, and empty when nothing is — T182e, D6. A `mix` that cannot answer leaves it empty too:
; the checks at the start of the uninstall ask again and say why. $0-$5 are scratch.
Function un.FindInUse
  Call un.Flags
  nsExec::ExecToStack '"$INSTDIR\mix.exe" uninstall --dry-run --blocked$R1'
  Pop $0
  Pop $1
  StrCpy $InUse ""
  ${If} $0 != 0
    Return
  ${EndIf}

  ; `mix` ends its lines with LF alone, which a Windows label draws as one long line.
  StrCpy $2 ""
  StrCpy $3 0
  ${Do}
    StrCpy $4 $1 1 $3
    ${If} $4 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $4 == "$\n"
      StrCpy $2 "$2$\r$\n"
    ${ElseIf} $4 != "$\r"
      StrCpy $2 "$2$4"
    ${EndIf}
    IntOp $3 $3 + 1
  ${Loop}

  ; And not a trailing line break, so a listing of nothing is an empty string.
  ${Do}
    StrCpy $5 $2 2 -2
    ${If} $5 != "$\r$\n"
      ${ExitDo}
    ${EndIf}
    StrCpy $2 $2 -2
  ${Loop}
  StrCpy $InUse $2
FunctionEnd

; The page that asks for them to be closed — T182e, D6. **Skipped when nothing is in the way**, which
; is the ordinary case: the choices lead straight to the progress page. Asked behind a banner, since
; reading what other programs hold takes a few seconds and the window would otherwise stand still.
Function un.InUsePage
  Banner::show /NOUNLOAD "Checking what is in use..."
  Call un.FindInUse
  Banner::destroy
  ${If} $InUse == ""
    Abort
  ${EndIf}

  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateLabel} 0 0 100% 30u "These programs are using MixLab's folders in a way that stops them being removed. Close them, then click Check again. When the list is clear, click Uninstall."
  Pop $0
  ${NSD_CreateLabel} 12u 34u -12u -56u "$InUse"
  Pop $InUseList
  ${NSD_CreateButton} 0 -18u 80u 15u "Check again"
  Pop $0
  ${NSD_OnClick} $0 un.InUseCheckAgain
  nsDialogs::Show
FunctionEnd

; Check again: ask once more and rewrite the list. Uninstall stays available either way: something
; still in the way is caught by the checks at the start of the uninstall, with Retry.
Function un.InUseCheckAgain
  Pop $0
  ${NSD_SetText} $InUseList "Checking..."
  Call un.FindInUse
  ${If} $InUse == ""
    ${NSD_SetText} $InUseList "Nothing is in the way now. Click Uninstall to go on."
  ${Else}
    ${NSD_SetText} $InUseList "$InUse"
  ${EndIf}
FunctionEnd

; Everything that could stop the uninstall half-way, found while nothing has changed — T182, P2.
; Leaves 1 in $R0 to go on, and 0 to stop; when it stops for something a person can close, what to
; close is in $R3, and an empty $R3 is a person who chose to stop.
Function un.Checks
  StrCpy $R3 ""
  Call un.CloseMixLab
  ${If} $R0 == 0
    Return
  ${EndIf}

  ; `mixengined.exe` is not checked: `mix uninstall` ends the daemon and waits for it. If it is
  ; somehow still held afterwards, the checked delete below keeps the uninstaller, so it can run
  ; again (P1).
  StrCpy $Locked ""
  !insertmacro CheckWritable "$INSTDIR\mix.exe"
  !insertmacro CheckWritable "$INSTDIR\mixengine-shim.exe"
  !insertmacro CheckWritable "$INSTDIR\mixengine-trampoline.exe"
  !insertmacro CheckWritable "$INSTDIR\mixengine-elevate.exe"
  !insertmacro CheckWritable "$INSTDIR\mixlab.exe"
  ; The lock both updaters hold while they swap these files (T187) — held means an update is running,
  ; and an uninstall in the middle of one would race its swaps.
  !insertmacro CheckWritable "$INSTDIR\update.lock"
  ${If} $Locked != ""
    StrCpy $R3 "These files are in use. Close the programs using them, then click Retry.$\r$\n$Locked"
    StrCpy $R0 0
    Return
  ${EndIf}

  ; The plan, with the choices made. Exit 3 is a program running from a folder that would go, or
  ; holding something in one that cannot be moved (T182e).
  Call un.Flags
  nsExec::ExecToStack '"$INSTDIR\mix.exe" uninstall --dry-run$R1'
  Pop $0
  Pop $1
  ${If} $0 == 3
    StrCpy $R3 "Some programs are using MixLab's folders in a way that stops them being removed. Close them, then click Retry.$\r$\n$\r$\n$1"
    StrCpy $R0 0
    Return
  ${ElseIf} $0 != 0
    StrCpy $R3 "MixLab could not check what it would remove.$\r$\n$\r$\n$1"
    StrCpy $R0 0
    Return
  ${EndIf}

  StrCpy $R0 1
FunctionEnd

Section "Uninstall"
  ; **The checks, first, while nothing has changed** — T182, P2, and here rather than on the page
  ; since T182b so the page does not freeze. Something a person can close is offered again after they
  ; have; Cancel, or a person who chose to stop, ends here with nothing removed.
  DetailPrint "Checking what is in use."
  !insertmacro MarqueeOn
  checks:
  Call un.Checks
  ${If} $R0 == 0
    ${If} $R3 != ""
    ${AndIfNot} ${Silent}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$R3" IDRETRY checks
    ${EndIf}
    !insertmacro MarqueeOff
    DetailPrint "Nothing was removed."
    !insertmacro FitLog
    SetErrorLevel 2
    Abort
  ${EndIf}
  !insertmacro MarqueeOff

  ; **Always: what MixLab changed on this machine** — the hosts block, the resolver wiring, the
  ; certificate authority, the port grant, firewall rules, the login entry, the helper and its log,
  ; and whichever folders were not kept. One UAC prompt. A declined prompt changes nothing (T182,
  ; D5), and MixLab stays installed so this can run again.
  Call un.Flags
  DetailPrint "Undoing what MixLab changed on this machine."
  !insertmacro MarqueeOn
  nsExec::ExecToLog '"$INSTDIR\mix.exe" uninstall --yes$R1'
  Pop $0
  !insertmacro MarqueeOff
  !insertmacro FitLog
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "MixLab could not finish undoing its changes to this machine, so it is still installed. Run Uninstall again from Installed apps to finish. The details are in the log above." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}

  ; **The program: binaries first, and each one checked.** One `RemoveChecked` per `File` above, and
  ; the pairing is not decoration: a binary with no line here would stay on the machine for ever. If
  ; any stays, everything that makes MixLab findable and re-runnable stays too (P1).
  StrCpy $Stuck ""
  !insertmacro RemoveChecked "$INSTDIR\mix.exe"
  !insertmacro RemoveWithRetry "$INSTDIR\mixengined.exe"
  !insertmacro RemoveChecked "$INSTDIR\mixengine-shim.exe"
  !insertmacro RemoveChecked "$INSTDIR\mixengine-trampoline.exe"
  !insertmacro RemoveChecked "$INSTDIR\mixengine-elevate.exe"
  !insertmacro RemoveChecked "$INSTDIR\mixlab.exe"
  ; Not placed by a `File` above, but by the first update (T187): `mix self-update` and MixLab's
  ; updater leave it behind for the next one, and a directory still holding it is one `RMDir` below
  ; cannot remove — which left an install folder with nothing in it but this.
  !insertmacro RemoveChecked "$INSTDIR\update.lock"
  ${If} $Stuck != ""
    MessageBox MB_ICONSTOP "These files are still in use and were not removed. Close the programs using them, then run Uninstall again from Installed apps.$\r$\n$Stuck" /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}

  Call un.RemoveFromPath
  Call un.RemoveScheme

  ; Both shortcuts. `Delete` says nothing about a file that is not there, so the optional one needs
  ; no condition, and a condition would need the section state, which an uninstaller does not have.
  Delete "$SMPROGRAMS\MixLab.lnk"
  Delete "$DESKTOP\MixLab.lnk"

  ; **And Explorer is told** — T182b. A deleted shortcut stayed drawn on the desktop until somebody
  ; refreshed it. SHCNE_DELETE (0x4) with SHCNF_PATHW (0x5) for each, and SHCNE_ASSOCCHANGED
  ; (0x08000000) for the scheme and the icon removed above.
  System::Call 'shell32::SHChangeNotify(i 0x4, i 0x5, w "$DESKTOP\MixLab.lnk", p 0)'
  System::Call 'shell32::SHChangeNotify(i 0x4, i 0x5, w "$SMPROGRAMS\MixLab.lnk", p 0)'

  ; Last: the uninstaller and its entry in Installed apps.
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\MixEngine"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
