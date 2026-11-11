#if defined(__APPLE__)
 #include <AudioUnit/AudioUnit.h>
#endif
#include "Automation.h"
#include "RustPluginProcessor.h"

namespace cadence
{
struct Automation::Macro final : juce::AudioProcessorParameterWithID
{
    Macro (int index) : AudioProcessorParameterWithID (juce::ParameterID ("automation_" + juce::String (index + 1), 1),
                                                       "Automation " + juce::String (index + 1)) {}
    float getValue() const override { return value.load(); }
    void setValue (float next) override { if (std::isfinite (next)) { value.store (juce::jlimit (0.0f, 1.0f, next)); dirty.store (true); } }
    void timed (float next) { value.store (juce::jlimit (0.0f, 1.0f, next)); }
    float getDefaultValue() const override { return 0.0f; }
    juce::String getText (float v, int) const override { return juce::String (v * 100.0f, 1) + "%"; }
    float getValueForText (const juce::String& t) const override { return juce::jlimit (0.0f, 1.0f, t.getFloatValue() / 100.0f); }
    std::atomic<float> value { 0.0f };
    std::atomic<bool> dirty { false };
};

Automation::Automation (juce::AudioProcessor& owner)
{
    for (int i = 0; i < numControls; ++i)
    {
        controls[static_cast<size_t> (i)] = new Macro (i);
        owner.addParameter (controls[static_cast<size_t> (i)]);
        routes[static_cast<size_t> (i)].owner = this;
        routes[static_cast<size_t> (i)].control = i;
    }
    for (size_t i = 0; i < capacity; ++i) queueStorage[i].sequence.store (i);
}
Automation::~Automation() { detach(); }

juce::String Automation::keyFor (juce::AudioProcessorParameter* parameter)
{
    if (auto* identified = dynamic_cast<juce::HostedAudioProcessorParameter*> (parameter)) return identified->getParameterID();
    return {};
}
std::vector<Automation::Target> Automation::targets (juce::AudioProcessor* processor)
{
    std::vector<Target> result;
    if (processor != nullptr)
        for (auto* p : processor->getParameters())
        {
            const auto key = keyFor (p);
            if (key.isNotEmpty() && p->isAutomatable()) result.push_back ({ key, p->getName (128), dynamic_cast<RustPluginProcessor*> (processor) != nullptr });
        }
    return result;
}
juce::AudioProcessorParameter& Automation::parameter (int control) const { return *controls[static_cast<size_t> (control)]; }
Automation::Assignment Automation::assignment (int control) const
{
    return juce::isPositiveAndBelow (control, numControls) ? routes[static_cast<size_t> (control)].saved : Assignment {};
}
void Automation::Route::parameterValueChanged (int, float value)
{
    if (applying.load() || ! std::isfinite (value)) return;
    applied = value;
    auto& macro = *owner->controls[static_cast<size_t> (control)];
    if (std::abs (macro.getValue() - value) > 0.0000001f) macro.setValueNotifyingHost (value);
}
void Automation::Route::parameterGestureChanged (int, bool starting)
{
    if (applying.load() || gestureOpen.exchange (starting) == starting) return;
    auto& macro = *owner->controls[static_cast<size_t> (control)];
    if (starting) macro.beginChangeGesture(); else macro.endChangeGesture();
}
void Automation::disconnect (int control)
{
    auto& route = routes[static_cast<size_t> (control)];
    if (route.target != nullptr) route.target->removeListener (&route);
    if (route.gestureOpen.exchange (false)) controls[static_cast<size_t> (control)]->endChangeGesture();
    route.target = nullptr; route.rust = nullptr; route.native = nullptr;
   #if JUCE_MAC
    route.au = nullptr;
    route.auValid = false;
   #endif
}
void Automation::detach() { for (int i = 0; i < numControls; ++i) disconnect (i); invalidate(); }
void Automation::invalidate() noexcept { epoch.fetch_add (1); }
void Automation::clear (int control)
{
    if (! juce::isPositiveAndBelow (control, numControls)) return;
    disconnect (control); routes[static_cast<size_t> (control)].saved = {}; invalidate();
}
void Automation::removeSlot (const juce::String& slot)
{
    for (int i = 0; i < numControls; ++i) if (routes[static_cast<size_t> (i)].saved.slot == slot) clear (i);
}
void Automation::bind (int control, juce::AudioProcessor* processor, bool capture)
{
    auto& route = routes[static_cast<size_t> (control)];
    disconnect (control);
    if (processor == nullptr) return;
    juce::AudioProcessorParameter* found = nullptr;
    for (auto* p : processor->getParameters())
        if (keyFor (p) == route.saved.key && p->isAutomatable()) { if (found != nullptr) return; found = p; }
    if (found == nullptr) { route.saved = {}; return; }
    route.target = found; route.rust = dynamic_cast<RustPluginProcessor*> (processor);
    route.native = route.rust == nullptr
                       ? dynamic_cast<NativeAutomationTarget*> (processor) : nullptr;
   #if JUCE_MAC
    route.au = nullptr;
    route.auValid = false;
    if (route.rust == nullptr && route.saved.key.isNotEmpty()
        && route.saved.key.containsOnly ("0123456789"))
    {
        if (auto* instance = dynamic_cast<juce::AudioPluginInstance*> (processor))
        {
            struct Visitor final : juce::ExtensionsVisitor
            {
                AudioUnit handle = nullptr;
                void visitAudioUnitClient (const AudioUnitClient& client) override
                {
                    handle = client.getAudioUnitHandle();
                }
            } visitor;
            instance->getExtensions (visitor);
            const auto parameter = static_cast<AudioUnitParameterID> (route.saved.key.getLargeIntValue());
            AudioUnitParameterInfo info {};
            UInt32 size = sizeof (info);
            if (visitor.handle != nullptr
                && AudioUnitGetProperty (visitor.handle, kAudioUnitProperty_ParameterInfo,
                                         kAudioUnitScope_Global, parameter, &info, &size) == noErr
                && info.maxValue > info.minValue)
            {
                route.au = visitor.handle;
                route.auParameter = parameter;
                route.auMin = info.minValue;
                route.auMax = info.maxValue;
                route.auValid = true;
            }
        }
    }
   #endif
    auto& macro = *controls[static_cast<size_t> (control)];
    if (capture) { macro.beginChangeGesture(); macro.setValueNotifyingHost (found->getValue()); macro.endChangeGesture(); }
    route.applied = macro.getValue();
    route.target->addListener (&route);
    route.applying.store (true);
    route.target->setValueNotifyingHost (route.applied.load());
    route.applying.store (false);
    macro.dirty.store (false);
}
bool Automation::assign (int control, const juce::String& slot, const Target& target, juce::AudioProcessor* processor, juce::String& error)
{
    if (! juce::isPositiveAndBelow (control, numControls) || slot.isEmpty() || target.key.isEmpty() || processor == nullptr)
    { error = "Choose a loaded plugin control"; return false; }
    for (int i = 0; i < numControls; ++i)
        if (i != control && routes[static_cast<size_t> (i)].saved.slot == slot && routes[static_cast<size_t> (i)].saved.key == target.key)
        { error = "That control is already assigned to Automation " + juce::String (i + 1); return false; }
    const auto available = targets (processor);
    if (std::none_of (available.begin(), available.end(), [&] (const auto& p) { return p.key == target.key; }))
    { error = "This control has no stable automation identity"; return false; }
    clear (control); routes[static_cast<size_t> (control)].saved = { slot, target.key, target.name };
    bind (control, processor, true); return true;
}
void Automation::suspend (const juce::String& slot)
{
    for (int i = 0; i < numControls; ++i)
        if (routes[static_cast<size_t> (i)].saved.slot == slot) disconnect (i);
}
void Automation::resolve (const juce::String& slot, juce::AudioProcessor* processor)
{
    for (int i = 0; i < numControls; ++i)
        if (routes[static_cast<size_t> (i)].saved.slot == slot) bind (i, processor, false);
}
void Automation::save (juce::XmlElement& root) const
{
    auto* bank = root.createNewChildElement ("AUTOMATION"); bank->setAttribute ("version", 1);
    for (int i = 0; i < numControls; ++i)
    {
        const auto& route = routes[static_cast<size_t> (i)];
        auto* entry = bank->createNewChildElement ("CONTROL"); entry->setAttribute ("index", i);
        entry->setAttribute ("value", static_cast<double> (controls[static_cast<size_t> (i)]->getValue()));
        entry->setAttribute ("slot", route.saved.slot); entry->setAttribute ("parameter", route.saved.key); entry->setAttribute ("name", route.saved.name);
    }
}
void Automation::restore (const juce::XmlElement& root)
{
    detach(); pendingCount = 0; overflow.store (false);
    for (auto& route : routes) route.saved = {};
    for (auto* macro : controls) { macro->timed (0.0f); macro->dirty.store (false); }
    const auto* bank = root.getChildByName ("AUTOMATION");
    if (bank == nullptr || bank->getIntAttribute ("version") != 1) return;
    std::array<bool, numControls> seen {};
    for (auto* entry : bank->getChildIterator())
    {
        const int i = entry->getIntAttribute ("index", -1); const auto value = entry->getDoubleAttribute ("value");
        if (! entry->hasTagName ("CONTROL") || ! juce::isPositiveAndBelow (i, numControls) || seen[static_cast<size_t> (i)] || ! std::isfinite (value)) continue;
        seen[static_cast<size_t> (i)] = true;
        auto& route = routes[static_cast<size_t> (i)];
        route.saved = { entry->getStringAttribute ("slot"), entry->getStringAttribute ("parameter"), entry->getStringAttribute ("name") };
        for (int other = 0; other < i; ++other)
            if (routes[static_cast<size_t> (other)].saved.slot == route.saved.slot && routes[static_cast<size_t> (other)].saved.key == route.saved.key) route.saved = {};
        controls[static_cast<size_t> (i)]->timed (static_cast<float> (value));
    }
}
bool Automation::push (Event event) noexcept
{
    event.epoch = epoch.load();
    size_t position = writePosition.load (std::memory_order_relaxed);
    for (int attempt = 0; attempt < 8; ++attempt)
    {
        auto& cell = queueStorage[position % capacity];
        const auto sequence = cell.sequence.load (std::memory_order_acquire);
        const auto difference = static_cast<intptr_t> (sequence) - static_cast<intptr_t> (position);
        if (difference == 0 && writePosition.compare_exchange_weak (position, position + 1, std::memory_order_relaxed))
        { cell.event = event; cell.sequence.store (position + 1, std::memory_order_release); return true; }
        if (difference < 0) break;
        position = writePosition.load (std::memory_order_relaxed);
    }
    overflow.store (true); return false;
}
bool Automation::pop (Event& event) noexcept
{
    auto& cell = queueStorage[readPosition % capacity];
    if (cell.sequence.load (std::memory_order_acquire) != readPosition + 1) return false;
    event = cell.event; cell.sequence.store (readPosition + capacity, std::memory_order_release); ++readPosition; return true;
}
bool Automation::queue (int control, int frame, float value) noexcept
{
    if (! juce::isPositiveAndBelow (control, numControls)) return false;
    if (frame < 0 || frame > 1'048'576 || ! std::isfinite (value)) { overflow.store (true); return true; }
    value = juce::jlimit (0.0f, 1.0f, value); controls[static_cast<size_t> (control)]->timed (value);
    push ({ control, frame, 0, value, value }); return true;
}
bool Automation::queueRamp (int control, int start, int duration, float from, float to) noexcept
{
    if (! juce::isPositiveAndBelow (control, numControls)) return false;
    if (duration <= 0 || duration > 1'048'576 || start < -1'048'576 || start > 1'048'576 || ! std::isfinite (from) || ! std::isfinite (to))
    { overflow.store (true); return true; }
    from = juce::jlimit (0.0f, 1.0f, from); to = juce::jlimit (0.0f, 1.0f, to);
    controls[static_cast<size_t> (control)]->timed (to); push ({ control, start, duration, from, to }); return true;
}
float Automation::valueAt (const Event& event, int frame) noexcept
{
    if (event.duration == 0) return event.to;
    const auto t = juce::jlimit (0.0, 1.0, static_cast<double> (frame - event.start) / event.duration);
    return static_cast<float> (event.from + (event.to - event.from) * t);
}
bool Automation::beginBlock (int frames) noexcept
{
    std::array<Event, capacity * 2 + numControls> events;
    size_t count = 0;
    const auto currentEpoch = epoch.load();
    if (pendingEpoch != currentEpoch) pendingCount = 0;
    for (size_t i = 0; i < pendingCount; ++i) events[count++] = pending[i];
    pendingCount = 0; pendingEpoch = currentEpoch;
    for (int i = 0; i < numControls; ++i)
        if (controls[static_cast<size_t> (i)]->dirty.exchange (false))
        { const auto v = controls[static_cast<size_t> (i)]->getValue(); events[count++] = { i, 0, 0, v, v, currentEpoch }; }
    Event event;
    while (pop (event)) if (event.epoch == currentEpoch)
    { if (count == events.size()) { overflow.store (true); break; } events[count++] = event; }
    if (overflow.exchange (false)) return false;
    for (auto& route : routes)
    {
        if (route.rust != nullptr) route.rust->clearQueuedAutomation();
        if (route.native != nullptr) route.native->resetNativeAutomation();
    }
    for (size_t i = 0; i < count; ++i) events[i].order = i;
    std::sort (events.begin(), events.begin() + static_cast<ptrdiff_t> (count), [] (const auto& a, const auto& b) {
        if (a.control != b.control) return a.control < b.control;
        return a.start != b.start ? a.start < b.start : a.order < b.order;
    });
    bool success = true;
    for (int control = 0; control < numControls; ++control)
    {
        auto& route = routes[static_cast<size_t> (control)];
        size_t begin = 0; while (begin < count && events[begin].control < control) ++begin;
        size_t end = begin; while (end < count && events[end].control == control) ++end;
        if (begin == end) continue;
        route.applying.store (true);
        bool nativeSuccess = true;
        if (route.rust != nullptr) success &= route.rust->queueParameterPoint (route.target->getParameterIndex(), 0, route.applied.load());
       #if JUCE_MAC
        std::array<AudioUnitParameterEvent, 64> auEvents {};
        size_t auEventCount = 0;
        bool auOverflow = false;
        const auto denormalise = [&route] (float value)
        {
            return static_cast<AudioUnitParameterValue> (route.auMin + value * (route.auMax - route.auMin));
        };
       #endif
        float finalValue = route.applied.load();
        for (size_t i = begin; i < end; ++i)
        {
            auto e = events[i];
            const int next = i + 1 < end ? events[i + 1].start : frames;
            if (e.start >= frames || (e.duration > 0 && e.start + e.duration >= frames && next >= frames))
            {
                if (pendingCount == pending.size()) success = false;
                else { auto carry = e; carry.start -= frames; pending[pendingCount++] = carry; }
            }
            if (e.start >= frames || next < 0) continue;
            const int first = juce::jmax (0, e.start);
            if (first >= frames || next <= first) continue;
            if (e.duration == 0)
            {
                if (route.rust != nullptr) success &= route.rust->queueParameterPoint (route.target->getParameterIndex(), first, e.to);
                if (route.native != nullptr) nativeSuccess &= route.native->queueNativeParameter (route.target->getParameterIndex(), first, e.to);
               #if JUCE_MAC
                if (route.auValid && auEventCount < auEvents.size())
                {
                    auto& scheduled = auEvents[auEventCount++];
                    scheduled.scope = kAudioUnitScope_Global;
                    scheduled.element = 0;
                    scheduled.parameter = route.auParameter;
                    scheduled.eventType = kParameterEvent_Immediate;
                    scheduled.eventValues.immediate.bufferOffset = static_cast<UInt32> (first);
                    scheduled.eventValues.immediate.value = denormalise (e.to);
                }
                else if (route.auValid)
                    auOverflow = true;
               #endif
                finalValue = e.to;
            }
            else
            {
                const int last = juce::jmin (frames, next, e.start + e.duration);
                const auto from = valueAt (e, first);
                if (route.rust != nullptr)
                {
                    success &= route.rust->queueParameterPoint (route.target->getParameterIndex(), first, from);
                    if (last > first) success &= route.rust->queueParameterRamp (route.target->getParameterIndex(), first, last - first, valueAt (e, last));
                }
                if (route.native != nullptr && last > first)
                    nativeSuccess &= route.native->queueNativeParameterRamp (route.target->getParameterIndex(), first,
                                                                             last - first, from, valueAt (e, last));
               #if JUCE_MAC
                if (route.auValid && last > first && auEventCount < auEvents.size())
                {
                    auto& scheduled = auEvents[auEventCount++];
                    scheduled.scope = kAudioUnitScope_Global;
                    scheduled.element = 0;
                    scheduled.parameter = route.auParameter;
                    scheduled.eventType = kParameterEvent_Ramped;
                    scheduled.eventValues.ramp.startBufferOffset = first;
                    scheduled.eventValues.ramp.durationInFrames = static_cast<UInt32> (last - first);
                    scheduled.eventValues.ramp.startValue = denormalise (from);
                    scheduled.eventValues.ramp.endValue = denormalise (valueAt (e, last));
                }
                else if (route.auValid)
                    auOverflow = true;
               #endif
                finalValue = valueAt (e, juce::jmin (frames - 1, next - 1));
            }
        }
        route.applied = finalValue;
       #if JUCE_MAC
        if (route.auValid)
        {
            if (auOverflow
                || (auEventCount > 0
                    && AudioUnitScheduleParameters (static_cast<AudioUnit> (route.au), auEvents.data(),
                                                    static_cast<UInt32> (auEventCount)) != noErr))
                route.auValid = false;
        }
       #endif
        if (route.target != nullptr)
        {
            if (route.rust != nullptr) route.rust->setTimedParameterValue (route.target->getParameterIndex(), finalValue);
            else if (route.native != nullptr)
            {
                if (nativeSuccess) route.native->setNativeParameterValue (route.target->getParameterIndex(), finalValue);
                else route.target->setValueNotifyingHost (finalValue);
            }
            else
            {
               #if JUCE_MAC
                if (! route.auValid)
               #endif
                    route.target->setValueNotifyingHost (finalValue);
            }
        }
        success &= nativeSuccess;
        controls[static_cast<size_t> (control)]->timed (finalValue);
        route.applying.store (false);
    }
    if (! success) for (auto& route : routes) if (route.rust != nullptr) route.rust->clearQueuedAutomation();
    return success;
}
}
