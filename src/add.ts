import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Reminder } from "./shared/types";
import "./styles/add.css";

const form = document.querySelector<HTMLFormElement>("form")!;
const title = document.querySelector<HTMLHeadingElement>("#editor-title")!;
const time = document.querySelector<HTMLInputElement>("#time")!;
const content = document.querySelector<HTMLInputElement>("#content")!;
const repeat = document.querySelector<HTMLInputElement>("#repeat")!;
const daily = document.querySelector<HTMLInputElement>("#daily")!;
const weekly = document.querySelector<HTMLInputElement>("#weekly")!;
const options = document.querySelector<HTMLElement>("#repeat-options")!;
const loop = document.querySelector<HTMLInputElement>("#loop")!;
const loopOptions = document.querySelector<HTMLElement>("#loop-options")!;
const loopHours = document.querySelector<HTMLInputElement>("#loop-hours")!;
const loopMinutes = document.querySelector<HTMLInputElement>("#loop-minutes")!;
const loopStartTime = document.querySelector<HTMLInputElement>("#loop-start-time")!;
const loopEndTime = document.querySelector<HTMLInputElement>("#loop-end-time")!;
const error = document.querySelector<HTMLParagraphElement>("#error")!;
let editingId: number | null = null;

function syncRepeat(): void {
  options.classList.toggle("hidden", !repeat.checked);
  if (!repeat.checked) {
    daily.checked = false;
    weekly.checked = false;
  } else if (!daily.checked && !weekly.checked) {
    repeat.checked = false;
    options.classList.add("hidden");
  }
}

function syncLoop(): void {
  loopOptions.classList.toggle("hidden", !loop.checked);
  time.disabled = loop.checked;
  time.required = !loop.checked;
  loopStartTime.required = loop.checked;
  loopEndTime.required = loop.checked;
}

function timeValue(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function resetEditor(): void {
  editingId = null;
  title.textContent = "新增提醒事项";
  form.reset();
  syncRepeat();
  syncLoop();
  error.textContent = "";
}

function editReminder(reminder: Reminder): void {
  editingId = reminder.id;
  title.textContent = "编辑提醒事项";
  content.value = reminder.content;
  time.value = timeValue(reminder.scheduled_at);
  daily.checked = reminder.repeat_type === "daily";
  weekly.checked = reminder.repeat_type === "weekly";
  repeat.checked = daily.checked || weekly.checked;
  loop.checked = reminder.loop_enabled;
  loopHours.value = String(Math.floor((reminder.loop_interval_minutes ?? 0) / 60));
  loopMinutes.value = String((reminder.loop_interval_minutes ?? 30) % 60);
  loopStartTime.value = reminder.loop_start_time ?? "";
  loopEndTime.value = reminder.loop_end_time ?? "";
  syncRepeat();
  syncLoop();
  error.textContent = "";
}

repeat.addEventListener("change", () => {
  options.classList.toggle("hidden", !repeat.checked);
  if (!repeat.checked) {
    daily.checked = false;
    weekly.checked = false;
  }
});
daily.addEventListener("change", () => {
  if (daily.checked) weekly.checked = false;
  syncRepeat();
});
weekly.addEventListener("change", () => {
  if (weekly.checked) daily.checked = false;
  syncRepeat();
});
loop.addEventListener("change", syncLoop);
document.querySelector("#cancel")!.addEventListener("click", () => void invoke("hide_add_window"));
form.addEventListener("submit", async (event) => {
  event.preventDefault();
  error.textContent = "";
  const repeatType = daily.checked ? "daily" : weekly.checked ? "weekly" : "none";
  try {
    const input = {
      content: content.value,
      time: time.value,
      repeatType,
      loopEnabled: loop.checked,
      loopHours: Number(loopHours.value),
      loopMinutes: Number(loopMinutes.value),
      loopStartTime: loopStartTime.value || null,
      loopEndTime: loopEndTime.value || null,
    };
    if (editingId === null) {
      await invoke("create_reminder", { input });
    } else {
      await invoke("update_reminder", { id: editingId, input });
    }
    resetEditor();
    await invoke("hide_add_window");
  } catch (reason) {
    error.textContent = String(reason);
  }
});
void listen<{ mode: "create" | "edit"; reminder: Reminder | null }>("reminder-editor", ({ payload }) => {
  if (payload.mode === "edit" && payload.reminder) {
    editReminder(payload.reminder);
  } else {
    resetEditor();
  }
});
resetEditor();
