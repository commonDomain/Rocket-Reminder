import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles/animation.css";

interface AnimationPayload { id: number; content: string }
interface ProbePayload { id: string }
const train = document.querySelector<HTMLElement>("#train")!;
const banner = document.querySelector<HTMLElement>("#banner")!;
const PIXELS_PER_SECOND = 255;
let activeId: number | null = null;
let completionTimer: number | null = null;

function clearCompletionTimer(): void {
  if (completionTimer !== null) {
    window.clearTimeout(completionTimer);
    completionTimer = null;
  }
}

function completeActive(): void {
  if (activeId === null) return;
  const id = activeId;
  activeId = null;
  clearCompletionTimer();
  train.classList.remove("playing");
  void invoke("complete_animation", { id });
}

async function recordProbe(id: string, ok: boolean, message: string): Promise<void> {
  await invoke("record_animation_probe", { id, ok, message });
}

void listen<AnimationPayload>("play-reminder", ({ payload }) => {
  clearCompletionTimer();
  activeId = payload.id;
  banner.textContent = payload.content;
  train.classList.remove("playing");
  train.classList.add("measuring");
  const distance = window.innerWidth + train.getBoundingClientRect().width;
  const durationSeconds = distance / PIXELS_PER_SECOND;
  train.style.setProperty("--fly-duration", `${durationSeconds}s`);
  train.classList.remove("measuring");
  void train.offsetWidth;
  train.classList.add("playing");
  completionTimer = window.setTimeout(completeActive, durationSeconds * 1000 + 1000);
});
void listen("cancel-reminder", () => {
  activeId = null;
  clearCompletionTimer();
  train.classList.remove("playing");
  train.classList.remove("measuring");
});
void listen<ProbePayload>("watchdog-probe", ({ payload }) => {
  void (async () => {
    if (activeId !== null) {
      await recordProbe(payload.id, false, "正在播放真实提醒，已跳过本次透明动画自检");
      return;
    }
    try {
      clearCompletionTimer();
      banner.textContent = "动画自检";
      train.classList.remove("playing");
      train.classList.add("measuring");
      const width = train.getBoundingClientRect().width;
      const distance = window.innerWidth + width;
      const durationSeconds = distance / PIXELS_PER_SECOND;
      const ok = window.innerWidth > 0 && width > 0 && Number.isFinite(durationSeconds) && durationSeconds > 0;
      train.classList.remove("measuring");
      await recordProbe(
        payload.id,
        ok,
        ok
          ? `隐藏动画页面正常，页面宽度 ${window.innerWidth}px，动画元素宽度 ${Math.round(width)}px`
          : "隐藏动画页面可响应，但动画元素尺寸异常"
      );
    } catch (error) {
      train.classList.remove("measuring");
      await recordProbe(payload.id, false, `前端动画自检出错：${String(error)}`);
    }
  })();
});
train.addEventListener("animationend", (event) => {
  if (event.animationName !== "fly" || activeId === null) return;
  completeActive();
});
