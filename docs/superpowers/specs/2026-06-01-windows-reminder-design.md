# Windows Rocket Reminder Desktop App Design

## Overview

Build a Windows desktop reminder application that runs primarily from the
system tray. Users create reminders with a time and content. Five minutes
before a reminder time, the app shows an unobtrusive rocket animation near the
top of the primary display. The rocket flies from left to right while pulling
a banner that contains the full reminder text.

The application is intentionally small and local-only. It has no account,
network service, or cloud synchronization.

The shipped interface uses Simplified Chinese. This document uses English
semantic label names to keep the specification encoding-independent.

## Confirmed Product Decisions

- Use Rust with Tauri and SQLite.
- Run as a single-instance tray application.
- Start automatically when the current user signs in to Windows.
- Keep the SQLite database in the selected installation directory.
- Default to a current-user writable installation directory so the app does
  not need administrator privileges during normal use.
- Package the result as a Windows `.exe` installer with installation-directory
  selection as the only installation option.
- Ask whether to delete or preserve the database during uninstall.

## Windows And Tray Behavior

The app starts in the tray without opening a taskbar window. The tray icon
supports:

- Right-click menu item `Reminders` to open the centered reminder list window.
- Right-click menu item `Exit` to request confirmation before ending the
  current process.
- Double-click to open the reminder list window.

Exiting the current process does not disable automatic startup. Closing the
list window or add-reminder dialog hides that window while the tray process
continues running.

## Windows

### Reminder List

The centered list window uses a compact table layout. It defaults to pending
records only and provides a switch to show all records.

Columns:

| Column | Meaning |
| --- | --- |
| Content | Reminder text |
| Time | Scheduled local time |
| Repeat | None, daily, or weekly |
| Status | Pending, completed, or missed |
| Action | Per-row delete action |

Completed and missed records use gray text. Every row has a delete button.
Deletion requires confirmation. A plus button in the lower-right corner opens
the add-reminder dialog.

### Add Reminder Dialog

The dialog contains:

- A time input in `HH:mm` format.
- A reminder-content input.
- A repeat switch.
- Mutually exclusive `Daily` and `Weekly` repeat choices shown when repetition is
  enabled.
- A save button.

Turning on one repeat choice turns off the other. Turning off both repeat
choices automatically turns off the repeat switch.

Only today's time is entered. If the selected time has already passed, saving
is blocked with a validation message. If the selected time is still in the
future but fewer than five minutes remain, saving succeeds and the rocket
animation starts immediately.

Weekly reminders repeat on the weekday on which the reminder was created.

### Rocket Banner Window

The animation uses a dedicated transparent, borderless, always-on-top window.
It appears around the upper 20 percent of the primary display, does not appear
in the taskbar, does not steal focus, and allows mouse clicks to pass through.

The animation lasts about eight seconds:

1. The rocket and its full banner start outside the left side of the primary
   display.
2. The rocket flies to the right while physically pulling the banner from its
   tail.
3. The banner remains connected to the rocket and moves as one unit until the
   entire composition leaves the right side of the display.
4. A moving texture or highlight inside the banner creates a continuous
   flowing effect while the banner is being pulled.
5. The transparent animation window closes after the entire composition has
   disappeared.

The banner width follows the full rendered reminder text width. Long text is
not truncated and the font is not automatically reduced.

Only the primary display shows the animation, even when multiple displays are
connected.

## Data Model

The SQLite database file is named `reminders.db` and is stored beside the
installed executable.

The core reminder table contains:

| Field | Purpose |
| --- | --- |
| `id` | Unique record identifier |
| `content` | Reminder text |
| `scheduled_at` | Concrete local date and time for this occurrence |
| `repeat_type` | `none`, `daily`, or `weekly` |
| `status` | `pending`, `completed`, or `missed` |
| `created_at` | Record creation time |
| `completed_at` | Completion or missed-processing time, nullable while pending |
| `source_reminder_id` | Previous occurrence identifier for derived repeats |

Each repeated occurrence is stored as a separate record. This preserves
history and allows users to delete individual occurrences. Enforce uniqueness
for each derived record's `source_reminder_id` so retries cannot generate the
same next occurrence twice.

## Scheduling And State Transitions

