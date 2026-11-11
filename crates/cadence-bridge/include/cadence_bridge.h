#ifndef CADENCE_BRIDGE_H
#define CADENCE_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define CADENCE_BRIDGE_ABI 2
#define CADENCE_MAX_BLOCK 4096
#define CADENCE_MAX_CHANNELS 8
#define CADENCE_MAX_EVENTS 256
#define CADENCE_MAX_STATE 65536
#define CADENCE_MAX_OUTPUT_SYSEX 16384

typedef struct CadenceBridge CadenceBridge;
typedef struct {
    double sample_rate;
    uint32_t block_size;
    double tempo;
    uint32_t flags; /* bit 0: playing; bit 1: offline/non-real-time rendering */
    int64_t sample_pos; /* Signed DAW timeline, including preroll before zero. */
    double ppq_position;
} CadenceContext;
typedef struct {
    uint32_t frame, kind; /* note on/off/pressure/CC/program/bend/channel pressure/parameter: 0..7 */
    uint16_t channel, data1, data2, padding;
    float value;
} CadenceEvent;
uint32_t cadence_bridge_abi(void);
/* Non-real-time. Source is .dylib, .clap, .nki/.nkm, signed manifest.json,
   or builtin:sine/builtin:gain. Selector optionally chooses a CLAP plugin ID.
   All strings are UTF-8. The caller owns pointers and buffer lifetimes. */
CadenceBridge* cadence_bridge_open(const char* worker, const char* source, const char* selector,
                                  char* error, size_t error_capacity);
/* Loading thread only. The callback/context must remain valid for the call.
   Cancellation kills/reaps a starting worker; no detached load survives it. */
typedef int32_t (*CadenceCancelCheck)(void* context);
CadenceBridge* cadence_bridge_open_cancellable(const char* worker, const char* source, const char* selector,
    char* error, size_t error_capacity, CadenceCancelCheck, void* context);
int32_t cadence_bridge_prepare_cancellable(CadenceBridge*, double sample_rate, uint32_t max_block,
    CadenceCancelCheck, void* context);
int32_t cadence_bridge_load_cancellable(CadenceBridge*, const uint8_t*, size_t length, uint32_t schema,
    CadenceCancelCheck, void* context);
void cadence_bridge_close(CadenceBridge*);
/* Immutable JSON; returns required capacity INCLUDING the terminating null. */
size_t cadence_bridge_metadata(const CadenceBridge*, char* output, size_t capacity);
int32_t cadence_bridge_prepare(CadenceBridge*, double sample_rate, uint32_t max_block);
int32_t cadence_bridge_reset(CadenceBridge*);
/* Audio thread only. All lifecycle/state operations must be serialized against
   this call. Distinct planar input/output arrays, channel stride = frames.
   No allocation, locks, pipes, process creation/destruction in the callback.
   Real-time exchange never waits. Late output becomes silence; stale results
   are discarded. Offline flag permits synchronous rendering with the same delay.
   In real time frames must not exceed the prepared maximum. */
int32_t cadence_bridge_process(CadenceBridge*, const CadenceContext*,
    const float* inputs, uint32_t input_channels, float* outputs, uint32_t output_channels,
    uint32_t frames, const CadenceEvent*, uint32_t event_count);
/* Additive sidechain-capable variant; the original remains unchanged. */
int32_t cadence_bridge_process_sidechain(CadenceBridge*, const CadenceContext*,
    const float* inputs, uint32_t input_channels,
    const float* sidechain, uint32_t sidechain_channels,
    float* outputs, uint32_t output_channels,
    uint32_t frames, const CadenceEvent*, uint32_t event_count);
/* Take one completed packet's output MIDI/SysEx group. Call after processing on
   the same serialized audio/control thread. kind 9 is SysEx: data1=offset into
   the SysEx buffer, data2=length. Returns 0, or -1. */
int32_t cadence_bridge_take_output_midi(CadenceBridge*, CadenceEvent* events,
    uint32_t event_capacity, uint8_t* sysex, uint32_t sysex_capacity,
    uint32_t* event_count, uint32_t* sysex_length);
