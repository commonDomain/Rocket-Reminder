# Windows Rocket Reminder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Windows tray reminder application that stores reminders beside the executable, plays a click-through rocket-and-banner animation five minutes before each reminder, and ships as a selectable-directory NSIS `.exe` installer.

**Architecture:** Use a Tauri 2 shell with a Vanilla TypeScript/Vite multi-page frontend and a Rust backend. Keep time rules, SQLite operations, scheduler decisions, and animation queue behavior in testable Rust modules; isolate Win32 session and power notifications behind a small adapter. Use three Tauri windows: a centered reminder list, a centered add-reminder dialog, and a transparent primary-monitor animation overlay.

**Tech Stack:** Rust stable, Tauri 2, `rusqlite` with bundled SQLite, `chrono`, `windows-sys`, Vanilla TypeScript, Vite, Vitest, jsdom, NSIS, Tauri autostart plugin, Tauri single-instance plugin.

---

## File Map

| Path | Responsibility |
| --- | --- |
| `package.json` | Frontend, test, Tauri, and packaging scripts |
| `vite.config.ts` | Vite multi-page build for list, add, and animation pages |
| `vitest.config.ts` | jsdom unit-test configuration |
| `tsconfig.json` | TypeScript compiler configuration |
| `index.html` | Reminder-list entry page |
| `add.html` | Add-reminder entry page |
| `animation.html` | Rocket-overlay entry page |
| `src/shared/types.ts` | Shared frontend DTOs |
| `src/list.ts` | Reminder-table loading, filtering, delete confirmation, plus button |
| `src/add.ts` | Form state, repeat-choice rules, validation messages, save command |
| `src/animation.ts` | Rocket payload listener and eight-second animation lifecycle |
| `src/styles/base.css` | Shared typography and controls |
| `src/styles/list.css` | Compact table-window styles |
| `src/styles/add.css` | Small centered add-dialog styles |
| `src/styles/animation.css` | Rocket, attached banner, flowing highlight, overlay styles |
| `src/**/*.test.ts` | Frontend behavior tests |
| `src-tauri/Cargo.toml` | Rust dependencies and Windows API features |
| `src-tauri/build.rs` | Tauri build hook |
| `src-tauri/tauri.conf.json` | Window, bundle, NSIS, and application configuration |
| `src-tauri/capabilities/default.json` | Minimum Tauri frontend permissions |
| `src-tauri/icons/icon.svg` | Source rocket icon |
| `src-tauri/windows/hooks.nsh` | NSIS uninstall database-retention prompt |
| `src-tauri/src/main.rs` | Binary entry point |
| `src-tauri/src/lib.rs` | App composition and managed state |
| `src-tauri/src/model.rs` | Reminder types, validation, and recurrence calculation |
| `src-tauri/src/db.rs` | SQLite schema and repository transactions |
| `src-tauri/src/scheduler.rs` | Scheduler decisions, catch-up, deduplicated animation queue |
| `src-tauri/src/system_monitor.rs` | Win32 lock, unlock, suspend, and resume adapter |
| `src-tauri/src/animation.rs` | Overlay-window placement, display, cancellation, and completion |
| `src-tauri/src/commands.rs` | Tauri commands exposed to the frontend |
| `src-tauri/src/tray.rs` | Tray menu, double-click, hidden-close behavior, exit confirmation |

## Task 1: Install Prerequisites And Scaffold The Tauri Project

**Files:**
- Create: `package.json`
- Create: `vite.config.ts`
- Create: `vitest.config.ts`
- Create: `tsconfig.json`
- Create: `index.html`
- Create: `add.html`
- Create: `animation.html`
- Modify: `.gitignore`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/icons/icon.svg`

- [ ] **Step 1: Install the missing Rust and MSVC prerequisites**

Run from an elevated-capable terminal:

```powershell
winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" --accept-package-agreements --accept-source-agreements
```

Open a fresh shell and verify:

```powershell
rustc --version
cargo --version
```

Expected: both commands print stable tool versions.

- [ ] **Step 2: Create the frontend package manifest**

Create `package.json` with:

```json
{
  "name": "rocket-reminder",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "test": "vitest run",
    "test:watch": "vitest",
    "tauri": "tauri",
    "tauri:dev": "tauri dev",
    "tauri:build": "tauri build"
  },
  "dependencies": {
    "@tauri-apps/api": "^2"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@types/node": "^22",
    "jsdom": "^26",
    "typescript": "^5",
    "vite": "^6",
    "vitest": "^3"
  }
}
```

Install with the `.cmd` shim because this machine blocks `npm.ps1`:

```powershell
& 'D:\program\nodejs\npm.cmd' install
```

Expected: `package-lock.json` and `node_modules/` are created.

- [ ] **Step 3: Create the Rust manifest**

Create `src-tauri/Cargo.toml` with:

```toml
[package]
name = "rocket-reminder"
version = "0.1.0"
description = "Windows tray reminder with rocket banner animation"
edition = "2021"

