#pragma once
#include <JuceHeader.h>
#include "HostAutomation.h"
#include "NativeAutomation.h"

namespace cadence
{
class RustPluginProcessor;

class Automation final
{
public:
    static constexpr int numControls = 8;
    struct Target { juce::String key, name; bool sampleAccurate = false; };
    struct Assignment { juce::String slot, key, name; };
    explicit Automation (juce::AudioProcessor& owner);
    ~Automation();
    static std::vector<Target> targets (juce::AudioProcessor*);
    static juce::String keyFor (juce::AudioProcessorParameter*);
    bool assign (int control, const juce::String& slot, const Target&, juce::AudioProcessor*, juce::String& error);
    void clear (int control);
    void removeSlot (const juce::String&);
    void detach();
    void suspend (const juce::String&);
    void resolve (const juce::String&, juce::AudioProcessor*);
    Assignment assignment (int control) const;
    void save (juce::XmlElement&) const;
    void restore (const juce::XmlElement&);
    void invalidate() noexcept;
    bool queue (int control, int frame, float value) noexcept;
    bool queueRamp (int control, int start, int duration, float from, float to) noexcept;
    bool beginBlock (int frames) noexcept;
    bool hasOverflowed() const { return overflow.load(); }
    juce::AudioProcessorParameter& parameter (int control) const;

private:
    struct Macro;
    struct Route final : juce::AudioProcessorParameter::Listener
    {
        Automation* owner = nullptr;
        int control = 0;
        Assignment saved;
        juce::AudioProcessorParameter* target = nullptr;
        RustPluginProcessor* rust = nullptr;
        NativeAutomationTarget* native = nullptr;
       #if JUCE_MAC
        void* au = nullptr;
        uint32_t auParameter = 0;
        double auMin = 0.0, auMax = 1.0;
        bool auValid = false;
       #endif
        std::atomic<bool> applying { false };
        std::atomic<bool> gestureOpen { false };
        std::atomic<float> applied { 0.0f };
        void parameterValueChanged (int, float) override;
        void parameterGestureChanged (int, bool) override;
    };
    struct Event { int control = 0, start = 0, duration = 0; float from = 0, to = 0; uint64_t epoch = 0, order = 0; };
    static constexpr size_t capacity = 256;
    struct Cell { std::atomic<size_t> sequence { 0 }; Event event; };
    bool push (Event) noexcept;
    bool pop (Event&) noexcept;
    void bind (int, juce::AudioProcessor*, bool capture);
    void disconnect (int);
    static float valueAt (const Event&, int) noexcept;
    std::array<Macro*, numControls> controls {};
    std::array<Route, numControls> routes;
    std::array<Cell, capacity> queueStorage;
    std::atomic<size_t> writePosition { 0 };
    size_t readPosition = 0;
    std::atomic<uint64_t> epoch { 1 };
    std::atomic<bool> overflow { false };
    std::array<Event, capacity> pending;
    size_t pendingCount = 0;
    uint64_t pendingEpoch = 1;
    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (Automation)
};
}
