//! Coherent, bounded snapshots of worker-owned streaming diagnostics.
//!
//! The worker is the sole publisher. Every payload member is atomic, so reading
//! during publication is always data-race free. A sequentially consistent
//! sequence counter brackets each publication; readers accept only an unchanged
//! even generation. They try at most three times, returning `None` if the worker
//! is busy (or has died mid-publication), rather than waiting or allocating.
//!
//! `None` also represents an instrument without streaming telemetry. Consumers
//! may retain their last successful snapshot for display, but must not infer a
//! zero-valued counter from a skipped snapshot. Counters are read-only here;
//! sampling never resets them. This is separate from pipeline dropout counters.
use plugin_api::StreamingTelemetry;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const SNAPSHOT_ATTEMPTS: usize = 3;
const AVAILABLE: u32 = 1;

#[repr(C)]
pub(crate) struct AtomicStreamingTelemetry {
    sequence: AtomicU64,
    values: [AtomicU64; 12],
    counts: [AtomicU32; 4],
}

impl Default for AtomicStreamingTelemetry {
    fn default() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            values: std::array::from_fn(|_| AtomicU64::new(0)),
            counts: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl AtomicStreamingTelemetry {
    /// Sole worker publisher only. No locks, waits, heap allocation, or counter
    /// resets. The option controls availability, independently of input flags.
    pub(crate) fn publish(&self, telemetry: Option<StreamingTelemetry>) {
        let available = telemetry.is_some();
        let telemetry = telemetry.unwrap_or_default();
        self.sequence.fetch_add(1, Ordering::SeqCst);
        for (target, value) in self.values.iter().zip([
            telemetry.resident_bytes,
            telemetry.cache_capacity_bytes,
            telemetry.cache_ready_bytes,
            telemetry.peak_cache_ready_bytes,
            telemetry.pages_read,
            telemetry.read_errors,
            telemetry.underrun_frames,
            telemetry.underrun_blocks,
            telemetry.read_count,
            telemetry.read_total_ns,
            telemetry.last_read_ns,
            telemetry.max_read_ns,
        ]) {
            target.store(value, Ordering::SeqCst);
        }
        for (target, value) in self.counts.iter().zip([
            telemetry.active_voices,
            telemetry.voice_capacity,
            telemetry.pending_reads,
            if available {
                telemetry.flags | AVAILABLE
            } else {
                0
            },
        ]) {
            target.store(value, Ordering::SeqCst);
        }
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn snapshot(&self) -> Option<StreamingTelemetry> {
        for _ in 0..SNAPSHOT_ATTEMPTS {
            let before = self.sequence.load(Ordering::SeqCst);
            if before & 1 != 0 {
                continue;
            }
            let values: [u64; 12] =
                std::array::from_fn(|index| self.values[index].load(Ordering::SeqCst));
            let counts: [u32; 4] =
                std::array::from_fn(|index| self.counts[index].load(Ordering::SeqCst));
            let after = self.sequence.load(Ordering::SeqCst);
            if before == after && after & 1 == 0 {
                return (counts[3] & AVAILABLE != 0).then_some(StreamingTelemetry {
                    resident_bytes: values[0],
                    cache_capacity_bytes: values[1],
                    cache_ready_bytes: values[2],
                    peak_cache_ready_bytes: values[3],
                    pages_read: values[4],
                    read_errors: values[5],
                    underrun_frames: values[6],
                    underrun_blocks: values[7],
                    read_count: values[8],
                    read_total_ns: values[9],
                    last_read_ns: values[10],
                    max_read_ns: values[11],
                    active_voices: counts[0],
                    voice_capacity: counts[1],
                    pending_reads: counts[2],
                    flags: counts[3],
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};
    fn sample(value: u64) -> StreamingTelemetry {
        StreamingTelemetry {
            resident_bytes: value,
            cache_capacity_bytes: value,
            cache_ready_bytes: value,
            peak_cache_ready_bytes: value,
            pages_read: value,
            read_errors: value,
            underrun_frames: value,
            underrun_blocks: value,
            read_count: value,
            read_total_ns: value,
            last_read_ns: value,
            max_read_ns: value,
            active_voices: value as u32,
            voice_capacity: value as u32,
            pending_reads: value as u32,
            flags: AVAILABLE,
        }
    }
    #[test]
    fn unavailable_zero_and_full_width_values_round_trip() {
        let storage = AtomicStreamingTelemetry::default();
        assert_eq!(storage.snapshot(), None);
        storage.publish(Some(StreamingTelemetry::default()));
        assert_eq!(
            storage.snapshot(),
            Some(StreamingTelemetry {
                flags: AVAILABLE,
                ..Default::default()
            })
        );
        storage.publish(Some(sample(u64::MAX)));
        assert_eq!(storage.snapshot(), Some(sample(u64::MAX)));
        storage.publish(None);
        assert_eq!(storage.snapshot(), None);
        assert!(storage
            .values
            .iter()
            .all(|value| value.load(Ordering::SeqCst) == 0));
        assert!(storage
            .counts
            .iter()
            .all(|value| value.load(Ordering::SeqCst) == 0));
    }
    #[test]
    fn busy_or_crashed_writer_never_blocks_a_reader() {
        let storage = AtomicStreamingTelemetry::default();
        storage.publish(Some(sample(42)));
        storage.sequence.fetch_add(1, Ordering::SeqCst);
        assert_eq!(storage.snapshot(), None);
        // An odd generation remains unavailable regardless of partial writes.
        storage.values[0].store(123, Ordering::SeqCst);
        assert_eq!(storage.snapshot(), None);
    }
    #[test]
    fn generation_wrap_preserves_completed_publication() {
        let storage = AtomicStreamingTelemetry::default();
        storage.sequence.store(u64::MAX - 1, Ordering::SeqCst);
        storage.publish(Some(sample(42)));
        assert_eq!(storage.sequence.load(Ordering::SeqCst), 0);
        assert_eq!(storage.snapshot(), Some(sample(42)));
    }
    #[test]
    fn concurrent_snapshots_never_mix_publications() {
        let storage = Arc::new(AtomicStreamingTelemetry::default());
        storage.publish(Some(sample(0)));
        let finished = Arc::new(AtomicBool::new(false));
        let worker = storage.clone();
        let signal = finished.clone();
        let publisher = std::thread::spawn(move || {
            for value in 1..100_000 {
                worker.publish(Some(sample(value)));
            }
            signal.store(true, Ordering::SeqCst);
        });
        let mut accepted = 0;
        while !finished.load(Ordering::SeqCst) {
            if let Some(value) = storage.snapshot() {
                assert_eq!(value, sample(value.resident_bytes));
                accepted += 1;
            }
        }
        publisher.join().unwrap();
        assert_eq!(storage.snapshot(), Some(sample(99_999)));
        // A completed snapshot is always available after publication stops.
        accepted += usize::from(storage.snapshot().is_some());
        assert!(accepted > 0);
    }
}