[lib]
name = "rocket_reminder_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
chrono = { version = "0.4", features = ["serde"] }
rusqlite = { version = "0.37", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-autostart = "2"
tauri-plugin-single-instance = "2"
thiserror = "2"
windows-sys = { version = "0.60", features = [
  "Win32_Foundation",
  "Win32_System_LibraryLoader",
  "Win32_System_Power",
  "Win32_System_RemoteDesktop",
  "Win32_System_StationsAndDesktops",
  "Win32_UI_WindowsAndMessaging"
] }
```

Create `src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

Create `src-tauri/src/main.rs`:

```rust
fn main() {
    rocket_reminder_lib::run();
}
```

Create `src-tauri/src/lib.rs`:

```rust
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run rocket reminder");
}
```

- [ ] **Step 4: Create the Vite and Tauri configuration**

Add `node_modules/` to `.gitignore`.

Create `vite.config.ts`:

```ts
import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    rollupOptions: {
      input: {
        reminders: resolve(__dirname, "index.html"),
        addReminder: resolve(__dirname, "add.html"),
        animation: resolve(__dirname, "animation.html"),
      },
    },
  },
});
```

Create `vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
  },
});
```

Create `tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": false,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "strict": true,
    "types": ["node"]
  },
  "include": ["src", "vite.config.ts", "vitest.config.ts"]
}
```

Create minimal buildable HTML entry points before behavior modules are added:

```html
<!doctype html>
<html lang="zh-CN">
  <head><meta charset="UTF-8" /><meta name="viewport" content="width=device-width, initial-scale=1.0" /></head>
  <body></body>
</html>
```

Use that content for `index.html`, `add.html`, and `animation.html`.

Create `src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Core capability for the three local reminder windows",
  "windows": ["reminders", "add-reminder", "rocket-banner"],
  "permissions": ["core:default"]
}
```

Create a simple square `src-tauri/icons/icon.svg` rocket image:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <rect width="256" height="256" rx="56" fill="#2563eb"/>
  <path d="M83 164c23-65 59-96 113-104-7 55-39 91-104 113l-9-9Z" fill="#fff"/>
  <circle cx="155" cy="101" r="18" fill="#60a5fa"/>
  <path d="M88 164 58 194l5-38 20-12 5 20Zm8 8 20 5-12 20-38 5 30-30Z" fill="#fbbf24"/>
</svg>
```

Generate the required Windows icon before Rust compilation:

```powershell
& 'D:\program\nodejs\npx.cmd' tauri icon src-tauri/icons/icon.svg
```

Expected: `src-tauri/icons/icon.ico` is created.

Create a Tauri config with:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Rocket Reminder",
  "version": "0.1.0",
  "identifier": "com.local.rocket-reminder",
  "build": {
    "beforeDevCommand": "npm.cmd run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm.cmd run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "reminders",
        "title": "提醒事项",
        "url": "index.html",
        "width": 760,
        "height": 520,
        "visible": false,
        "center": true,
        "skipTaskbar": true
      },
      {
        "label": "add-reminder",
        "title": "新增提醒事项",
        "url": "add.html",
        "width": 380,
        "height": 430,
        "visible": false,
        "center": true,
        "skipTaskbar": true,
        "resizable": false
      },
      {
        "label": "rocket-banner",
        "url": "animation.html",
        "visible": false,
        "transparent": true,
        "decorations": false,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "focus": false,
        "resizable": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico"],
    "windows": {
      "nsis": {
        "installMode": "currentUser",
        "installerHooks": "./windows/hooks.nsh"
      }
    }
  }
}
```

- [ ] **Step 5: Verify the scaffold**

Run:

```powershell
& 'D:\program\nodejs\npm.cmd' run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: the empty frontend build and Rust shell compile.

- [ ] **Step 6: Commit**

```powershell
git add .gitignore package.json package-lock.json vite.config.ts vitest.config.ts tsconfig.json index.html add.html animation.html src-tauri
git commit -m "chore: scaffold tauri reminder app"
```

## Task 2: Implement Reminder Validation And Recurrence Rules

**Files:**
- Create: `src-tauri/src/model.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing model tests**

Add tests in `src-tauri/src/model.rs` for:

```rust
#[test]
fn rejects_a_time_that_already_passed_today() {
    let now = local_dt(2026, 6, 1, 14, 0);
    assert_eq!(
        schedule_today("09:30", now).unwrap_err(),
        ValidationError::TimeAlreadyPassed
    );
}

#[test]
fn keeps_a_future_time_even_inside_the_five_minute_window() {
    let now = local_dt(2026, 6, 1, 9, 28);
    let scheduled = schedule_today("09:30", now).unwrap();
    assert_eq!(scheduled, local_dt(2026, 6, 1, 9, 30));
}

#[test]
fn weekly_occurrence_uses_the_same_weekday_and_time() {
    let scheduled = local_dt(2026, 6, 1, 9, 30);
    assert_eq!(
        next_occurrence(scheduled, RepeatType::Weekly),
        Some(local_dt(2026, 6, 8, 9, 30))
    );
}
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml model -- --nocapture
```

Expected: FAIL because `schedule_today`, `next_occurrence`, and domain types do
not exist.

- [ ] **Step 3: Implement the domain model**

Create:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatType { None, Daily, Weekly }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReminderStatus { Pending, Completed, Missed }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("事项内容不能为空")]
    EmptyContent,
    #[error("时间格式无效，请使用 HH:mm")]
    InvalidTime,
    #[error("提醒时间已经过去")]
    TimeAlreadyPassed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reminder {
    pub id: i64,
    pub content: String,
    pub scheduled_at: DateTime<Local>,
    pub repeat_type: RepeatType,
    pub status: ReminderStatus,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub source_reminder_id: Option<i64>,
}

