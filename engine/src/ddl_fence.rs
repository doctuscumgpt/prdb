//! Per-table coordination between data writers and schema-changing work.
//!
//! Writers acquire shared fences for every table they can modify. DDL acquires
//! exclusive fences for every affected table in one operation. A manager must
//! be shared by all writers and DDL operations for the same database; pinned
//! read-only snapshots do not need a fence.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub const MAX_DDL_FENCE_WAITERS: usize = 128;
pub const MAX_DDL_FENCE_RESOURCES_PER_REQUEST: usize = 128;
pub const MAX_DDL_FENCE_WAIT_PAGE: usize = 16;
pub const MAX_DDL_FENCE_IDS_PER_WAIT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdlFenceError {
    InvalidTableId,
    TooManyResources,
    TimedOut,
    DeadlineOutOfRange,
    TooManyWaiters,
    RandomnessUnavailable,
}

impl std::fmt::Display for DdlFenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "DDL fence unavailable: {self:?}")
    }
}

impl std::error::Error for DdlFenceError {}

#[derive(Clone, Default)]
pub struct DdlFenceManager {
    inner: Arc<DdlFenceInner>,
}

/// Shares one fence manager and timeout policy across coordinated commit paths.
#[derive(Clone)]
pub struct DdlFenceCoordinator {
    manager: DdlFenceManager,
    timeout: Duration,
}

