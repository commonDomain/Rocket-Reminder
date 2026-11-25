import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Reminder, ReminderStatus, RepeatType } from "./shared/types";
import "./styles/list.css";

const tbody = document.querySelector<HTMLTableSectionElement>("#reminders")!;
const error = document.querySelector<HTMLParagraphElement>("#error")!;
const pendingButton = document.querySelector<HTMLButtonElement>("#pending-filter")!;
const allButton = document.querySelector<HTMLButtonElement>("#all-filter")!;
let includeHistory = false;

const repeatLabels: Record<RepeatType, string> = { none: "不重复", daily: "每日", weekly: "每周" };
const statusLabels: Record<ReminderStatus, string> = {
  pending: "待提醒", running: "进行中", completed: "已完成", missed: "已错过", stopped: "已停止",
};

function timeLabel(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false,
  }).format(date);
}

function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}

function repeatLabel(reminder: Reminder): string {
  const repeat = repeatLabels[reminder.repeat_type];
  if (!reminder.loop_enabled || reminder.loop_interval_minutes === null) return repeat;
  const hours = Math.floor(reminder.loop_interval_minutes / 60);
  const minutes = reminder.loop_interval_minutes % 60;
  const interval = `${hours ? `${hours}小时` : ""}${minutes ? `${minutes}分钟` : ""}`;
  return `${repeat} / 每${interval}循环`;
}

function render(reminders: Reminder[]): void {
  tbody.replaceChildren();
  if (reminders.length === 0) {
    const row = document.createElement("tr");
    row.innerHTML = '<td class="empty" colspan="5">暂无提醒事项</td>';
    tbody.append(row);
    return;
  }
  for (const reminder of reminders) {
    const row = document.createElement("tr");
    row.className = reminder.status;
    row.innerHTML = `<td title="${escapeHtml(reminder.content)}">${escapeHtml(reminder.content)}</td>
      <td>${timeLabel(reminder.scheduled_at)}</td><td>${repeatLabel(reminder)}</td>
      <td>${statusLabels[reminder.status]}</td><td>
        ${["pending", "running"].includes(reminder.status) ? '<button class="edit" type="button">编辑</button>' : ""}
        ${reminder.status === "running" ? '<button class="stop" type="button">停止</button>' : ""}
        ${reminder.status === "stopped" ? '<button class="resume" type="button">继续</button>' : ""}
        <button class="delete" type="button">删除</button>
      </td>`;
    row.querySelector(".edit")?.addEventListener("click", () => {
      void invoke("show_edit_window", { id: reminder.id });
    });
    row.querySelector(".stop")?.addEventListener("click", async () => {
      await invoke("stop_loop", { id: reminder.id });
      await reload();
    });
    row.querySelector(".resume")?.addEventListener("click", async () => {
      await invoke("resume_loop", { id: reminder.id });
      await reload();
    });
    row.querySelector(".delete")!.addEventListener("click", async () => {
      if (!window.confirm("确定删除这条提醒事项吗？")) return;
      await invoke("delete_reminder", { id: reminder.id });
      await reload();
    });
    tbody.append(row);
  }
}

async function reload(): Promise<void> {
  try {
    error.textContent = "";
    render(await invoke<Reminder[]>("list_reminders", { includeHistory }));
  } catch (reason) {
    error.textContent = String(reason);
  }
}

pendingButton.addEventListener("click", () => {
  includeHistory = false;
  pendingButton.classList.add("active");
  allButton.classList.remove("active");
  void reload();
});
allButton.addEventListener("click", () => {
  includeHistory = true;
  allButton.classList.add("active");
  pendingButton.classList.remove("active");
  void reload();
});
document.querySelector("#add")!.addEventListener("click", () => void invoke("show_add_window"));
void listen("reminders-changed", reload);
void reload();