pub fn schedule_today(input: &str, now: DateTime<Local>) -> Result<DateTime<Local>, ValidationError>;
pub fn next_occurrence(current: DateTime<Local>, repeat: RepeatType) -> Option<DateTime<Local>>;
pub fn trigger_at(reminder: &Reminder) -> DateTime<Local>;
```

Parse strictly with `%H:%M`, reject blank content separately, combine with
`now.date_naive()`, and use checked `Days::new(1)` or `Days::new(7)` recurrence.

- [ ] **Step 4: Run tests to verify GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml model -- --nocapture
```

Expected: all model tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/model.rs src-tauri/src/lib.rs
git commit -m "feat: add reminder time and recurrence rules"
```

## Task 3: Implement SQLite Storage And Transactional Repeat Derivation

**Files:**
- Create: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing repository tests**

Use an in-memory SQLite connection and add:

```rust
#[test]
fn completion_creates_exactly_one_next_daily_occurrence() {
    let repo = test_repo();
    let first = repo.insert_pending("喝水", dt(2026, 6, 1, 9, 30), RepeatType::Daily).unwrap();
    repo.mark_completed(first.id, dt(2026, 6, 1, 9, 25)).unwrap();
    repo.mark_completed(first.id, dt(2026, 6, 1, 9, 25)).unwrap();

    let all = repo.list(true).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].scheduled_at, dt(2026, 6, 2, 9, 30));
    assert_eq!(all[1].source_reminder_id, Some(first.id));
}

