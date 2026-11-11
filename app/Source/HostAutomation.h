#pragma once

// Project-only wrapper extension. The wrapper retains the original audio block;
// compact timing is delivered to Cadence rather than subdividing its graph.
namespace cadence
{
class HostAutomationClient
{
public:
    virtual ~HostAutomationClient() = default;
    virtual bool queueHostAutomation (int parameterIndex, int frame, float normalized) noexcept = 0;
    virtual bool queueHostAutomationRamp (int parameterIndex, int startFrame, int duration,
                                          float startNormalized, float endNormalized) noexcept = 0;
};
}
