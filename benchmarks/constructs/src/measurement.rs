use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const BATCH_COUNT: usize = 31;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

pub struct CountingAllocator;

fn record_allocation(size: usize) {
    if COUNTING.load(Ordering::Relaxed) {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }
}

// Benchmark-only wrapper. Every request is forwarded with its original pointer/layout.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation(size);
        System.realloc(pointer, layout, size)
    }
}

/// Reports batch-average timings and a separate allocation-count pass.
pub fn measure(name: &str, iterations: usize, mut operation: impl FnMut()) {
    for _ in 0..iterations.min(1_000) {
        operation();
    }
    let mut samples = Vec::with_capacity(BATCH_COUNT);
    for _ in 0..BATCH_COUNT {
        let started = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        samples.push(started.elapsed().as_nanos() as f64 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    let allocation_samples = iterations.clamp(1, 100);
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    REQUESTED_BYTES.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    for _ in 0..allocation_samples {
        operation();
    }
    COUNTING.store(false, Ordering::Relaxed);
    println!(
        "{name},{iterations},{BATCH_COUNT},{:.2},{:.2},{:.2},{:.2}",
        samples[BATCH_COUNT / 2],
        samples[((BATCH_COUNT - 1) as f64 * 0.95).round() as usize],
        ALLOCATION_CALLS.load(Ordering::Relaxed) as f64 / allocation_samples as f64,
        REQUESTED_BYTES.load(Ordering::Relaxed) as f64 / allocation_samples as f64,
    );
}