#[test]
fn missed_weekly_occurrence_still_creates_the_next_week() {
    let repo = test_repo();
    let first = repo.insert_pending("提交周报", dt(2026, 6, 1, 17, 30), RepeatType::Weekly).unwrap();
    repo.mark_missed(first.id, dt(2026, 6, 1, 17, 31)).unwrap();
    assert_eq!(repo.list(false).unwrap()[0].scheduled_at, dt(2026, 6, 8, 17, 30));
}
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml db -- --nocapture
```

Expected: FAIL because `ReminderRepository` does not exist.

- [ ] **Step 3: Add schema and repository**

Create the table:

```sql
CREATE TABLE IF NOT EXISTS reminders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  content TEXT NOT NULL,
  scheduled_at TEXT NOT NULL,
  repeat_type TEXT NOT NULL CHECK (repeat_type IN ('none', 'daily', 'weekly')),
  status TEXT NOT NULL CHECK (status IN ('pending', 'completed', 'missed')),
  created_at TEXT NOT NULL,
  completed_at TEXT,
  source_reminder_id INTEGER UNIQUE REFERENCES reminders(id)
);
CREATE INDEX IF NOT EXISTS idx_reminders_pending_time
  ON reminders(status, scheduled_at);
```

Implement:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("日期时间格式错误: {0}")]
    DateTime(#[from] chrono::ParseError),
}

pub struct ReminderRepository {
    conn: Mutex<Connection>,
}

impl ReminderRepository {
    pub fn open(path: &Path) -> Result<Self, AppError>;
    pub fn in_memory() -> Result<Self, AppError>;
    pub fn insert_pending(&self, content: &str, scheduled_at: DateTime<Local>, repeat_type: RepeatType) -> Result<Reminder, AppError>;
    pub fn list(&self, include_history: bool) -> Result<Vec<Reminder>, AppError>;
    pub fn delete(&self, id: i64) -> Result<(), AppError>;
    pub fn pending(&self) -> Result<Vec<Reminder>, AppError>;
    pub fn mark_completed(&self, id: i64, at: DateTime<Local>) -> Result<Option<Reminder>, AppError>;
    pub fn mark_missed(&self, id: i64, at: DateTime<Local>) -> Result<Option<Reminder>, AppError>;
}
```

Both state-transition methods must run `UPDATE` plus `INSERT OR IGNORE` for the
next repeated occurrence in one transaction. Only derive when the previous
record changed from `pending`.

- [ ] **Step 4: Run tests to verify GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml db -- --nocapture
```

Expected: repository tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/db.rs src-tauri/src/lib.rs
git commit -m "feat: persist reminders in sqlite"
```

## Task 4: Implement Scheduler Decisions, Catch-Up, And Animation Queue

**Files:**
- Create: `src-tauri/src/scheduler.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing scheduler tests**

Add pure decision and queue tests:

```rust
#[test]
fn unlocked_record_inside_trigger_window_animates() {
    assert_eq!(decide(&pending_at(9, 30), dt(2026, 6, 1, 9, 28), false), Decision::Animate);
}

#[test]
fn locked_record_inside_trigger_window_is_missed() {
    assert_eq!(decide(&pending_at(9, 30), dt(2026, 6, 1, 9, 25), true), Decision::Miss);
}

#[test]
fn record_after_scheduled_time_is_missed_even_when_unlocked() {
    assert_eq!(decide(&pending_at(9, 30), dt(2026, 6, 1, 9, 31), false), Decision::Miss);
}

#[test]
fn queue_deduplicates_the_same_record() {
    let mut queue = AnimationQueue::default();
    assert!(queue.enqueue(7));
    assert!(!queue.enqueue(7));
    assert_eq!(queue.start_next(), Some(7));
}
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml scheduler -- --nocapture
```

Expected: FAIL because the scheduler types are missing.

- [ ] **Step 3: Implement scheduler primitives**

Create:

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum Decision { Wait, Animate, Miss }

pub fn decide(reminder: &Reminder, now: DateTime<Local>, locked: bool) -> Decision {
    if reminder.scheduled_at <= now {
        Decision::Miss
    } else if trigger_at(reminder) <= now {
        if locked { Decision::Miss } else { Decision::Animate }
    } else {
        Decision::Wait
    }
}

#[derive(Default)]
pub struct AnimationQueue {
    queued: VecDeque<i64>,
    claimed: HashSet<i64>,
    active: Option<i64>,
}
```

Add `enqueue`, `start_next`, `complete_active`, and `drain_claimed` methods.
Add a `SchedulerService::tick(now, locked)` loop that repeatedly marks missed
records and derives repeats until stable, then enqueues due animations. This
loop is what converts a multi-day offline gap into chronological missed
history and one future pending record.

