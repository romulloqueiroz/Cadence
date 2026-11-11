//! Fixed per-voice page storage. Only the disk thread touches files/decoders.
//!
//! One audio-thread owner publishes requests and consumes READY buffers. The
//! disk thread changes REQUESTED -> FILLING -> READY/FAILED. It never changes
//! READY, so the audio owner can read or retire that buffer without a lock.
use nk_format::{audio, stream::SampleReader};
use std::{
    cell::{Cell, UnsafeCell},
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const PAGE_FRAMES: usize = 8192;
const PAGES_PER_VOICE: usize = 4;
const EMPTY: u8 = 0;
const REQUESTED: u8 = 1;
const FILLING: u8 = 2;
const READY: u8 = 3;
const FAILED: u8 = 4;

#[derive(Debug, Clone, Copy)]
pub struct StreamingConfig {
    /// Resident frames at every zone's start (rounded up to whole pages).
    pub preload_frames: usize,
    /// Hard limit on decoded resident pages. Loading fails if exceeded.
    pub max_preload_bytes: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            preload_frames: 16_384,
            max_preload_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StreamingStats {
    pub resident_bytes: usize,
    pub page_cache_bytes: usize,
    pub cache_ready_bytes: usize,
    pub peak_cache_ready_bytes: usize,
    pub pending_reads: u32,
    pub read_count: u64,
    pub read_total_ns: u64,
    pub last_read_ns: u64,
    pub max_read_ns: u64,
    pub pages_read: u64,
    pub read_errors: u64,
    /// Output voice frames with either interpolation tap missing.
    pub underrun_frames: u64,
    pub underrun_blocks: u64,
}

pub(crate) struct PreloadPage {
    pub start: usize,
    pub data: Box<[f32]>,
}

pub(crate) fn stereo_page(sample: &audio::Sample) -> Box<[f32]> {
    let mut data = vec![0.0; PAGE_FRAMES * 2];
    for frame in 0..sample.num_frames.min(PAGE_FRAMES) {
        data[2 * frame] = sample.samples[0][frame];
        data[2 * frame + 1] = sample.samples[sample.channels.min(2) - 1][frame];
    }
    data.into_boxed_slice()
}

struct Slot {
    state: AtomicU8,
    sample: AtomicUsize,
    start: AtomicUsize,
    data: UnsafeCell<Box<[f32]>>,
}

// SAFETY: the sole audio owner writes metadata before releasing REQUESTED.
// Only the disk worker writes data after acquiring REQUESTED, and publishes
// those writes with READY release. Only audio reads READY; the worker cannot
// mutate it until audio retires it and publishes a subsequent request. FILLING
// slots are never retired/reassigned, including on reset and voice stealing.
unsafe impl Sync for Slot {}

impl Slot {
    fn matches(&self, sample: usize, start: usize) -> bool {
        self.sample.load(Ordering::Relaxed) == sample && self.start.load(Ordering::Relaxed) == start
    }
}

struct Shared {
    slots: Vec<Slot>,
    stop: AtomicBool,
    pages_read: AtomicU64,
    read_errors: AtomicU64,
    delay_us: AtomicU64,
    timing_sequence: AtomicU64,
    ready_slots: AtomicU64,
    peak_ready_slots: AtomicU64,
    read_count: AtomicU64,
    read_total_ns: AtomicU64,
    last_read_ns: AtomicU64,
    max_read_ns: AtomicU64,
}

pub(crate) struct StreamEngine {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    offline: bool,
    offline_timed_out: bool,
    offline_timeouts: u64,
    last_timing: Cell<ReadTiming>,
    last_peak_ready: Cell<u64>,
}

impl StreamEngine {
    pub fn new(readers: Vec<SampleReader>, voices: usize) -> std::io::Result<Self> {
        Self::with_delay(readers, voices, Duration::ZERO)
    }

    fn with_delay(
        mut readers: Vec<SampleReader>,
        voices: usize,
        delay: Duration,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            slots: (0..voices * PAGES_PER_VOICE)
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
            delay_us: AtomicU64::new(delay.as_micros() as u64),
            timing_sequence: AtomicU64::new(0),
            ready_slots: AtomicU64::new(0),
            peak_ready_slots: AtomicU64::new(0),
            read_count: AtomicU64::new(0),
            read_total_ns: AtomicU64::new(0),
            last_read_ns: AtomicU64::new(0),
            max_read_ns: AtomicU64::new(0),
        });
        let disk = shared.clone();
        let worker = thread::Builder::new()
            .name("Cadence sample disk".into())
            .spawn(move || {
                while !disk.stop.load(Ordering::Relaxed) {
                    let mut worked = false;
                    // Round robin: one request per voice before servicing its next
                    // lookahead, so a chord cannot starve behind one voice's tail.
                    for voice in disk.slots.chunks(PAGES_PER_VOICE) {
                        if disk.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let Some(slot) = voice.iter().find(|s| {
                            s.state
                                .compare_exchange(
                                    REQUESTED,
                                    FILLING,
                                    Ordering::Acquire,
                                    Ordering::Relaxed,
                                )
                                .is_ok()
                        }) else {
                            continue;
                        };
                        worked = true;
                        let read_started = Instant::now();
                        let until = read_started
                            + Duration::from_micros(disk.delay_us.load(Ordering::Relaxed));
                        while Instant::now() < until {
                            if disk.stop.load(Ordering::Relaxed) {
                                return;
                            }
                            thread::sleep(Duration::from_millis(1));
                        }
                        let result = readers[slot.sample.load(Ordering::Relaxed)]
                            .read_frames(slot.start.load(Ordering::Relaxed), PAGE_FRAMES);
                        match result {
                            Ok(sample) => {
                                // SAFETY: FILLING grants exclusive worker access.
                                let data = unsafe { &mut *slot.data.get() };
                                data.fill(0.0);
                                for frame in 0..sample.num_frames {
                                    data[frame * 2] = sample.samples[0][frame];
                                    data[frame * 2 + 1] =
                                        sample.samples[sample.channels.min(2) - 1][frame];
                                }
                                let ready = disk.ready_slots.fetch_add(1, Ordering::Relaxed) + 1;
                                disk.peak_ready_slots.fetch_max(ready, Ordering::Relaxed);
                                record_read(&disk, read_started, true);
                                slot.state.store(READY, Ordering::Release);
                            }
                            Err(_) => {
                                record_read(&disk, read_started, false);
                                slot.state.store(FAILED, Ordering::Release);
                            }
                        }
                    }
                    // Audio publishes atomic requests only, with no syscall, lock,
                    // allocation, or wake primitive. Idle discovery is <=1ms plus
                    // OS scheduling; resident attacks cover that normal latency.
                    if !worked {
                        thread::park_timeout(Duration::from_millis(1));
                    }
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
            offline: false,
            offline_timed_out: false,
            offline_timeouts: 0,
            last_timing: Cell::new(ReadTiming::default()),
            last_peak_ready: Cell::new(0),
        })
    }

    pub fn set_offline(&mut self, offline: bool) {
        if offline != self.offline {
            self.offline_timed_out = false;
        }
        self.offline = offline;
    }

    #[cfg(test)]
    pub fn set_read_delay(&self, delay: Duration) {
        self.shared
            .delay_us
            .store(delay.as_micros() as u64, Ordering::Relaxed);
    }

    pub fn plan(&mut self, voice: usize, sample: usize, pages: &[Option<usize>; 4]) {
        let slots = &self.shared.slots[voice * PAGES_PER_VOICE..(voice + 1) * PAGES_PER_VOICE];
        for slot in slots {
            let state = slot.state.load(Ordering::Acquire);
            if (state == READY || state == FAILED || state == REQUESTED)
                && !(slot.sample.load(Ordering::Relaxed) == sample
                    && pages.contains(&Some(slot.start.load(Ordering::Relaxed))))
            {
                if state == REQUESTED {
                    // The worker can win this race; then FILLING remains owned
                    // by it until completion. Never overwrite a filling request.
                    let _ = slot.state.compare_exchange(
                        REQUESTED,
                        EMPTY,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                } else {
                    retire(&self.shared, slot, state);
                }
            }
        }
        for start in pages.iter().flatten().copied() {
            Self::request(slots, sample, start);
        }
    }

    fn request(slots: &[Slot], sample: usize, start: usize) {
        if slots
            .iter()
            .any(|s| s.state.load(Ordering::Acquire) != EMPTY && s.matches(sample, start))
        {
            return;
        }
        if let Some(slot) = slots
            .iter()
            .find(|s| s.state.load(Ordering::Acquire) == EMPTY)
        {
            slot.sample.store(sample, Ordering::Relaxed);
            slot.start.store(start, Ordering::Relaxed);
            slot.state.store(REQUESTED, Ordering::Release);
        }
    }

    pub fn frame(&mut self, voice: usize, sample: usize, index: usize) -> Option<(f32, f32)> {
        if self.offline_timed_out {
            return None;
        }
        let start = index / PAGE_FRAMES * PAGE_FRAMES;
        let slots = &self.shared.slots[voice * PAGES_PER_VOICE..(voice + 1) * PAGES_PER_VOICE];
        if !slots
            .iter()
            .any(|s| s.state.load(Ordering::Acquire) != EMPTY && s.matches(sample, start))
            && !slots
                .iter()
                .any(|s| s.state.load(Ordering::Acquire) == EMPTY)
        {
            // A stolen voice's in-flight pages may complete mid-page, after
            // plan() ran. Reclaim a completed slot immediately in both modes.
            if let Some(slot) = slots
                .iter()
                .filter(|s| matches!(s.state.load(Ordering::Acquire), READY | FAILED))
                .max_by_key(|s| {
                    if s.sample.load(Ordering::Relaxed) == sample {
                        s.start.load(Ordering::Relaxed).abs_diff(start)
                    } else {
                        usize::MAX
                    }
                })
            {
                retire(&self.shared, slot, slot.state.load(Ordering::Acquire));
            }
        }
        Self::request(slots, sample, start);
        let deadline = self
            .offline
            .then(|| Instant::now() + Duration::from_secs(2));
        loop {
            if let Some(slot) = slots
                .iter()
                .find(|s| s.state.load(Ordering::Acquire) != EMPTY && s.matches(sample, start))
            {
                match slot.state.load(Ordering::Acquire) {
                    READY => {
                        // SAFETY: sole audio owner holds READY through this read.
                        let data = unsafe { &*slot.data.get() };
                        let offset = (index - start) * 2;
                        return Some((data[offset], data[offset + 1]));
                    }
                    FAILED => return None,
                    _ => {}
                }
            }
            match deadline {
                Some(deadline) if Instant::now() < deadline => {
                    // Only explicitly offline rendering may wait. Stale fills
                    // from a stolen voice can finish before an empty slot opens.
                    for slot in slots {
                        let state = slot.state.load(Ordering::Acquire);
                        if (state == READY || state == FAILED) && !slot.matches(sample, start) {
                            retire(&self.shared, slot, state);
                            break;
                        }
                    }
                    Self::request(slots, sample, start);
                    thread::sleep(Duration::from_micros(100));
                }
                Some(_) => {
                    self.offline_timed_out = true;
                    self.offline_timeouts = self.offline_timeouts.saturating_add(1);
                    return None;
                }
                None => return None,
            }
        }
    }

    pub fn has_error(&self) -> bool {
        self.offline_timeouts > 0 || self.shared.read_errors.load(Ordering::Relaxed) > 0
    }

    pub fn stats(&self) -> StreamingStats {
        let timing = read_timing(&self.shared).unwrap_or_else(|| self.last_timing.get());
        self.last_timing.set(timing);
        let ready = self.shared.ready_slots.load(Ordering::Relaxed);
        let peak = self
            .shared
            .peak_ready_slots
            .load(Ordering::Relaxed)
            .max(ready)
            .max(self.last_peak_ready.get());
        self.last_peak_ready.set(peak);
        StreamingStats {
            page_cache_bytes: self.shared.slots.len() * PAGE_FRAMES * 2 * size_of::<f32>(),
            cache_ready_bytes: ready as usize * PAGE_FRAMES * 2 * size_of::<f32>(),
            peak_cache_ready_bytes: peak as usize * PAGE_FRAMES * 2 * size_of::<f32>(),
            pending_reads: self
                .shared
                .slots
                .iter()
                .filter(|s| matches!(s.state.load(Ordering::Relaxed), REQUESTED | FILLING))
                .count() as u32,
            read_count: timing.read_count,
            read_total_ns: timing.read_total_ns,
            last_read_ns: timing.last_read_ns,
            max_read_ns: timing.max_read_ns,
            pages_read: timing.pages_read,
            // Failure count is sampled live even if the timing group is busy:
            // a terminal read error must be visible before the worker exits.
            read_errors: timing
                .read_errors
                .max(self.shared.read_errors.load(Ordering::Relaxed))
                .saturating_add(self.offline_timeouts),
            ..Default::default()
        }
    }
}

// Disk thread only. Includes open/seek/decode/copy and injected test delay;
// queue wait is excluded. These are page service times, not disk busy percent.
#[derive(Default, Clone, Copy)]
struct ReadTiming {
    pages_read: u64,
    read_errors: u64,
    read_count: u64,
    read_total_ns: u64,
    last_read_ns: u64,
    max_read_ns: u64,
}

fn record_read(shared: &Shared, started: Instant, success: bool) {
    let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    shared.timing_sequence.fetch_add(1, Ordering::SeqCst);
    if success {
        shared.pages_read.fetch_add(1, Ordering::SeqCst);
    } else {
        shared.read_errors.fetch_add(1, Ordering::SeqCst);
    }
    shared.read_total_ns.fetch_add(elapsed, Ordering::SeqCst);
    shared.last_read_ns.store(elapsed, Ordering::SeqCst);
    shared.max_read_ns.fetch_max(elapsed, Ordering::SeqCst);
    shared.read_count.fetch_add(1, Ordering::SeqCst);
    shared.timing_sequence.fetch_add(1, Ordering::SeqCst);
}

fn read_timing(shared: &Shared) -> Option<ReadTiming> {
    for _ in 0..3 {
        let before = shared.timing_sequence.load(Ordering::SeqCst);
        if before & 1 != 0 {
            continue;
        }
        let value = ReadTiming {
            pages_read: shared.pages_read.load(Ordering::SeqCst),
            read_errors: shared.read_errors.load(Ordering::SeqCst),
            read_count: shared.read_count.load(Ordering::SeqCst),
            read_total_ns: shared.read_total_ns.load(Ordering::SeqCst),
            last_read_ns: shared.last_read_ns.load(Ordering::SeqCst),
            max_read_ns: shared.max_read_ns.load(Ordering::SeqCst),
        };
        if before == shared.timing_sequence.load(Ordering::SeqCst) {
            return Some(value);
        }
    }
    None
}

fn retire(shared: &Shared, slot: &Slot, state: u8) {
    if state == READY {
        shared.ready_slots.fetch_sub(1, Ordering::Relaxed);
    }
    slot.state.store(EMPTY, Ordering::Relaxed);
}

impl Drop for StreamEngine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
#[path = "streaming_timing_tests.rs"]
mod streaming_timing_tests;
