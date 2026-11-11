# Native CLAP editor (worker-hosted)

The native CLAP editor lives in a floating Cocoa window owned by the plugin
worker. The Cadence Inspector supplies an Open/Close native editor button and
continues to expose generic parameter controls.

## Threading and lifecycle

All CLAP workers, including plugins without a Cocoa editor, keep their creator
thread available for host requests. On macOS this is the actual process main
thread. Plugin callbacks, activation/deactivation, state operations, parameter
value queries and destruction run there. The audio thread owns process,
start/stop/reset and active parameter flushes. Creator-thread dispatch is used
only by non-realtime lifecycle/state operations; `process()` never queues or
waits for main-thread work.

The main loop pumps NSApplication mouse/key events as well as CFRunLoop sources,
with an autorelease pool per turn and bounded task/event batches. Parameter and
timing feedback travels through atomic caches; the audio worker does not call
main-thread getters. See [host callbacks](native-clap-adapter.md).

The native title-bar close button defers hide/destroy until Cocoa finishes
handling the action. Both host and native closes use one destruction path.
Cadence polls the actual worker window state, so its button also updates after a
native close or Inspector recreation. The public C bridge uses 0=close, 1=open;
public ABI 2 and private shared-memory layout 11 are unchanged.

A shutdown acknowledgement prevents plugin destruction/dlclose until its native
view has been closed. Windows are retained by Rust, with Cocoa's automatic
release-on-close disabled to avoid a second release.

## Verification

`cargo build -p cadence-test-plugin` builds the deterministic fixture.
`cargo test -p plugin-sandbox --test clap_main_thread --offline` runs on the real
OS main thread without shared-memory IPC. It exercises editor/headless modes,
idle and active flushes, callback delivery, same-instance restart, structural
restart quiescence, state save/load, native performClose/reopen and zero Rust
allocations during process/request callbacks.

The worker GUI test checks open/close/reopen through shared memory. The C++
bridge test also checks the Inspector button, native close feedback and Inspector
recreation. These IPC checks currently cannot execute in the restricted shell:
macOS denies shm_open with EPERM. They compile; do not count them as passing.
Evidence: `app/build/verification/clap-host-requests-2026-11-06/report.json`.

## Remaining editor work

- The window uses the initial size reported by the plugin. Host GUI resize,
  show/hide-request extensions and interactive resizing are not implemented.
- No new third-party mouse/keyboard interaction session was performed. The
  focused test invokes the same NSWindow close action used by the title bar.
- Native AU/VST3 editors remain in-process; the CLAP editor is a separate window,
  rather than an embedded Inspector view.