- [ ] **Step 4: Add a catch-up integration test**

```rust
#[test]
fn catch_up_marks_each_past_daily_occurrence_and_stops_at_the_future() {
    let repo = Arc::new(test_repo());
    repo.insert_pending("喝水", dt(2026, 5, 29, 9, 30), RepeatType::Daily).unwrap();
    let scheduler = SchedulerService::new(repo.clone());
    scheduler.tick(dt(2026, 6, 1, 8, 0), false).unwrap();

    let all = repo.list(true).unwrap();
    assert_eq!(statuses(&all), vec![Missed, Missed, Missed, Pending]);
    assert_eq!(all.last().unwrap().scheduled_at, dt(2026, 6, 1, 9, 30));
}
```

- [ ] **Step 5: Run tests to verify GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml scheduler -- --nocapture
```

Expected: scheduler and catch-up tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/scheduler.rs src-tauri/src/lib.rs
git commit -m "feat: schedule reminders and catch up missed repeats"
```

## Task 5: Implement Frontend Form Rules And Compact Reminder Table

**Files:**
- Create: `src/shared/types.ts`
- Create: `src/add.ts`
- Create: `src/add.test.ts`
- Create: `src/list.ts`
- Create: `src/list.test.ts`
- Create: `src/styles/base.css`
- Create: `src/styles/add.css`
- Create: `src/styles/list.css`
- Modify: `index.html`
- Modify: `add.html`

- [ ] **Step 1: Write failing form-state tests**

Create:

```ts
import { describe, expect, it } from "vitest";
import { RepeatState } from "./add";

describe("RepeatState", () => {
  it("selecting daily turns weekly off", () => {
    const state = new RepeatState();
    state.toggleWeekly(true);
    state.toggleDaily(true);
    expect(state.value()).toEqual({ enabled: true, type: "daily" });
  });

  it("turning both choices off disables repetition", () => {
    const state = new RepeatState();
    state.toggleDaily(true);
    state.toggleDaily(false);
    expect(state.value()).toEqual({ enabled: false, type: "none" });
  });
});
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
& 'D:\program\nodejs\npm.cmd' test -- src/add.test.ts
```

Expected: FAIL because `RepeatState` does not exist.

- [ ] **Step 3: Implement form state and add-dialog wiring**

Create `src/shared/types.ts`:

```ts
export type RepeatType = "none" | "daily" | "weekly";
export type ReminderStatus = "pending" | "completed" | "missed";

export interface ReminderDto {
  id: number;
  content: string;
  scheduled_at: string;
  repeat_type: RepeatType;
  status: ReminderStatus;
}
```

Implement:

```ts
export type RepeatType = "none" | "daily" | "weekly";

export class RepeatState {
  private selected: RepeatType = "none";
  toggleEnabled(enabled: boolean) { if (!enabled) this.selected = "none"; }
  toggleDaily(enabled: boolean) { this.selected = enabled ? "daily" : "none"; }
  toggleWeekly(enabled: boolean) { this.selected = enabled ? "weekly" : "none"; }
  value() { return { enabled: this.selected !== "none", type: this.selected }; }
}
```

Build `add.html` with:

```html
<main class="add-card">
  <h1>新增提醒事项</h1>
  <label>提醒时间<input id="time" type="time" required /></label>
  <label>事项内容<textarea id="content" required></textarea></label>
  <label class="switch-row"><span>重复提醒</span><input id="repeat-enabled" type="checkbox" /></label>
  <div id="repeat-options" hidden>
    <label><input id="daily" type="checkbox" />每日</label>
    <label><input id="weekly" type="checkbox" />每周</label>
  </div>
  <p id="error" role="alert"></p>
  <button id="save" type="button">保存</button>
</main>
<script type="module" src="/src/add.ts"></script>
```

Wire it to `invoke("create_reminder", { input })`, render Chinese validation
errors, and invoke `hide_add_window` after a successful save.

- [ ] **Step 4: Write failing list rendering tests**

Test `renderRows` and `filterRows`:

