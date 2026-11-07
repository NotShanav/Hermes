# Milestone 1: Windows foreground tracking

Scope: foreground application detection, process identity, window titles, timestamps, switching events,
session boundaries, closed/inaccessible windows. **No typed-text collection** (milestone 3).

## Architecture
* `hermes-tracker::engine`: OS-independent state machine (all rules unit-tested on any OS).
* `hermes-tracker::win`: Win32 backend. One thread, one message loop (`GetMessageW` blocks: 0 CPU when idle).
* `hermes-collector`: console build wiring tracker → `EventStore` (encrypt with public key, sign, batch).
  Tray UI is milestone 5.
* `hermes-dump`: verification tool that decrypts with BOTH shares. Not the Viewer.

## Windows APIs used (and nothing else)
`SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_NAMECHANGE`, `EVENT_OBJECT_DESTROY`) with
`WINEVENT_OUTOFCONTEXT` (events delivered to our own thread; **no DLL injection into other processes**),
`GetForegroundWindow`, `GetWindowThreadProcessId`, `GetWindowTextW`, `GetClassNameW`, `OpenProcess(
PROCESS_QUERY_LIMITED_INFORMATION)`, `QueryFullProcessImageNameW`, `GetProcessTimes`,
`WTSRegisterSessionNotification` (lock/unlock/logoff/connect/disconnect), `WM_POWERBROADCAST` (suspend/resume),
`WM_ENDSESSION`, `GetSystemTimePreciseAsFileTime`, `SetConsoleCtrlHandler`. No keyboard/mouse hooks, no polling,
no screen capture, no accessibility text reading.

## Events (kind numbers are encrypted; body is JSON `{"v":1,"mono_us":...,"t":...}`)
| Event | Meaning |
|---|---|
| `SESSION_START` reason | collector_start, unlock, resume, resume_collection, reconnect |
| `SESSION_END` reason | collector_stop, lock, sleep, pause, logoff, disconnect |
| `FOCUS_GAINED` | app identity (pid, process start time, path, name, status), window (hwnd, class, title, status), `from_hwnd` |
| `FOCUS_LOST` | reason switched / no_foreground_window / window_closed / session_end; hwnd, pid, held time (monotonic) |
| `TITLE_CHANGED` | new title (coalesced 500 ms, at most 1/s) |
| storage-layer: `SYSTEM_GAP`, `SYSTEM_RECOVERED` | dropped events; unclean shutdown/repair marker |

Every switch is a `FOCUS_LOST` + `FOCUS_GAINED` pair with consecutive sequence numbers. Multiple windows of
one application are distinct targets (hwnd + pid + process start time). A reused PID/HWND is not confused with
the old process.

## Timestamps
`ts_us` = `GetSystemTimePreciseAsFileTime` (100 ns resolution) converted to Unix microseconds, plain column.
`mono_us` (encrypted) = monotonic microseconds since collector start (`Instant`/QPC) for ordering and durations
that survive clock changes; `focus_us` uses it. `seq` is the authoritative order.

## Session semantics
Collection state = {paused, locked, asleep, disconnected, stopped}. A session is open only while none is set.
The first cause ends the session; the session restarts only when **all** causes are cleared (pause + lock +
unlock stays paused). Events arriving while inactive are ignored. Crash = no `SESSION_END`; the next start
writes a `SYSTEM_RECOVERED` event with the last durable sequence number, and the viewer infers the end.

## Closed / inaccessible windows (honest recording, nothing invented)
* Window destroyed or vanishing before it can be read → `FOCUS_LOST(window_closed)`; a destroyed window that the
  OS still reports as foreground for a moment is ignored (regression-tested; found via the real-hook test).
* No foreground window (desktop switch, UAC / Ctrl+Alt+Del secure desktop, transition) → `FOCUS_LOST(no_foreground_window)`.
  Hooks do not fire on the secure desktop; Hermes records that it cannot see, it does not guess.
* Process cannot be opened (protected/system/elevated against a non-elevated collector) → `status:
  access_denied`, no executable name; the window title is still recorded if readable.
* Packaged (UWP) apps: the window belongs to `ApplicationFrameHost.exe`; the real process is resolved through the
  child window and the host is noted in `host_exe`.

## Privacy controls in this milestone
* Application exclusions (case-insensitive exe name). Excluded apps are logged only as `status: excluded` with
  no name, path, class or title (so the timeline has no holes but reveals nothing). Default list: KeePass,
  KeePassXC, 1Password, Bitwarden, LastPass, Dashlane, Enpass, LogonUI, consent.exe, CredentialUIBroker,
  CredentialEnrollmentManager. Extend with `--exclude name.exe`; disable defaults with `--no-default-exclusions`.
  This is best effort: it matches process names, not password *fields* (no text is collected yet).
* The visible recording indicator and tray pause control are milestone 5. Today: it is a console program you start
  yourself, it prints a banner, Ctrl+C stops it. There is no hidden mode and no persistence/auto-start.

## Known limitations
* Titles can contain sensitive text (document names, URLs, chat subjects) in apps that are not excluded.
* Windows that never take the foreground (overlays, tool windows, background tabs) are not tracked.
* Multiple monitors/virtual desktops: only the single system foreground window is tracked.
* `EVENT_OBJECT_NAMECHANGE` is subscribed globally and filtered in the callback; a title-spamming background
  process costs a callback per event (cheap, but not zero).
* Real Windows hardware has not been used yet by the author: see docs/STATUS.md.
