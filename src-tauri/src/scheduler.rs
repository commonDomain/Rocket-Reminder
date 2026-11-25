use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration, Local};

use crate::db::{AppError, ReminderRepository};
use crate::model::{trigger_at, Reminder, ReminderStatus};

const STALE_ACTIVE_AFTER: Duration = Duration::minutes(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerHealth {
    pub queued: usize,
    pub claimed: usize,
    pub active_id: Option<i64>,
    pub active_for_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Wait,
    Animate,
    Miss,
}

pub fn decide(reminder: &Reminder, now: DateTime<Local>, locked: bool) -> Decision {
    if reminder.loop_enabled {
        return if reminder.scheduled_at <= now {
            if locked {
                Decision::Miss
            } else {
                Decision::Animate
            }
        } else {
            Decision::Wait
        };
    }
    if reminder.scheduled_at <= now {
        Decision::Miss
    } else if trigger_at(reminder) <= now {
        if locked {
            Decision::Miss
        } else {
            Decision::Animate
        }
    } else {
        Decision::Wait
    }
}

#[derive(Default)]
struct AnimationQueue {
    queued: VecDeque<i64>,
    claimed: HashSet<i64>,
    active: Option<ActiveAnimation>,
}

struct ActiveAnimation {
    id: i64,
    started_at: DateTime<Local>,
}

impl AnimationQueue {
    fn enqueue(&mut self, id: i64) {
        if self.claimed.insert(id) {
            self.queued.push_back(id);
        }
    }

    fn start_next(&mut self, now: DateTime<Local>) -> Option<i64> {
        if self.active.is_some() {
            return None;
        }
        let id = self.queued.pop_front()?;
        self.active = Some(ActiveAnimation {
            id,
            started_at: now,
        });
        Some(id)
    }

    fn finish(&mut self, id: i64) -> bool {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
            self.claimed.remove(&id);
            true
        } else {
            false
        }
    }

    fn drain_claimed(&mut self) -> Vec<i64> {
        self.active = None;
        self.queued.clear();
        self.claimed.drain().collect()
    }

    fn cancel(&mut self, id: i64) -> bool {
        self.queued.retain(|queued_id| *queued_id != id);
        self.claimed.remove(&id);
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
            true
        } else {
            false
        }
    }

    fn drain_stale_active(&mut self, now: DateTime<Local>) -> Option<i64> {
        let active = self.active.as_ref()?;
        if now.signed_duration_since(active.started_at) < STALE_ACTIVE_AFTER {
            return None;
        }
        let id = active.id;
        self.active = None;
        self.claimed.remove(&id);
        Some(id)
    }
}

pub struct SchedulerService {
    repo: Arc<ReminderRepository>,
    queue: Mutex<AnimationQueue>,
}

impl SchedulerService {
    pub fn new(repo: Arc<ReminderRepository>) -> Self {
        Self {
            repo,
            queue: Mutex::new(AnimationQueue::default()),
        }
    }

    pub fn repository(&self) -> &ReminderRepository {
        &self.repo
    }

    pub fn tick(&self, now: DateTime<Local>, locked: bool) -> Result<bool, AppError> {
        let mut changed = self.recover_stale_active(now)?;
        loop {
            let mut missed_any = false;
            for reminder in self.repo.pending()? {
                match decide(&reminder, now, locked) {
                    Decision::Miss => {
                        if self.repo.mark_missed(reminder.id, now)?.is_some() {
                            changed = true;
                            missed_any = true;
                        }
                    }
                    Decision::Animate => self.queue()?.enqueue(reminder.id),
                    Decision::Wait => {}
                }
            }
            if !missed_any {
                return Ok(changed);
            }
        }
    }

    pub fn next_animation(&self, now: DateTime<Local>) -> Result<Option<Reminder>, AppError> {
        loop {
            let Some(id) = self.queue()?.start_next(now) else {
                return Ok(None);
            };
            let Some(reminder) = self.repo.get(id)? else {
                let _ = self.queue()?.finish(id);
                continue;
            };
            if matches!(
                reminder.status,
                ReminderStatus::Pending | ReminderStatus::Running
            ) {
                return Ok(Some(reminder));
            }
            let _ = self.queue()?.finish(id);
        }
    }

    pub fn complete_active(&self, id: i64, now: DateTime<Local>) -> Result<bool, AppError> {
        if !self.queue()?.finish(id) {
            return Ok(false);
        }
        Ok(self.repo.finish_animation(id, now)?.is_some())
    }

    pub fn cancel_claimed_as_missed(&self, now: DateTime<Local>) -> Result<bool, AppError> {
        let mut changed = false;
        for id in self.queue()?.drain_claimed() {
            changed |= self.repo.mark_missed(id, now)?.is_some();
        }
        Ok(changed)
    }