```ts
it("defaults to pending rows and grays history rows when all is selected", () => {
  const rows = [
    reminder(1, "pending"),
    reminder(2, "completed"),
    reminder(3, "missed"),
  ];
  expect(filterRows(rows, false).map((row) => row.id)).toEqual([1]);
  document.body.innerHTML = "<tbody id='reminder-rows'></tbody>";
  renderRows(rows, () => {});
  expect(document.querySelectorAll(".history-row")).toHaveLength(2);
});

it("confirms before deleting a row", async () => {
  const deleted: number[] = [];
  await confirmAndDelete(9, () => true, async (id) => deleted.push(id));
  expect(deleted).toEqual([9]);
});
```

- [ ] **Step 5: Run list tests to verify RED**

```powershell
& 'D:\program\nodejs\npm.cmd' test -- src/list.test.ts
```

Expected: FAIL because list rendering helpers do not exist.

- [ ] **Step 6: Implement the compact list window**

Build `index.html` with:

```html
<main class="list-shell">
  <header><h1>提醒事项</h1></header>
  <nav class="filters">
    <button id="pending-filter" class="active">未完成</button>
    <button id="all-filter">全部</button>
  </nav>
  <table>
    <thead><tr><th>内容</th><th>时间</th><th>重复</th><th>状态</th><th>操作</th></tr></thead>
    <tbody id="reminder-rows"></tbody>
  </table>
  <button id="add-reminder" class="floating-add" aria-label="新增提醒事项">+</button>
</main>
<script type="module" src="/src/list.ts"></script>
```

Implement `renderRows`, `filterRows`, `bindDelete`, list refresh after
`reminders-changed`, the pending/all segmented switch, and the lower-right
plus button. Invoke `show_add_window` from the plus button. Use the confirmed
five columns: content, time, repeat, status, and action.

- [ ] **Step 7: Run frontend tests and build**

```powershell
& 'D:\program\nodejs\npm.cmd' test
& 'D:\program\nodejs\npm.cmd' run build
```

Expected: frontend tests and Vite build pass.

- [ ] **Step 8: Commit**

```powershell
git add index.html add.html src package.json package-lock.json
git commit -m "feat: add reminder list and form interface"
```

## Task 6: Connect Tauri Commands, Tray Menu, Autostart, And Window Lifecycle

**Files:**
- Create: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Write failing command-helper tests**

Add tests for the helper that maps form input to a validated pending reminder:

```rust
#[test]
fn create_input_rejects_blank_content() {
    let result = validate_create(CreateReminderInput {
        content: "   ".into(),
        time: "09:30".into(),
        repeat_type: RepeatType::None,
    }, dt(2026, 6, 1, 9, 0));
    assert_eq!(result.unwrap_err(), ValidationError::EmptyContent);
}
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands -- --nocapture
```

Expected: FAIL because commands and validation helper are missing.

- [ ] **Step 3: Implement commands and managed app state**

Open the database beside the executable during Tauri setup:

```rust
fn database_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let install_dir = exe.parent().ok_or_else(|| "无法确定安装目录".to_string())?;
    Ok(install_dir.join("reminders.db"))
}
```

Construct `ReminderRepository::open(&database_path()?)`, wrap it in
`Arc<ReminderRepository>`, and store it in `AppState`.

Expose:

```rust
#[tauri::command]
fn list_reminders(state: State<'_, AppState>, include_history: bool) -> Result<Vec<Reminder>, String>;

#[tauri::command]
fn create_reminder(app: AppHandle, state: State<'_, AppState>, input: CreateReminderInput) -> Result<Reminder, String>;

#[tauri::command]
fn delete_reminder(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String>;

#[tauri::command]
fn complete_animation(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String>;

#[tauri::command]
fn show_add_window(app: AppHandle) -> Result<(), String>;

#[tauri::command]
fn hide_add_window(app: AppHandle) -> Result<(), String>;
```

`create_reminder` invokes an immediate scheduler tick so a newly saved record
inside the five-minute window animates at once. Every mutation emits
`reminders-changed`.

- [ ] **Step 4: Implement tray behavior**

Build a tray with Chinese `提醒事项` and `退出` items. Left double-click opens
and centers the list window. The reminders item does the same. The exit item
calls a small `MessageBoxW` helper with `MB_OKCANCEL | MB_ICONQUESTION` before
`app.exit(0)`. Intercept close requests for the list and add windows, prevent
closing, and hide instead.

