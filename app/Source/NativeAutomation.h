#pragma once

// Project-owned hook implemented by in-process native hosts that can accept
// timed parameter changes. The automation controller uses this only when a
// hosted target is not a Cadence Rust worker.
namespace cadence
{
class NativeAutomationTarget
{
public:
    virtual ~NativeAutomationTarget() = default;

    virtual bool queueNativeParameter (int parameterIndex, int sampleOffset, float normalized) noexcept = 0;
    virtual bool queueNativeParameterRamp (int parameterIndex, int start, int duration,
                                           float from, float to) noexcept = 0;

    // Update the host's cached/editor value without sending another processor
    // event. Timed audio delivery stays in the queued events above.
    virtual void setNativeParameterValue (int parameterIndex, float normalized) noexcept = 0;
    virtual void resetNativeAutomation() noexcept {}

    // Diagnostic/test hook: number of timed events consumed by the target since
    // construction.
    virtual int nativeAutomationEventsApplied() const noexcept { return 0; }
};
}