    pub fn stop_loop(&self, id: i64, now: DateTime<Local>) -> Result<(bool, bool), AppError> {
        let was_active = self.queue()?.cancel(id);
        Ok((self.repo.stop_loop(id, now)?, was_active))
    }

    pub fn resume_loop(&self, id: i64, now: DateTime<Local>) -> Result<bool, AppError> {
        self.repo.resume_loop(id, now)
    }

    pub fn remove_claim(&self, id: i64) -> Result<bool, AppError> {
        Ok(self.queue()?.cancel(id))
    }

    pub fn abandon_active(&self, id: i64) -> Result<bool, AppError> {
        Ok(self.queue()?.finish(id))
    }

    pub fn health_snapshot(&self, now: DateTime<Local>) -> Result<SchedulerHealth, AppError> {
        let queue = self.queue()?;
        Ok(SchedulerHealth {
            queued: queue.queued.len(),
            claimed: queue.claimed.len(),
            active_id: queue.active.as_ref().map(|active| active.id),
            active_for_seconds: queue
                .active
                .as_ref()
                .map(|active| now.signed_duration_since(active.started_at).num_seconds()),
        })
    }

    fn queue(&self) -> Result<MutexGuard<'_, AnimationQueue>, AppError> {
        self.queue.lock().map_err(|_| AppError::DatabaseLock)
    }

    fn recover_stale_active(&self, now: DateTime<Local>) -> Result<bool, AppError> {
        let Some(id) = self.queue()?.drain_stale_active(now) else {
            return Ok(false);
        };
        let Some(reminder) = self.repo.get(id)? else {
            return Ok(false);
        };
        if reminder.loop_enabled || reminder.scheduled_at <= now {
            return Ok(self.repo.mark_missed(id, now)?.is_some());
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::model::RepeatType;

    fn local_dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn service() -> SchedulerService {
        SchedulerService::new(Arc::new(ReminderRepository::in_memory().unwrap()))
    }

    #[test]
    fn abandoned_launch_can_be_retried() {
        let scheduler = service();
        let reminder = scheduler
            .repository()
            .insert_pending(
                "Prepare launch",
                local_dt(2026, 6, 1, 10, 5),
                RepeatType::None,
                None,
            )
            .unwrap();
        let now = local_dt(2026, 6, 1, 10, 0);

        scheduler.tick(now, false).unwrap();
        assert_eq!(
            scheduler.next_animation(now).unwrap().unwrap().id,
            reminder.id
        );
        assert!(scheduler.abandon_active(reminder.id).unwrap());

        scheduler.tick(now, false).unwrap();
        assert_eq!(
            scheduler.next_animation(now).unwrap().unwrap().id,
            reminder.id
        );
    }

    #[test]
    fn stale_active_animation_retries_before_scheduled_time() {
        let scheduler = service();
        let reminder = scheduler
            .repository()
            .insert_pending(
                "Prepare launch",
                local_dt(2026, 6, 1, 10, 5),
                RepeatType::None,
                None,
            )
            .unwrap();
        let trigger_time = local_dt(2026, 6, 1, 10, 0);

        scheduler.tick(trigger_time, false).unwrap();
        assert_eq!(
            scheduler.next_animation(trigger_time).unwrap().unwrap().id,
            reminder.id
        );

        let stale_time = trigger_time + STALE_ACTIVE_AFTER + Duration::seconds(1);
        assert!(!scheduler.tick(stale_time, false).unwrap());
        assert_eq!(
            scheduler.next_animation(stale_time).unwrap().unwrap().id,
            reminder.id
        );
    }

    #[test]
    fn stale_active_animation_is_missed_after_scheduled_time() {
        let scheduler = service();
        let reminder = scheduler
            .repository()
            .insert_pending(
                "Prepare launch",
                local_dt(2026, 6, 1, 10, 1),
                RepeatType::None,
                None,
            )
            .unwrap();
        let trigger_time = local_dt(2026, 6, 1, 9, 56);

        scheduler.tick(trigger_time, false).unwrap();
        assert_eq!(
            scheduler.next_animation(trigger_time).unwrap().unwrap().id,
            reminder.id
        );

        let stale_time = trigger_time + STALE_ACTIVE_AFTER + Duration::minutes(4);
        assert!(scheduler.tick(stale_time, false).unwrap());
        assert!(scheduler.next_animation(stale_time).unwrap().is_none());
        assert_eq!(
            scheduler
                .repository()
                .get(reminder.id)
                .unwrap()
                .unwrap()
                .status,
            ReminderStatus::Missed
        );
    }
}