- [ ] **Step 5: Register single-instance and autostart plugins**

Register `tauri-plugin-single-instance` as the first builder plugin. On a
second launch, show and focus the reminders window in the existing process. Register
`tauri-plugin-autostart`, and during setup call `enable()` when it is not
already enabled.

- [ ] **Step 6: Verify Rust shell compilation**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: Rust tests and compile check pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri
git commit -m "feat: connect tray windows and reminder commands"
```

## Task 7: Implement The Attached Rocket Banner Overlay

**Files:**
- Create: `src/animation.ts`
- Create: `src/animation.test.ts`
- Create: `src/styles/animation.css`
- Create: `src-tauri/src/animation.rs`
- Modify: `animation.html`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing frontend animation tests**

Test payload rendering:

```ts
it("keeps the full reminder text inside the attached banner", () => {
  renderRocket({ id: 7, content: "这是一条非常长且不能截断的提醒事项" });
  expect(document.querySelector(".banner-text")?.textContent)
    .toBe("这是一条非常长且不能截断的提醒事项");
  expect(document.querySelector(".rocket-train")).not.toBeNull();
  expect(document.querySelector(".banner-flow")).not.toBeNull();
});
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
& 'D:\program\nodejs\npm.cmd' test -- src/animation.test.ts
```

Expected: FAIL because overlay rendering is missing.

- [ ] **Step 3: Implement the overlay markup and animation**

Render one connected `.rocket-train` element:

```html
<div class="rocket-train">
  <div class="banner">
    <div class="banner-flow"></div>
    <span class="banner-text"></span>
  </div>
  <div class="rocket" aria-hidden="true">
    <span class="rocket-body">🚀</span>
    <span class="rocket-flame"></span>
  </div>
</div>
```

Use `width: max-content` on the banner, no ellipsis, no maximum width, and no
font shrinking. Animate the connected train from `translateX(-100%)` to
`translateX(100vw)` over eight seconds. Animate `.banner-flow` continuously
inside the banner so the texture moves while the train is being pulled.

On `animationend`, invoke `complete_animation` with the reminder id.

- [ ] **Step 4: Implement overlay-window control in Rust**

Before showing `rocket-banner`:

1. Read the primary monitor size and position.
2. Set overlay width to the primary-monitor width.
3. Set overlay height to approximately 20 percent of primary-monitor height.
4. Place the overlay at the primary-monitor left edge and top area.
5. Call `set_ignore_cursor_events(true)`, `set_focusable(false)`,
   `set_skip_taskbar(true)`, and show without focusing.
6. Emit `play-reminder` with `{ id, content }`.

On cancellation, emit `cancel-reminder` and hide the overlay.

- [ ] **Step 5: Run frontend and Rust verification**

```powershell
& 'D:\program\nodejs\npm.cmd' test
& 'D:\program\nodejs\npm.cmd' run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: all frontend and Rust checks pass.

- [ ] **Step 6: Commit**

```powershell
git add animation.html src src-tauri
git commit -m "feat: animate attached rocket reminder banner"
```

## Task 8: Handle Windows Lock, Unlock, Suspend, And Resume

**Files:**
- Create: `src-tauri/src/system_monitor.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/scheduler.rs`

- [ ] **Step 1: Write failing event-decoder tests**

```rust
#[test]
fn decodes_session_lock_and_unlock() {
    assert_eq!(decode_session_change(WTS_SESSION_LOCK), Some(SystemEvent::Locked));
    assert_eq!(decode_session_change(WTS_SESSION_UNLOCK), Some(SystemEvent::Unlocked));
}

#[test]
fn decodes_resume_notification() {
    assert_eq!(decode_power_change(PBT_APMRESUMEAUTOMATIC), Some(SystemEvent::Resume));
}
```