typedef struct {
    uint64_t underruns, missed_frames, pending_packets;
    uint32_t latency_frames, scheduling_flags; /* bit 0: time-constraint accepted; bit 1: host Audio Workgroup joined */
} CadencePipelineStats;
int32_t cadence_bridge_pipeline_stats(const CadenceBridge*, CadencePipelineStats*);
/* Additive dynamic plugin timing. Latency/tail are the plugin's current values;
   pipeline latency remains in CadencePipelineStats. Thread-safe atomic read. */
typedef struct {
    uint32_t plugin_latency_frames, plugin_tail_frames;
} CadenceDynamicStats;
int32_t cadence_bridge_dynamic_stats(const CadenceBridge*, CadenceDynamicStats*);
/* Plugin-to-host parameter value feedback. Generation is a seqlock counter.
   Snapshot returns the number of entries written into caller buffers, or -1.
   IDs are the same stable IDs exposed in metadata; values are normalized. */
uint64_t cadence_bridge_parameter_generation(const CadenceBridge*);
/* 1 when the plugin requested a structural parameter/port reload. */
uint32_t cadence_bridge_reload_requested(const CadenceBridge*);
/* 0 close, 1 open. Non-real-time UI request for a worker-owned native editor. */
int32_t cadence_bridge_set_gui_request(CadenceBridge*, uint32_t request);
/* Returns bit0 ready, bit1 open. Thread-safe atomic read. */
uint32_t cadence_bridge_gui_state(const CadenceBridge*);
int32_t cadence_bridge_parameter_snapshot(const CadenceBridge*, uint32_t* ids, float* values,
    uint32_t capacity, uint64_t* generation);
/* Non-real-time control path. `workgroup` is a borrowed os_workgroup_t handle
   from the host; null removes availability. `generation` must increase for each
   change. macOS only; returns 0 when published, -1 on unsupported/failure. */
int32_t cadence_bridge_set_workgroup(CadenceBridge*, void* workgroup, uint32_t generation);
/* Pipeline counters exclude streaming misses. Thread-safe, bounded atomic read.
   flags bit0 = streaming available. A nonstreaming/busy snapshot returns zeros.
   Timing ns = page service time (open/read/decode/copy), excludes request queue.
   Missing frames count output voice frames with either interpolation tap missing.
   Counters/peaks last for the loaded worker, including reset/offline renders. */
typedef struct {
    uint64_t resident_bytes, cache_capacity_bytes, cache_ready_bytes, peak_cache_ready_bytes;
    uint64_t pages_read, read_errors, underrun_frames, underrun_blocks;
    uint64_t read_count, read_total_ns, last_read_ns, max_read_ns;
    uint32_t active_voices, voice_capacity, pending_reads, flags;
} CadenceStreamingStats;
int32_t cadence_bridge_streaming_stats(const CadenceBridge*, CadenceStreamingStats*);
/* Non-real-time. State save returns bytes written, or -1; never truncates. */
int32_t cadence_bridge_save(CadenceBridge*, uint8_t*, size_t capacity, uint32_t* schema);
int32_t cadence_bridge_load(CadenceBridge*, const uint8_t*, size_t length, uint32_t schema);
/* Thread-safe status query, with no access to mutable plugin state. */
uint32_t cadence_bridge_failed(const CadenceBridge*);
uint32_t cadence_bridge_worker_id(const CadenceBridge*);
/* First failure: 0 none, 1 invalid audio, 2 queue overflow, 3 protocol,
   4 notification, 5 audio timeout, 6 command timeout, 7 drain timeout,
   8 worker exit, 9 pipeline timeout, 10 worker-reported error, 11 cancellation,
   12 startup. Thread-safe; detailed records are written off the audio thread. */
uint32_t cadence_bridge_failure_reason(const CadenceBridge*);
#ifdef __cplusplus
}
static_assert(sizeof(CadenceContext) == 48, "CadenceContext ABI");
static_assert(sizeof(CadencePipelineStats) == 32, "CadencePipelineStats ABI");
static_assert(sizeof(CadenceStreamingStats) == 112, "CadenceStreamingStats ABI");
static_assert(sizeof(CadenceEvent) == 20, "CadenceEvent ABI");
#endif
#endif
