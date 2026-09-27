use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

static TRACKING: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
pub struct Allocator;

fn account(requested: usize, released: usize, allocation: bool) {
    if !TRACKING.load(Ordering::Relaxed) {
        return;
    }
    CALLS.fetch_add(u64::from(allocation), Ordering::Relaxed);
    REQUESTED.fetch_add(requested as u64, Ordering::Relaxed);
    let delta = requested as i64 - released as i64;
    let current = LIVE.fetch_add(delta, Ordering::Relaxed) + delta;
    PEAK.fetch_max(current, Ordering::Relaxed);
}

// Benchmark-only allocator: forward every pointer and layout to System unchanged.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            account(layout.size(), 0, true);
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc_zeroed(layout);
        if !pointer.is_null() {
            account(layout.size(), 0, true);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        account(0, layout.size(), false);
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let updated = System.realloc(pointer, layout, size);
        if !updated.is_null() {
            account(size, layout.size(), true);
        }
        updated
    }
}

#[derive(Debug)]
pub struct AllocationStats {
    pub calls: u64,
    pub requested: u64,
    pub retained: i64,
    pub peak: i64,
}

/// The closure must only free allocations created inside this interval.
pub fn allocations<Result>(operation: impl FnOnce() -> Result) -> (Result, AllocationStats) {
    CALLS.store(0, Ordering::Relaxed);
    REQUESTED.store(0, Ordering::Relaxed);
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    TRACKING.store(true, Ordering::Relaxed);
    let result = operation();
    TRACKING.store(false, Ordering::Relaxed);
    (
        result,
        AllocationStats {
            calls: CALLS.load(Ordering::Relaxed),
            requested: REQUESTED.load(Ordering::Relaxed),
            retained: LIVE.load(Ordering::Relaxed),
            peak: PEAK.load(Ordering::Relaxed),
        },
    )
}

/// Each sample times one complete query, including its result consumption.
pub fn latency(name: &str, count: usize, mut operation: impl FnMut(usize)) {
    for sample in 0..32 {
        operation(sample);
    }
    let mut timings = Vec::with_capacity(count);
    for sample in 0..count {
        let started = Instant::now();
        operation(sample);
        timings.push(started.elapsed().as_nanos());
    }
    timings.sort_unstable();
    println!(
        "latency,{name},{count},{},{},0,0,0,0",
        timings[count / 2],
        timings[(count - 1) * 95 / 100]
    );
}

pub fn build<Result>(name: &str, mut operation: impl FnMut() -> Result) {
    let mut timings = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let result = std::hint::black_box(operation());
        timings.push(started.elapsed().as_nanos());
        drop(result);
    }
    timings.sort_unstable();
    let (result, stats) = allocations(operation);
    assert!(stats.retained >= 0, "measurement freed untracked memory");
    println!(
        "build,{name},5,{},0,{},{},{},{}",
        timings[2], stats.calls, stats.requested, stats.retained, stats.peak
    );
    drop(result);
}

pub fn query_allocations(name: &str, operation: impl FnOnce()) {
    let (_, stats) = allocations(operation);
    assert_eq!(
        stats.retained, 0,
        "query results must be dropped within the interval"
    );
    println!(
        "allocation,{name},1,0,0,{},{},0,{}",
        stats.calls, stats.requested, stats.peak
    );
}