For every pending reminder:

- `trigger_at = scheduled_at - 5 minutes`.
- The scheduler periodically scans pending records.
- A record can have at most one active animation instance.

### Normal Execution

When the process is running, the session is unlocked, and `trigger_at` is
reached:

1. Start the rocket banner animation.
2. Wait until the animation fully exits after about eight seconds.
3. Mark the record as `completed`.
4. For a daily or weekly record, immediately create the next occurrence.

### Immediate Execution For A Newly Added Reminder

When saving a reminder with a future `scheduled_at` but an already-reached
`trigger_at`, start the animation immediately. Completion and repeat
generation then follow the normal execution flow.

### Startup, Wake, And Lock Handling

The app checks pending records at startup and after the computer resumes from
sleep.

- If `scheduled_at` is already past, mark the record as `missed` without
  showing an animation.
- If `trigger_at` is past but `scheduled_at` is still in the future and the
  session is unlocked, show the animation immediately.
- If Windows is locked when a reminder reaches `trigger_at`, mark the record
  as `missed` without showing an animation.
- If Windows becomes locked while the eight-second animation is running, close
  the animation immediately and mark the record as `missed`.
- Unlocking Windows does not replay a record already marked as `missed`.
- For every missed daily or weekly record, immediately create the next
  occurrence.

When the app has not run for multiple repeat intervals, process each past
occurrence as `missed` in chronological order until a future occurrence is
created. This preserves the full occurrence history without attempting to
play stale animations.

The Rust backend should listen for Windows session lock, unlock, suspend, and
resume notifications and also verify lock state before starting an animation.

## Error Handling

- Show a clear error if the database cannot be created, opened, or written.
- Reject empty content, invalid `HH:mm` input, and times that have already
  passed today.
- Prevent duplicate animation execution for the same record when scans
  overlap.
- Use transactions when completing or missing a repeated record and creating
  its next occurrence.
- Require confirmation before deleting any record.
- Require confirmation before exiting from the tray menu.

## Packaging

Use the Tauri Windows installer pipeline to produce an `.exe` installer.
Configure it to:

- Default to a current-user writable installation directory.
- Allow the user to choose a different installation directory.
- Avoid unrelated installer choices.
- Register current-user automatic startup.
- Ask during uninstall whether `reminders.db` should be retained or deleted.

Use a custom NSIS uninstall hook for the database-retention prompt.

If the user chooses to retain the database, reinstalling into the same
directory restores the reminder history.

## Verification Plan

### Reminder Input And List

- Create non-repeating, daily, and weekly reminders.
- Validate empty content, invalid time text, and already-passed times.
- Confirm immediate animation when fewer than five minutes remain.
- Confirm repeat-choice exclusivity and automatic repeat-switch shutdown.
- Confirm pending-only default view and all-records switch.
- Confirm gray completed and missed records.
- Confirm deletion requires approval and works for every status.

### Scheduler And Recovery

- Confirm normal completion only after the animation exits.
- Confirm a repeated completion immediately creates the next occurrence.
- Confirm startup and resume mark past occurrences as missed without
  animation.
- Confirm a missed repeated occurrence immediately creates the next
  occurrence.
- Confirm locked-session triggers become missed without animation.
- Confirm locking Windows during an active animation closes it and records a
  missed occurrence.
- Confirm a long offline period creates each missed repeated occurrence and
  stops catch-up after creating a future occurrence.
- Confirm overlapping scheduler scans do not duplicate animations or derived
  records.

### Animation

- Confirm the animation window is transparent, always on top, click-through,
  focus-free, and absent from the taskbar.
- Confirm the primary-display position and approximately eight-second flight.
- Confirm the banner stays attached to the rocket tail throughout the flight.
- Confirm the banner's internal flowing effect continues until it exits.
- Confirm long banner text is rendered in full.

### Windows Integration And Installer

- Confirm tray menu actions, tray-icon double-click, hidden close behavior,
  and exit confirmation.
- Confirm automatic startup remains enabled after a manual process exit.
- Confirm custom installation-directory selection.
- Confirm database writes succeed without administrator privileges in the
  default directory.
- Confirm uninstall asks whether to preserve the database and both choices
  work as expected.
