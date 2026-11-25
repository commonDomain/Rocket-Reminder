export type RepeatType = "none" | "daily" | "weekly";
export type ReminderStatus = "pending" | "running" | "completed" | "missed" | "stopped";

export interface Reminder {
  id: number;
  content: string;
  scheduled_at: string;
  repeat_type: RepeatType;
  status: ReminderStatus;
  loop_enabled: boolean;
  loop_interval_minutes: number | null;
  loop_start_time: string | null;
  loop_end_time: string | null;
}
