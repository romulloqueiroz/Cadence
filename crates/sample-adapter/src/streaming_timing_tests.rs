//! Tests the disk/audio diagnostics boundary without filesystem scheduling.
use super::*;
use std::sync::atomic::AtomicBool;

// No disk worker: tests explicitly drive its publication points, including
// pauses inside them that normal file-based tests cannot reliably schedule.
fn engine(slot_count: usize) -> StreamEngine {
    StreamEngine {
        shared: Arc::new(Shared {
            slots: (0..slot_count)
                .map(|_| Slot {
                    state: AtomicU8::new(EMPTY),
                    sample: AtomicUsize::new(0),
                    start: AtomicUsize::new(0),
                    data: UnsafeCell::new(vec![0.0; PAGE_FRAMES * 2].into_boxed_slice()),
                })
                .collect(),
            stop: AtomicBool::new(false),
            pages_read: AtomicU64::new(0),
            read_errors: AtomicU64::new(0),
            delay_us: AtomicU64::new(0),
            timing_sequence: AtomicU64::new(0),
            ready_slots: AtomicU64::new(0),
            peak_ready_slots: AtomicU64::new(0),
            read_count: AtomicU64::new(0),
            read_total_ns: AtomicU64::new(0),
            last_read_ns: AtomicU64::new(0),
            max_read_ns: AtomicU64::new(0),
        }),
        worker: None,
        offline: false,
        offline_timed_out: false,
        offline_timeouts: 0,
        last_timing: Cell::new(ReadTiming::default()),
        last_peak_ready: Cell::new(0),
    }
}

fn timing_tuple(stats: StreamingStats) -> (u64, u64, u64, u64, u64) {
    (
        stats.pages_read,
        stats.read_count,
        stats.read_total_ns,
        stats.last_read_ns,
        stats.max_read_ns,
    )
}
fn assert_timing(stats: StreamingStats) {
    // Public error counts deliberately include live disk errors/offline
    // timeouts, independently of the last completed timing publication.
    assert!(stats.read_count >= stats.pages_read, "{stats:?}");
    assert!(stats.max_read_ns >= stats.last_read_ns, "{stats:?}");
    assert!(stats.read_total_ns >= stats.max_read_ns, "{stats:?}");
}
fn assert_internal_timing(timing: ReadTiming) {
    assert_eq!(timing.read_count, timing.pages_read + timing.read_errors);
    assert!(timing.max_read_ns >= timing.last_read_ns);
    assert!(timing.read_total_ns >= timing.max_read_ns);
}

#[test]
fn concurrent_disk_timing_snapshots_preserve_all_counter_invariants() {
    let engine = engine(0);
    let disk = engine.shared.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let publisher = thread::spawn(move || {
        for index in 0..50_000 {
            let started = Instant::now() - Duration::from_nanos(1_000 * (index % 37 + 1));
            record_read(&disk, started, index % 5 != 0);
        }
        done.store(true, Ordering::SeqCst);
    });
    let mut previous_count = 0;
    while !finished.load(Ordering::SeqCst) {
        if let Some(timing) = read_timing(&engine.shared) {
            assert_internal_timing(timing);
        }
        let stats = engine.stats();
        assert_timing(stats);
        assert!(
            stats.read_count >= previous_count,
            "a cached snapshot went backwards"
        );
        previous_count = stats.read_count;
    }
    publisher.join().unwrap();
    let stats = engine.stats();
    assert_timing(stats);
    assert_eq!(stats.read_count, 50_000);
    assert_eq!(stats.pages_read, 40_000);
    assert_eq!(stats.read_errors, 10_000);
    assert_internal_timing(read_timing(&engine.shared).unwrap());
    assert!(stats.last_read_ns > 0);
}