- [ ] **Step 2: Run tests to verify RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml system_monitor -- --nocapture
```

Expected: FAIL because Win32 event decoding is missing.

- [ ] **Step 3: Implement the isolated Win32 adapter**

Start a background thread that:

1. Creates a hidden Win32 message window.
2. Calls `WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)`.
3. Processes `WM_WTSSESSION_CHANGE` and maps `WTS_SESSION_LOCK` /
   `WTS_SESSION_UNLOCK`.
4. Processes `WM_POWERBROADCAST` and maps `PBT_APMRESUMEAUTOMATIC`.
5. Sends `SystemEvent` values through a Rust channel.

Keep Windows constants and unsafe calls inside `system_monitor.rs`.

Use this startup/resume lock-state check:

```rust
pub fn is_workstation_locked() -> bool {
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if desktop.is_null() {
            return true;
        }
        let unlocked = SwitchDesktop(desktop) != 0;
        CloseDesktop(desktop);
        !unlocked
    }
}
```

- [ ] **Step 4: Connect system events to scheduling**

Maintain an atomic locked flag. On:

- `Locked`: set locked, cancel the active overlay, mark active and queued due
  reminders missed, derive repeats, then tick the scheduler while locked.
- `Unlocked`: clear locked and tick the scheduler.
- `Resume`: refresh the lock state and tick the scheduler.

Start a periodic scheduler tick every five seconds in addition to event-driven
ticks.

- [ ] **Step 5: Verify Rust tests and compilation**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: all Rust checks pass.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri
git commit -m "feat: mark locked and resumed reminders as missed"
```

## Task 9: Configure NSIS Installer And Run End-To-End Verification

**Files:**
- Create: `src-tauri/windows/hooks.nsh`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `.gitignore`
- Create: `docs/manual-test-checklist.md`

- [ ] **Step 1: Verify the generated Windows icon set**

Run:

```powershell
Get-ChildItem src-tauri/icons
```

Expected: the icon source, `icon.ico`, and Tauri-generated platform variants
from Task 1 are present.

- [ ] **Step 2: Add the uninstall retention prompt**

Create `src-tauri/windows/hooks.nsh`:

```nsh
!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\reminders.db" 0 done
  MessageBox MB_YESNO|MB_ICONQUESTION "是否删除提醒事项数据库？选择“否”将保留历史记录。" IDNO done
  Delete "$INSTDIR\reminders.db"
done:
!macroend
```

Keep `installMode` as `currentUser` and `installerHooks` as
`./windows/hooks.nsh`.

- [ ] **Step 3: Add a manual Windows acceptance checklist**

Document exact steps for:

1. Pending-only default list and all-history switch.
2. Daily and weekly derivation.
3. Immediate animation inside five minutes.
4. Eight-second attached banner with flowing texture and full long text.
5. Main-monitor-only placement, transparency, always-on-top, click-through,
   focus-free behavior, and no taskbar entry.
6. Lock before trigger, lock during animation, sleep/resume, and long offline
   catch-up.
7. Tray right-click items, tray double-click, hidden close, confirmed exit,
   and autostart persistence after exit.
8. Selectable install directory and both uninstall database-retention choices.

- [ ] **Step 4: Run the full automated verification**

```powershell
& 'D:\program\nodejs\npm.cmd' test
& 'D:\program\nodejs\npm.cmd' run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
& 'D:\program\nodejs\npm.cmd' run tauri:build
```

Expected: frontend tests pass, frontend bundle builds, Rust tests pass, Rust
compiles, and an NSIS `.exe` installer is created under
`src-tauri/target/release/bundle/nsis/`.

- [ ] **Step 5: Run manual Windows verification**

Install the generated `.exe` into a current-user writable directory. Execute
every item in `docs/manual-test-checklist.md`, record pass/fail notes, fix any
failure with a failing automated test where feasible, and repeat the full
verification command.

- [ ] **Step 6: Commit**

```powershell
git add .gitignore src-tauri docs/manual-test-checklist.md
git commit -m "build: package rocket reminder windows installer"
```

## Official References

- Tauri Windows prerequisites: <https://v2.tauri.app/start/prerequisites/>
- Tauri NSIS installer configuration: <https://v2.tauri.app/distribute/windows-installer/>
- Tauri NSIS installer hooks: <https://v2.tauri.app/distribute/windows-installer/#installer-hooks>
- Tauri autostart plugin: <https://v2.tauri.app/plugin/autostart/>
- Tauri single-instance plugin: <https://v2.tauri.app/plugin/single-instance/>
- Microsoft session notifications: <https://learn.microsoft.com/windows/win32/termserv/wm-wtssession-change>
- Microsoft power notifications: <https://learn.microsoft.com/windows/win32/power/wm-powerbroadcast>