#[derive(Debug, Default)]
struct DdlFenceInner {
    state: Mutex<FenceState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct FenceState {
    tables: HashMap<[u8; 16], TableFence>,
    waiters: HashMap<[u8; 16], FenceWaiter>,
}

#[derive(Debug, Default)]
struct TableFence {
    active_writers: HashSet<[u8; 16]>,
    exclusive_held: Option<[u8; 16]>,
    waiting_exclusive: HashSet<[u8; 16]>,
}

#[derive(Debug)]
struct FenceWaiter {
    mode: FenceMode,
    table_ids: Vec<[u8; 16]>,
    started_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdlFenceWaitCategory {
    Writer,
    Ddl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DdlFenceWaitStatus {
    pub wait_id: [u8; 16],
    pub category: DdlFenceWaitCategory,
    pub waited_us: u64,
    pub resource_ids: Vec<[u8; 16]>,
    pub resources_truncated: bool,
    pub blocker_ids: Vec<[u8; 16]>,
    pub blockers_truncated: bool,
}

#[derive(Clone, Copy, Debug)]
enum FenceMode {
    Shared,
    Exclusive,
}

/// RAII ownership of one or more table fences.
#[must_use = "dropping the guard releases its table fences"]
#[derive(Debug)]
pub struct DdlFenceGuard {
    inner: Arc<DdlFenceInner>,
    lease_id: [u8; 16],
    table_ids: Vec<[u8; 16]>,
    mode: FenceMode,
}

impl DdlFenceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit a data writer for all listed tables until the returned guard drops.
    pub fn acquire_shared(
        &self,
        table_ids: &[[u8; 16]],
        timeout: Duration,
    ) -> Result<DdlFenceGuard, DdlFenceError> {
        self.acquire(table_ids, timeout, FenceMode::Shared)
    }

    /// Exclude data writers for all listed tables until the returned guard drops.
    pub fn acquire_exclusive(
        &self,
        table_ids: &[[u8; 16]],
        timeout: Duration,
    ) -> Result<DdlFenceGuard, DdlFenceError> {
        self.acquire(table_ids, timeout, FenceMode::Exclusive)
    }

    fn acquire(
        &self,
        table_ids: &[[u8; 16]],
        timeout: Duration,
        mode: FenceMode,
    ) -> Result<DdlFenceGuard, DdlFenceError> {
        let table_ids = normalize_table_ids(table_ids)?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(DdlFenceError::DeadlineOutOfRange)?;
        let mut state = self.lock_state();
        let lease_id = unique_id(&state)?;
        if can_acquire(&state, &table_ids, mode) {
            acquire_tables(&mut state, &table_ids, mode, lease_id);
            return Ok(DdlFenceGuard {
                inner: Arc::clone(&self.inner),
                lease_id,
                table_ids,
                mode,
            });
        }
        if table_ids.len() > MAX_DDL_FENCE_RESOURCES_PER_REQUEST {
            return Err(DdlFenceError::TooManyResources);
        }
        if state.waiters.len() >= MAX_DDL_FENCE_WAITERS {
            return Err(DdlFenceError::TooManyWaiters);
        }
        if matches!(mode, FenceMode::Exclusive) {
            for table_id in &table_ids {
                state
                    .tables
                    .entry(*table_id)
                    .or_default()
                    .waiting_exclusive
                    .insert(lease_id);
            }
        }
        state.waiters.insert(
            lease_id,
            FenceWaiter {
                mode,
                table_ids: table_ids.clone(),
                started_at: Instant::now(),
            },
        );

        loop {
            if can_acquire(&state, &table_ids, mode) {
                remove_waiter(&mut state, lease_id);
                acquire_tables(&mut state, &table_ids, mode, lease_id);
                return Ok(DdlFenceGuard {
                    inner: Arc::clone(&self.inner),
                    lease_id,
                    table_ids,
                    mode,
                });
            }

            let now = Instant::now();
            if now >= deadline {
                remove_waiter(&mut state, lease_id);
                drop(state);
                self.inner.changed.notify_all();
                return Err(DdlFenceError::TimedOut);
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_state, _) = self
                .inner
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            state = next_state;
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, FenceState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn wait_snapshot(&self) -> (Vec<DdlFenceWaitStatus>, bool) {
        let state = self.lock_state();
        let mut waits = state
            .waiters
            .iter()
            .map(|(wait_id, waiter)| {
                let mut blockers = HashSet::new();
                let mut blockers_truncated = false;
                {
                    let mut record_blocker = |blocker_id| {
                        if blockers.contains(&blocker_id) {
                            return;
                        }
                        if blockers.len() <= MAX_DDL_FENCE_IDS_PER_WAIT {
                            blockers.insert(blocker_id);
                        } else {
                            blockers_truncated = true;
                        }
                    };
                    for table_id in &waiter.table_ids {
                        if let Some(fence) = state.tables.get(table_id) {
                            if let Some(exclusive_id) = fence.exclusive_held {
                                record_blocker(exclusive_id);
                            }
                            match waiter.mode {
                                FenceMode::Exclusive => {
                                    for blocker_id in &fence.active_writers {
                                        record_blocker(*blocker_id);
                                    }
                                }
                                FenceMode::Shared => {
                                    for blocker_id in &fence.waiting_exclusive {
                                        if blocker_id != wait_id {
                                            record_blocker(*blocker_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let mut blocker_ids = blockers.into_iter().collect::<Vec<_>>();
                blocker_ids.sort_unstable();
                let resources_truncated = waiter.table_ids.len() > MAX_DDL_FENCE_IDS_PER_WAIT;
                let resource_ids = waiter
                    .table_ids
                    .iter()
                    .take(MAX_DDL_FENCE_IDS_PER_WAIT)
                    .copied()
                    .collect();
                blockers_truncated |= blocker_ids.len() > MAX_DDL_FENCE_IDS_PER_WAIT;
                blocker_ids.truncate(MAX_DDL_FENCE_IDS_PER_WAIT);
                DdlFenceWaitStatus {
                    wait_id: *wait_id,
                    category: match waiter.mode {
                        FenceMode::Shared => DdlFenceWaitCategory::Writer,
                        FenceMode::Exclusive => DdlFenceWaitCategory::Ddl,
                    },
                    waited_us: u64::try_from(waiter.started_at.elapsed().as_micros())
                        .unwrap_or(u64::MAX),
                    resource_ids,
                    resources_truncated,
                    blocker_ids,
                    blockers_truncated,
                }
            })
            .collect::<Vec<_>>();
        waits.sort_unstable_by_key(|wait| wait.wait_id);
        let truncated = waits.len() > MAX_DDL_FENCE_WAIT_PAGE;
        waits.truncate(MAX_DDL_FENCE_WAIT_PAGE);
        (waits, truncated)
    }
}

impl DdlFenceCoordinator {
    pub fn new(manager: DdlFenceManager, timeout: Duration) -> Self {
        Self { manager, timeout }
    }

    pub fn acquire_shared(&self, table_ids: &[[u8; 16]]) -> Result<DdlFenceGuard, DdlFenceError> {
        self.manager.acquire_shared(table_ids, self.timeout)
    }

    pub fn acquire_exclusive(
        &self,
        table_ids: &[[u8; 16]],
    ) -> Result<DdlFenceGuard, DdlFenceError> {
        self.manager.acquire_exclusive(table_ids, self.timeout)
    }

    pub(crate) fn wait_snapshot(&self) -> (Vec<DdlFenceWaitStatus>, bool) {
        self.manager.wait_snapshot()
    }
}

impl Drop for DdlFenceGuard {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for table_id in &self.table_ids {
            if let Some(fence) = state.tables.get_mut(table_id) {
                match self.mode {
                    FenceMode::Shared => {
                        fence.active_writers.remove(&self.lease_id);
                    }
                    FenceMode::Exclusive => {
                        if fence.exclusive_held == Some(self.lease_id) {
                            fence.exclusive_held = None;
                        }
                    }
                }
            }
            remove_unused_fence(&mut state, table_id);
        }
        drop(state);
        self.inner.changed.notify_all();
    }
}

fn normalize_table_ids(table_ids: &[[u8; 16]]) -> Result<Vec<[u8; 16]>, DdlFenceError> {
    if table_ids.contains(&[0; 16]) {
        return Err(DdlFenceError::InvalidTableId);
    }
    let mut normalized = table_ids.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    Ok(normalized)
}

fn can_acquire(state: &FenceState, table_ids: &[[u8; 16]], mode: FenceMode) -> bool {
    table_ids.iter().all(|table_id| {
        let fence = state.tables.get(table_id);
        match mode {
            FenceMode::Shared => fence.is_none_or(|fence| {
                fence.exclusive_held.is_none() && fence.waiting_exclusive.is_empty()
            }),
            FenceMode::Exclusive => fence.is_none_or(|fence| {
                fence.exclusive_held.is_none() && fence.active_writers.is_empty()
            }),
        }
    })
}

fn remove_unused_fence(state: &mut FenceState, table_id: &[u8; 16]) {
    if state.tables.get(table_id).is_some_and(|fence| {
        fence.active_writers.is_empty()
            && fence.exclusive_held.is_none()
            && fence.waiting_exclusive.is_empty()
    }) {
        state.tables.remove(table_id);
    }
}

fn acquire_tables(
    state: &mut FenceState,
    table_ids: &[[u8; 16]],
    mode: FenceMode,
    lease_id: [u8; 16],
) {
    for table_id in table_ids {
        let fence = state.tables.entry(*table_id).or_default();
        match mode {
            FenceMode::Shared => {
                fence.active_writers.insert(lease_id);
            }
            FenceMode::Exclusive => {
                fence.exclusive_held = Some(lease_id);
            }
        }
    }
}

fn remove_waiter(state: &mut FenceState, wait_id: [u8; 16]) {
    if let Some(waiter) = state.waiters.remove(&wait_id)
        && matches!(waiter.mode, FenceMode::Exclusive)
    {
        for table_id in &waiter.table_ids {
            if let Some(fence) = state.tables.get_mut(table_id) {
                fence.waiting_exclusive.remove(&wait_id);
            }
            remove_unused_fence(state, table_id);
        }
    }
}

fn unique_id(state: &FenceState) -> Result<[u8; 16], DdlFenceError> {
    loop {
        let mut candidate = [0; 16];
        if getrandom::fill(&mut candidate).is_err() {
            return Err(DdlFenceError::RandomnessUnavailable);
        }
        if candidate == [0; 16] || state.waiters.contains_key(&candidate) {
            continue;
        }
        if state.tables.values().any(|fence| {
            fence.exclusive_held == Some(candidate)
                || fence.active_writers.contains(&candidate)
                || fence.waiting_exclusive.contains(&candidate)
        }) {
            continue;
        }
        return Ok(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DdlFenceError, DdlFenceManager, DdlFenceWaitCategory, MAX_DDL_FENCE_IDS_PER_WAIT,
        MAX_DDL_FENCE_RESOURCES_PER_REQUEST, MAX_DDL_FENCE_WAITERS,
    };
    use std::time::Duration;

    fn id(value: u8) -> [u8; 16] {
        [value; 16]
    }

    #[test]
    fn wait_snapshot_reports_bounded_resources_and_blocker_ids() {
        let manager = DdlFenceManager::new();
        let too_many_resources = (1..=MAX_DDL_FENCE_RESOURCES_PER_REQUEST + 1)
            .map(|value| [u8::try_from(value).unwrap(); 16])
            .collect::<Vec<_>>();
        let block = manager.acquire_exclusive(&[id(1)], Duration::ZERO).unwrap();
        assert!(matches!(
            manager.acquire_shared(&too_many_resources, Duration::ZERO),
            Err(DdlFenceError::TooManyResources)
        ));
        drop(block);
        drop(
            manager
                .acquire_shared(&too_many_resources, Duration::ZERO)
                .unwrap(),
        );
        let resources = (9..18).map(id).collect::<Vec<_>>();
        let writers = resources
            .iter()
            .map(|table_id| {
                manager
                    .acquire_shared(&[*table_id], Duration::ZERO)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let waiter_manager = manager.clone();
        let waiter_resources = resources.clone();
        let waiter = std::thread::spawn(move || {
            waiter_manager.acquire_exclusive(&waiter_resources, Duration::from_secs(2))
        });

        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        let waits = loop {
            let waits = manager.wait_snapshot().0;
            if !waits.is_empty() || std::time::Instant::now() >= deadline {
                break waits;
            }
            std::thread::yield_now();
        };
        assert!(waits.len() <= MAX_DDL_FENCE_WAITERS);
        assert_eq!(waits.len(), 1);
        assert_ne!(waits[0].wait_id, [0; 16]);
        assert_eq!(waits[0].category, DdlFenceWaitCategory::Ddl);
        assert_eq!(
            waits[0].resource_ids,
            resources[..MAX_DDL_FENCE_IDS_PER_WAIT]
        );
        assert!(waits[0].resource_ids.len() <= MAX_DDL_FENCE_IDS_PER_WAIT);
        assert!(waits[0].resources_truncated);
        assert_eq!(waits[0].blocker_ids.len(), MAX_DDL_FENCE_IDS_PER_WAIT);
        assert!(waits[0].blocker_ids.len() <= MAX_DDL_FENCE_IDS_PER_WAIT);
        assert!(waits[0].blockers_truncated);
        assert_ne!(waits[0].blocker_ids[0], [0; 16]);
        assert!(!waits[0].blocker_ids.contains(&waits[0].wait_id));

        drop(writers);
        drop(waiter.join().unwrap().unwrap());
        assert!(manager.wait_snapshot().0.is_empty());
    }

    #[test]
    fn waiter_capacity_is_enforced_and_page_truncation_is_explicit() {
        let bounded = DdlFenceManager::new();
        {
            let mut state = bounded.lock_state();
            state.tables.entry(id(240)).or_default().exclusive_held = Some(id(241));
            for value in 1..=MAX_DDL_FENCE_WAITERS {
                let wait_id = [u8::try_from(value).unwrap(); 16];
                state
                    .tables
                    .get_mut(&id(240))
                    .unwrap()
                    .waiting_exclusive
                    .insert(wait_id);
                state.waiters.insert(
                    wait_id,
                    super::FenceWaiter {
                        mode: super::FenceMode::Exclusive,
                        table_ids: vec![id(240)],
                        started_at: std::time::Instant::now(),
                    },
                );
            }
        }
        assert!(matches!(
            bounded.acquire_shared(&[id(240)], Duration::ZERO),
            Err(DdlFenceError::TooManyWaiters)
        ));

        let page = DdlFenceManager::new();
        {
            let mut state = page.lock_state();
            for value in 1..=super::MAX_DDL_FENCE_WAIT_PAGE + 1 {
                let wait_id = [u8::try_from(value).unwrap(); 16];
                state.waiters.insert(
                    wait_id,
                    super::FenceWaiter {
                        mode: super::FenceMode::Shared,
                        table_ids: vec![id(242)],
                        started_at: std::time::Instant::now(),
                    },
                );
            }
        }
        let (waits, truncated) = page.wait_snapshot();
        assert_eq!(waits.len(), super::MAX_DDL_FENCE_WAIT_PAGE);
        assert!(truncated);
    }
}