#[test]
fn busy_timing_writer_returns_cached_snapshot_without_waiting() {
    let engine = engine(0);
    record_read(
        &engine.shared,
        Instant::now() - Duration::from_micros(10),
        true,
    );
    record_read(
        &engine.shared,
        Instant::now() - Duration::from_micros(20),
        false,
    );
    let before = engine.stats();
    assert_timing(before);
    engine.shared.timing_sequence.fetch_add(1, Ordering::SeqCst);
    // A writer paused forever after partially changing every payload field.
    engine.shared.pages_read.store(123, Ordering::SeqCst);
    engine.shared.read_errors.store(456, Ordering::SeqCst);
    engine.shared.read_count.store(999, Ordering::SeqCst);
    engine.shared.read_total_ns.store(1, Ordering::SeqCst);
    engine.shared.last_read_ns.store(u64::MAX, Ordering::SeqCst);
    engine.shared.max_read_ns.store(0, Ordering::SeqCst);
    assert!(read_timing(&engine.shared).is_none());
    assert!(engine.has_error());
    assert_eq!(
        engine.stats().read_errors,
        456,
        "terminal errors must stay live during a busy timing snapshot"
    );
    for _ in 0..100 {
        assert_eq!(timing_tuple(engine.stats()), timing_tuple(before));
    }
    // The odd generation is deliberately never completed. Returning from this
    // test establishes that readers cannot spin until a publisher finishes.
}

#[test]
fn busy_first_snapshot_defaults_to_zero_and_offline_timeouts_remain_separate() {
    let mut engine = engine(0);
    engine.shared.timing_sequence.store(1, Ordering::SeqCst);
    engine
        .shared
        .read_total_ns
        .store(u64::MAX, Ordering::SeqCst);
    assert_eq!(timing_tuple(engine.stats()), (0, 0, 0, 0, 0));
    engine.offline_timeouts = 2;
    let stats = engine.stats();
    assert_eq!(stats.read_errors, 2);
    assert_eq!(stats.read_count, 0);
    assert_eq!(stats.pages_read, 0);
    assert!(engine.has_error());
}

#[test]
fn ready_peak_does_not_decrease_when_disk_pauses_before_peak_publication() {
    let engine = engine(PAGES_PER_VOICE);
    let bytes = PAGE_FRAMES * 2 * size_of::<f32>();
    let slot = &engine.shared.slots[0];
    // One completed page, then a second page whose disk-side ready count has
    // advanced but whose peak/state publication has not completed yet.
    slot.state.store(READY, Ordering::Release);
    engine.shared.ready_slots.store(2, Ordering::Relaxed);
    engine.shared.peak_ready_slots.store(1, Ordering::Relaxed);
    let before = engine.stats();
    assert_eq!(before.cache_ready_bytes, 2 * bytes);
    assert_eq!(before.peak_cache_ready_bytes, 2 * bytes);
    // The audio owner can retire the first page while the disk is still paused.
    retire(&engine.shared, slot, READY);
    let after = engine.stats();
    assert_eq!(after.cache_ready_bytes, bytes);
    assert!(
        after.peak_cache_ready_bytes >= before.peak_cache_ready_bytes,
        "lifetime peak decreased: before={before:?}, after={after:?}"
    );
    assert!(after.peak_cache_ready_bytes <= after.page_cache_bytes);
}

#[test]
fn retiring_and_cancelling_pages_preserves_ready_capacity_bounds() {
    let mut engine = engine(PAGES_PER_VOICE);
    for _ in 0..100 {
        let slot = &engine.shared.slots[0];
        let ready = engine.shared.ready_slots.fetch_add(1, Ordering::Relaxed) + 1;
        engine
            .shared
            .peak_ready_slots
            .fetch_max(ready, Ordering::Relaxed);
        slot.state.store(READY, Ordering::Release);
        let stats = engine.stats();
        assert!(stats.cache_ready_bytes <= stats.peak_cache_ready_bytes);
        assert!(stats.peak_cache_ready_bytes <= stats.page_cache_bytes);
        retire(&engine.shared, slot, READY);
        assert_eq!(engine.stats().cache_ready_bytes, 0);
        slot.state.store(FAILED, Ordering::Release);
        retire(&engine.shared, slot, FAILED);
        assert_eq!(engine.stats().cache_ready_bytes, 0);
    }
    engine.shared.slots[0]
        .state
        .store(REQUESTED, Ordering::Release);
    engine.shared.slots[1]
        .state
        .store(FILLING, Ordering::Release);
    engine.plan(0, 0, &[None; PAGES_PER_VOICE]);
    assert_eq!(engine.shared.slots[0].state.load(Ordering::Acquire), EMPTY);
    assert_eq!(
        engine.shared.slots[1].state.load(Ordering::Acquire),
        FILLING
    );
    assert_eq!(engine.stats().cache_ready_bytes, 0);
    assert_eq!(engine.stats().pending_reads, 1);
}
