use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

struct MeasuredAllocator;

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

// SAFETY: all allocation operations delegate to System with the original
// layout and pointer. Thread-local accounting does not change ownership.
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's layout preconditions are inherited unchanged.
        let ptr = unsafe { System.alloc(layout) };
        count_allocation(ptr);
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the valid incoming layout is passed unchanged.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        count_allocation(ptr);
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the caller owns the System allocation with this layout.
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        count_allocation(ptr);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the original pointer and layout match the System allocation.
        unsafe { System.dealloc(ptr, layout) };
    }
}

fn count_allocation(ptr: *mut u8) {
    if !ptr.is_null() {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get().wrapping_add(1)));
    }
}

pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, Duration, u64) {
    let before = ALLOCATIONS.get();
    let start = Instant::now();
    let value = operation();
    (
        value,
        start.elapsed(),
        ALLOCATIONS.get().wrapping_sub(before),
    )
}

pub fn p99_us<const N: usize>(samples: &mut [Duration; N]) -> f64 {
    samples.sort_unstable();
    samples[(N * 99).div_ceil(100) - 1].as_secs_f64() * 1e6
}
