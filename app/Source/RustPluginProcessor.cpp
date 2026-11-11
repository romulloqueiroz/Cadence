#include "RustPluginProcessor.h"
#include "Theme.h"

namespace cadence
{
namespace
{
int32_t checkCancelled (void* context)
{
    return context != nullptr && static_cast<const std::atomic<bool>*> (context)->load (std::memory_order_acquire) ? 1 : 0;
}

juce::String identifier (const juce::String& source, const juce::String& selector)
{
    auto* object = new juce::DynamicObject();
    object->setProperty ("source", source);
    object->setProperty ("selector", selector);
    return juce::JSON::toString (juce::var (object), true);
}

class RustEditor final : public juce::AudioProcessorEditor, private juce::Timer
{
public:
    RustEditor (RustPluginProcessor& p, const juce::String& format)
        : AudioProcessorEditor (p), bridgeProcessor (p), generic (p), formatName (format)
    {
        setLookAndFeel (&theme);
        addAndMakeVisible (message);
        message.setJustificationType (juce::Justification::centredLeft);
        message.setColour (juce::Label::textColourId, colours::accent);
        addAndMakeVisible (streaming);
        streaming.setName ("Streaming diagnostics");
        streaming.setJustificationType (juce::Justification::topLeft);
        streaming.setFont (juce::Font (juce::FontOptions (12.5f)));
        streaming.setColour (juce::Label::textColourId, juce::Colour (0xffc8cdd8));
        streaming.setTooltip ("Sample and cache bytes exclude process/OS memory. Page time includes file access and decoding, excludes queue wait. Streaming misses count output voice frames. Counters and peaks last until the instrument is reloaded; values update after audio/control blocks.");
        showStreaming = bridgeProcessor.streamingStats (streamingSnapshot);
        streaming.setVisible (showStreaming);
        addAndMakeVisible (genericContainer);
        notices.setName ("Instrument compatibility");
        notices.setMultiLine (true);
        notices.setReadOnly (true);
        notices.setScrollbarsShown (true);
        notices.setText (bridgeProcessor.compatibilityNotices(), false);
        notices.setVisible (bridgeProcessor.compatibilityNotices().isNotEmpty());
        addChildComponent (notices);
        genericContainer.addAndMakeVisible (generic);
        genericWidth = generic.getWidth();
        if (formatName == "CLAP")
        {
            nativeEditorButton.setName ("Native CLAP editor");
            nativeEditorButton.setButtonText ("Open native editor");
            nativeEditorButton.onClick = [this]
            {
                const auto open = (bridgeProcessor.nativeEditorState() & 2u) == 0;
                bridgeProcessor.setNativeEditorOpen (open);
                nativeEditorButton.setButtonText (open ? "Close native editor" : "Open native editor");
            };
            addAndMakeVisible (nativeEditorButton);
        }
        setSize (400, juce::jmax (180, generic.getHeight() + 72) + noticeHeight() + (showStreaming ? 144 : 0) + (formatName == "CLAP" ? 28 : 0));
        timerCallback(); startTimerHz (5);
    }
    ~RustEditor() override { setLookAndFeel (nullptr); }
    void resized() override
    {
        auto area = getLocalBounds().reduced (8);
        if (formatName == "CLAP")
            nativeEditorButton.setBounds (area.removeFromTop (28));
        message.setBounds (area.removeFromTop (48));
        notices.setBounds (area.removeFromTop (noticeHeight()).reduced (0, 4));
        streaming.setBounds (showStreaming ? area.removeFromTop (144) : juce::Rectangle<int>());
        const auto scale = juce::jmin (1.0f, static_cast<float> (area.getWidth()) / static_cast<float> (genericWidth));
        generic.setBounds (0, 0, genericWidth, static_cast<int> (std::ceil (static_cast<float> (area.getHeight()) / scale)));
        genericContainer.setBounds (0, 0, generic.getWidth(), generic.getHeight());
        genericContainer.setTransform (juce::AffineTransform::scale (scale).translated (static_cast<float> (area.getX()), static_cast<float> (area.getY())));
    }
    void paint (juce::Graphics& g) override { g.fillAll (juce::Colour (0xff15171b)); }
private:
    int noticeHeight() const { return bridgeProcessor.compatibilityNotices().isNotEmpty() ? 120 : 0; }
    void timerCallback() override
    {
        bridgeProcessor.refreshTimedParameterListeners();
        if (formatName == "CLAP")
        {
            const auto state = bridgeProcessor.nativeEditorState();
            nativeEditorButton.setEnabled ((state & 1u) != 0 && ! bridgeProcessor.hasFailed());
            nativeEditorButton.setButtonText ((state & 2u) != 0 ? "Close native editor" : "Open native editor");
        }
        CadenceStreamingStats latest {};
        if (bridgeProcessor.streamingStats (latest))
        {
            streamingSnapshot = latest;
            if (! showStreaming)
            {
                showStreaming = true;
                streaming.setVisible (true);
                setSize (400, juce::jmax (180, generic.getHeight() + 72) + noticeHeight() + 144);
            }
        }
        const auto& stats = streamingSnapshot;
        if (showStreaming)
        {
            auto mib = [] (uint64_t bytes) { return juce::String (static_cast<double> (bytes) / 1048576.0, 1) + " MiB"; };
            auto ms = [] (double nanos) { return juce::String (nanos / 1000000.0, 2); };
            const auto mean = stats.read_count > 0 ? static_cast<double> (stats.read_total_ns) / static_cast<double> (stats.read_count) : 0.0;
            streaming.setText ("Voices: " + juce::String (stats.active_voices) + " / " + juce::String (stats.voice_capacity)
                + "    Read queue: " + juce::String (stats.pending_reads)
                + "\nResident: " + mib (stats.resident_bytes) + "    Cache: " + mib (stats.cache_capacity_bytes)
                + "\nReady: " + mib (stats.cache_ready_bytes) + "    Peak ready: " + mib (stats.peak_cache_ready_bytes)
                + "\nPages read: " + juce::String (stats.pages_read) + "    Read errors: " + juce::String (stats.read_errors)
                + "\nPage time mean / last / peak (ms):\n" + ms (mean) + " / " + ms (static_cast<double> (stats.last_read_ns)) + " / " + ms (static_cast<double> (stats.max_read_ns))
                + "\nStreaming misses: " + juce::String (stats.underrun_blocks) + " blocks / " + juce::String (stats.underrun_frames) + " voice frames",
                juce::dontSendNotification);
        }
        message.setText (bridgeProcessor.hasFailed() ? bridgeProcessor.failureReason() + ".\nRemove and reload this plugin."
                                               : bridgeProcessor.hasStateError() ? "State could not be saved.\nThe worker may have failed or exceeded 64 KiB."
                                               : "Sandboxed " + formatName + " - " + juce::String (bridgeProcessor.getLatencySamples()) + " sample delay\n"
                                                 + bridgeProcessor.getName() + (showStreaming ? " - Host dropouts: " : " - Dropouts: ") + juce::String (bridgeProcessor.pipelineUnderruns())
                                                 + (bridgeProcessor.audioWorkgroupActive() ? " - Workgroup"
                                                    : bridgeProcessor.realtimeSchedulingActive() ? " - Realtime" : " - QoS"),
                         juce::dontSendNotification);
        generic.setEnabled (! bridgeProcessor.hasFailed());
    }
    RustPluginProcessor& bridgeProcessor;
    CadenceLookAndFeel theme;
    juce::Component genericContainer;
    juce::GenericAudioProcessorEditor generic;
    juce::Label message, streaming;
    juce::TextEditor notices;
    juce::TextButton nativeEditorButton;
    CadenceStreamingStats streamingSnapshot {};
    bool showStreaming = false;
    juce::String formatName;
    int genericWidth = 414;
};
}

struct RustPluginProcessor::Parameter final : juce::AudioParameterFloat
{
    Parameter (const juce::var& info)
        : AudioParameterFloat (juce::ParameterID (info["key"].toString(), 1), info["name"].toString(),
              juce::NormalisableRange<float> (static_cast<float> (info["min"]), static_cast<float> (info["max"]),
                                             static_cast<bool> (info["stepped"]) ? 1.0f : 0.0f),
              static_cast<float> (info["default"]),
              juce::AudioParameterFloatAttributes().withStringFromValueFunction (
                  [] (float value, int) { return juce::String::formatted ("%.6g", static_cast<double> (value)); })),
          id (static_cast<uint16_t> (static_cast<int> (info["id"]))), readOnly (static_cast<bool> (info["read_only"])) {}
    void valueChanged (float) override { if (timedParameter != this) dirty.store (true, std::memory_order_release); }
    bool isAutomatable() const override { return ! readOnly; }
    void timed (float normalized) noexcept
    {
        const auto* previous = timedParameter; timedParameter = this;
        static_cast<juce::AudioProcessorParameter*> (this)->setValue (normalized);
        timedParameter = previous; uiPending.store (true);
    }
    void adoptValue (float normalized) noexcept
    {
        const auto* previous = timedParameter; timedParameter = this;
        static_cast<juce::AudioProcessorParameter*> (this)->setValue (juce::jlimit (0.0f, 1.0f, normalized));
        timedParameter = previous;
    }
    inline static thread_local const Parameter* timedParameter = nullptr;
    uint16_t id;
    bool readOnly = false;
    std::atomic<bool> dirty { true }, uiPending { false };
};

bool RustPluginProcessor::supports (const juce::String& format)
{
    return format == "Protocol" || format == "Kontakt" || format == "CLAP";
}

juce::PluginDescription RustPluginProcessor::kontaktDescription (const juce::File& file)
{
    juce::PluginDescription d;
    d.name = d.descriptiveName = file.getFileNameWithoutExtension();
    d.pluginFormatName = "Kontakt";
    d.manufacturerName = "Cadence";
    d.version = "0.1.0";
    d.isInstrument = true;
    d.numOutputChannels = 2;
    d.uniqueId = d.deprecatedUid = file.getFullPathName().hashCode();
    d.fileOrIdentifier = identifier (file.getFullPathName(), {});
    return d;
}

juce::File RustPluginProcessor::kontaktFile (const juce::PluginDescription& d)
{
    const auto stored = juce::JSON::parse (d.fileOrIdentifier);
    const auto source = stored.isObject() ? stored["source"].toString() : d.fileOrIdentifier;
    return juce::File (source.startsWith ("cadence-kontakt:") ? juce::JSON::parse (source.substring (16))["path"].toString() : source);
}

juce::PluginDescription RustPluginProcessor::relocateKontakt (juce::PluginDescription d,
                                                              const juce::File& instrument,
                                                              const juce::File& sampleRoot)
{
    const auto stored = juce::JSON::parse (d.fileOrIdentifier);
    const auto source = stored.isObject() ? stored["source"].toString() : d.fileOrIdentifier;
    auto spec = source.startsWith ("cadence-kontakt:") ? juce::JSON::parse (source.substring (16)) : juce::var();
    if (! spec.isObject())
    {
        spec = juce::var (new juce::DynamicObject());
        spec.getDynamicObject()->setProperty ("path", source);
    }
    if (instrument != juce::File())
    {
        spec.getDynamicObject()->setProperty ("path", instrument.getFullPathName());
        d.name = d.descriptiveName = instrument.getFileNameWithoutExtension();
    }
    if (sampleRoot != juce::File())
    {
        juce::Array<juce::var> roots;
        if (const auto* old = spec["sample_roots"].getArray()) roots = *old;
        roots.addIfNotAlreadyThere (sampleRoot.getFullPathName());
        spec.getDynamicObject()->setProperty ("sample_roots", roots);
    }
    else if (instrument == juce::File())
        spec.getDynamicObject()->setProperty ("sample_roots", juce::Array<juce::var>());
    d.fileOrIdentifier = identifier ("cadence-kontakt:" + juce::JSON::toString (spec, true),
                                      stored.isObject() ? stored["selector"].toString() : juce::String());
    return d;
}

juce::File RustPluginProcessor::workerFile()
{
    const auto parent = juce::File::getSpecialLocation (juce::File::currentExecutableFile).getParentDirectory();
    return (parent.getFileName() == "MacOS" ? parent.getParentDirectory() : parent).getChildFile ("Helpers/plugin-worker");
}

CadenceBridge* RustPluginProcessor::connect (const juce::String& source, const juce::String& selector,
                                            juce::String& error, const std::atomic<bool>* cancelled)
{
    if (cadence_bridge_abi() != CADENCE_BRIDGE_ABI)
    {
        error = "Rust bridge ABI mismatch"; return nullptr;
    }
    const auto worker = workerFile();
    if (! worker.existsAsFile())
    {
        error = "Rust plugin worker is missing - rebuild or reinstall Cadence"; return nullptr;
    }
    std::array<char, 2048> buffer {};
    auto* result = cadence_bridge_open_cancellable (worker.getFullPathName().toRawUTF8(), source.toRawUTF8(),
        selector.isEmpty() ? nullptr : selector.toRawUTF8(), buffer.data(), buffer.size(), checkCancelled, const_cast<std::atomic<bool>*> (cancelled));
    if (result == nullptr)
        error = juce::String::fromUTF8 (buffer.data()).isNotEmpty()
                    ? juce::String::fromUTF8 (buffer.data()) : "Rust plugin could not be loaded";
    return result;
}

juce::var RustPluginProcessor::metadata (CadenceBridge* handle)
{
    const auto length = cadence_bridge_metadata (handle, nullptr, 0);
    if (length == 0 || length > 65536)
        return {};
    juce::MemoryBlock storage (length, true);
    cadence_bridge_metadata (handle, static_cast<char*> (storage.getData()), length);
    return juce::JSON::parse (juce::String::fromUTF8 (static_cast<const char*> (storage.getData())));
}

juce::PluginDescription RustPluginProcessor::describe (const juce::String& source, const juce::String& format,
                                                      juce::String& error, const juce::String& selector)
{
    juce::PluginDescription result;
    auto* handle = connect (source, selector, error);
    if (handle == nullptr)
        return result;
    const auto info = metadata (handle);
    cadence_bridge_close (handle);
    if (! info.isObject()) { error = "Invalid Rust plugin metadata"; return result; }
    if (static_cast<int> (info["outputs"]) == 0 || static_cast<int> (info["kind"]) == 2)
    { error = "MIDI-only and outputless plugins are not supported by this bridge"; return result; }
    result.name = format == "Kontakt" ? juce::File (source).getFileNameWithoutExtension() : info["name"].toString();
    result.descriptiveName = result.name;
    result.pluginFormatName = format;
    result.manufacturerName = info["vendor"].toString();
    result.version = info["version"].toString();
    result.uniqueId = result.deprecatedUid = info["id"].toString().hashCode();
    result.fileOrIdentifier = identifier (source, info["id"].toString());
    result.isInstrument = static_cast<int> (info["kind"]) == 0;
    result.numInputChannels = static_cast<int> (info["inputs"]);
    result.numOutputChannels = static_cast<int> (info["outputs"]);
    return result;
}

std::vector<juce::PluginDescription> RustPluginProcessor::describeAll (const juce::String& source,
                                                                      const juce::String& format,
                                                                      juce::String& error)
{
    std::vector<juce::PluginDescription> result;
    auto* handle = connect (source, {}, error);
    if (handle == nullptr)
        return result;
    const auto info = metadata (handle);
    cadence_bridge_close (handle);
    if (! info.isObject()) { error = "Invalid Rust plugin metadata"; return result; }
    if (static_cast<int> (info["outputs"]) == 0 || static_cast<int> (info["kind"]) == 2)
    { error = "MIDI-only and outputless plugins are not supported by this bridge"; return result; }

    auto add = [&] (const juce::String& id, const juce::String& variantName, const juce::String& vendor,
                    const juce::String& version, int kind)
    {
        if (id.isEmpty() || variantName.isEmpty()) return;
        juce::PluginDescription description;
        description.name = description.descriptiveName = variantName;
        description.pluginFormatName = format;
        description.manufacturerName = vendor;
        description.version = version;
        description.uniqueId = description.deprecatedUid = id.hashCode();
        description.fileOrIdentifier = identifier (source, id);
        description.isInstrument = kind == 0;
        description.numInputChannels = static_cast<int> (info["inputs"]);
        description.numOutputChannels = static_cast<int> (info["outputs"]);
        result.push_back (description);
    };

    if (const auto* variants = info["variants"].getArray())
        for (const auto& variant : *variants)
            add (variant["id"].toString(), variant["name"].toString(), variant["vendor"].toString(),
                 variant["version"].toString(), static_cast<int> (variant["kind"]));

    if (result.empty())
        add (info["id"].toString(),
             format == "Kontakt" ? juce::File (source).getFileNameWithoutExtension() : info["name"].toString(),
             info["vendor"].toString(), info["version"].toString(), static_cast<int> (info["kind"]));
    return result;
}

std::vector<juce::PluginDescription> RustPluginProcessor::bundledDescriptions()
{
    std::vector<juce::PluginDescription> result;
    const auto contents = juce::File::getSpecialLocation (juce::File::currentExecutableFile).getParentDirectory().getParentDirectory();
    const auto gain = contents.getChildFile ("Resources/ProtocolPlugins/gain.dylib");
    for (bool instrument : { true, false })
    {
        if (! instrument && ! gain.existsAsFile()) continue;
        juce::PluginDescription d;
        const juce::String id = instrument ? "com.example.sine" : "com.example.gain";
        d.name = d.descriptiveName = instrument ? "Sine Demo" : "Gain";
        d.pluginFormatName = "Protocol"; d.manufacturerName = "Example"; d.version = "1.0.0";
        d.fileOrIdentifier = identifier (instrument ? "builtin:sine" : "builtin:gain", id);
        d.uniqueId = d.deprecatedUid = id.hashCode(); d.isInstrument = instrument;
        d.numInputChannels = instrument ? 0 : 2; d.numOutputChannels = 2;
        result.push_back (d);
    }
    return result;
}

std::unique_ptr<RustPluginProcessor> RustPluginProcessor::open (const juce::PluginDescription& description,
                                                             juce::String& error, const std::atomic<bool>* cancelled)
{
    const auto stored = juce::JSON::parse (description.fileOrIdentifier);
    const auto source = stored.isObject() ? stored["source"].toString() : description.fileOrIdentifier;
    const auto selector = stored.isObject() ? stored["selector"].toString() : juce::String();
    auto* handle = connect (source, selector, error, cancelled);
    if (handle == nullptr)
        return nullptr;
    const auto info = metadata (handle);
    if (! info.isObject()) { cadence_bridge_close (handle); error = "Invalid Rust plugin metadata"; return nullptr; }
    if (static_cast<int> (info["outputs"]) == 0 || static_cast<int> (info["kind"]) == 2)
    { cadence_bridge_close (handle); error = "MIDI-only and outputless plugins are not supported by this bridge"; return nullptr; }
    auto result = std::unique_ptr<RustPluginProcessor> (new RustPluginProcessor (handle, info, description.pluginFormatName));
    result->name = description.name;
    return result;
}

RustPluginProcessor::RustPluginProcessor (CadenceBridge* bridge, const juce::var& info, juce::String format)
    : AudioProcessor ([&info]
      {
          BusesProperties buses;
          const auto in = static_cast<int> (info["inputs"]);
          const auto side = static_cast<int> (info["sidechain_inputs"]);
          if (in > 0) buses = buses.withInput ("Input", juce::AudioChannelSet::canonicalChannelSet (in), true);
          if (side > 0) buses = buses.withInput ("Sidechain", juce::AudioChannelSet::canonicalChannelSet (side), true);
          return buses.withOutput ("Output", juce::AudioChannelSet::canonicalChannelSet (static_cast<int> (info["outputs"])), true);
      }()),
      handle (bridge), name (info["name"].toString()), formatName (std::move (format)),
      inputs (static_cast<int> (info["inputs"])), outputs (static_cast<int> (info["outputs"])),
      sidechainInputs (static_cast<int> (info["sidechain_inputs"]))
{
    if (const auto* items = info["notices"].getArray())
        for (const auto& item : *items)
            notices += (notices.isEmpty() ? juce::String() : "\n\n") + item.toString();
    inputStorage.resize (static_cast<size_t> (inputs) * CADENCE_MAX_BLOCK);
    sidechainStorage.resize (static_cast<size_t> (sidechainInputs) * CADENCE_MAX_BLOCK);
    outputStorage.resize (static_cast<size_t> (outputs) * CADENCE_MAX_BLOCK);
    if (const auto* parameters = info["parameters"].getArray())
        for (const auto& parameter : *parameters)
        {
            if (static_cast<bool> (parameter["read_only"])) continue;
            auto* control = new Parameter (parameter);
            controls.push_back (control);
            addParameter (control);
        }
    intrinsicLatency = static_cast<int> (info["latency_samples"]);
    setLatencySamples (intrinsicLatency);
    tailSamples = static_cast<double> (info["tail_samples"]);
    tail = tailSamples / rate;
    lastDynamicLatency = intrinsicLatency;
    lastDynamicTail = static_cast<int> (tailSamples);
    polledLatency = lastDynamicLatency;
    polledTail = lastDynamicTail;
    // A CLAP latency/tail change is published by the worker's creator thread,
    // which can update shared memory while this host is idle. Poll on the
    // message thread so DAW latency compensation follows without a new block.
    startTimer (16);
}

RustPluginProcessor::~RustPluginProcessor()
{
    stopTimer();
    cancelPendingUpdate();
    cadence_bridge_close (handle);
}

void RustPluginProcessor::takeOutputMidi (int blockOffset)
{
    std::array<CadenceEvent, CADENCE_MAX_EVENTS> events {};
    std::array<uint8_t, CADENCE_MAX_OUTPUT_SYSEX> sysex {};
    uint32_t count = 0, sysexLength = 0;
    if (cadence_bridge_take_output_midi (handle, events.data(), static_cast<uint32_t> (events.size()),
            sysex.data(), static_cast<uint32_t> (sysex.size()), &count, &sysexLength) != 0)
        return;
    count = juce::jmin (count, static_cast<uint32_t> (CADENCE_MAX_EVENTS));
    sysexLength = juce::jmin (sysexLength, static_cast<uint32_t> (CADENCE_MAX_OUTPUT_SYSEX));
    for (uint32_t i = 0; i < count; ++i)
    {
        if (nextMidi->count == nextMidi->events.size())
            break;
        auto event = events[static_cast<size_t> (i)];
        event.frame = static_cast<uint32_t> (juce::jmax (0, static_cast<int> (event.frame) + blockOffset));
        if (event.kind == 9)
        {
            const auto offset = static_cast<uint32_t> (event.data1);
            const auto length = static_cast<uint32_t> (event.data2);
            if (offset > sysexLength || length > sysexLength - offset
                || nextMidi->sysexLength + length > nextMidi->sysex.size())
                continue;
            std::copy_n (sysex.data() + offset, length, nextMidi->sysex.data() + nextMidi->sysexLength);
            event.data1 = static_cast<uint16_t> (nextMidi->sysexLength);
            nextMidi->sysexLength += length;
        }
        nextMidi->events[static_cast<size_t> (nextMidi->count++)] = event;
    }
}

void RustPluginProcessor::emitPendingMidi (juce::MidiBuffer& midi, int frames)
{
    for (uint32_t i = 0; i < pendingMidi->count; ++i)
    {
        const auto& event = pendingMidi->events[static_cast<size_t> (i)];
        const auto channel = juce::jlimit (1, 16, static_cast<int> (event.channel) + 1);
        const auto value = static_cast<int> (juce::jlimit (0.0f, 1.0f, event.value) * 127.0f);
        juce::MidiMessage message;
        switch (event.kind)
        {
            case 0: message = juce::MidiMessage::noteOn (channel, static_cast<int> (event.data1), static_cast<juce::uint8> (value)); break;
            case 1: message = juce::MidiMessage::noteOff (channel, static_cast<int> (event.data1), static_cast<float> (value) / 127.0f); break;
            case 2: message = juce::MidiMessage::aftertouchChange (channel, static_cast<int> (event.data1), static_cast<juce::uint8> (value)); break;
            case 3: message = juce::MidiMessage::controllerEvent (channel, static_cast<int> (event.data1), static_cast<juce::uint8> (value)); break;
            case 4: message = juce::MidiMessage::programChange (channel, static_cast<int> (event.data1)); break;
            case 5: message = juce::MidiMessage::pitchWheel (channel, juce::jlimit (0, 16383, static_cast<int> ((event.value + 1.0f) * 8192.0f))); break;
            case 6: message = juce::MidiMessage::channelPressureChange (channel, static_cast<juce::uint8> (value)); break;
            case 9:
            {
                const auto offset = static_cast<size_t> (event.data1);
                const auto length = static_cast<size_t> (event.data2);
                if (offset <= pendingMidi->sysexLength && length <= pendingMidi->sysexLength - offset && length > 0)
                    message = juce::MidiMessage (pendingMidi->sysex.data() + offset, static_cast<int> (length));
                break;
            }
            default: break;
        }
        if (message.getRawDataSize() > 0)
            midi.addEvent (message, juce::jlimit (0, juce::jmax (0, frames - 1), static_cast<int> (event.frame)));
    }
    pendingMidi->count = 0;
    pendingMidi->sysexLength = 0;
}

void RustPluginProcessor::refreshDynamicTiming()
{
    CadenceDynamicStats dynamic {};
    if (cadence_bridge_dynamic_stats (handle, &dynamic) == 0)
    {
        intrinsicLatency = static_cast<int> (dynamic.plugin_latency_frames);
        tailSamples = static_cast<double> (dynamic.plugin_tail_frames);
        lastDynamicLatency = intrinsicLatency;
        lastDynamicTail = static_cast<int> (tailSamples);
    }
    CadencePipelineStats pipeline {};
    cadence_bridge_pipeline_stats (handle, &pipeline);
    setLatencySamples (intrinsicLatency + static_cast<int> (pipeline.latency_frames));
    tail = (tailSamples + static_cast<double> (pipeline.latency_frames)) / rate;
}

void RustPluginProcessor::refreshParameterValues()
{
    static constexpr uint32_t capacity = 512;
    std::array<uint32_t, capacity> ids {};
    std::array<float, capacity> values {};
    uint64_t generation = 0;
    const auto count = cadence_bridge_parameter_snapshot (handle, ids.data(), values.data(), capacity, &generation);
    if (count <= 0)
        return;
    for (auto* control : controls)
    {
        for (int index = 0; index < count; ++index)
        {
            if (ids[static_cast<size_t> (index)] != static_cast<uint32_t> (control->id))
                continue;
            const auto next = juce::jlimit (0.0f, 1.0f, values[static_cast<size_t> (index)]);
            if (! juce::approximatelyEqual (next, static_cast<juce::AudioProcessorParameter*> (control)->getValue()))
            {
                control->adoptValue (next);
                control->sendValueChangedMessageToListeners (next);
            }
            break;
        }
    }
}

void RustPluginProcessor::handleAsyncUpdate()
{
    refreshDynamicTiming();
    refreshParameterValues();
}

void RustPluginProcessor::timerCallback()
{
    if (handle == nullptr)
        return;
    CadenceDynamicStats dynamic {};
    if (cadence_bridge_dynamic_stats (handle, &dynamic) == 0)
    {
        const auto latency = static_cast<int> (dynamic.plugin_latency_frames);
        const auto tailFrames = static_cast<int> (dynamic.plugin_tail_frames);
        if (latency != polledLatency || tailFrames != polledTail)
        {
            polledLatency = latency;
            polledTail = tailFrames;
            refreshDynamicTiming();
        }
    }
    const auto generation = cadence_bridge_parameter_generation (handle);
    if (generation != lastParameterGeneration)
    {
        lastParameterGeneration = generation;
        refreshParameterValues();
    }
}
bool RustPluginProcessor::hasFailed() const
{
    return failed.load (std::memory_order_acquire) || cadence_bridge_failed (handle) != 0;
}

juce::String RustPluginProcessor::failureReason() const
{
    switch (cadence_bridge_failure_reason (handle))
    {
        case 1: return "Invalid audio block";
        case 2: return "Instrument input queue overflowed";
        case 3: return "Worker protocol error";
        case 4: return "Worker could not be notified";
        case 5: case 9: return "Worker audio processing timed out";
        case 6: return "Worker command timed out";
        case 7: return "Worker did not finish queued audio";
        case 8: return "Worker process exited";
        case 10: return "Worker reported an error";
        case 11: return "Loading cancelled";
        default: return "Worker stopped";
    }
}

uint64_t RustPluginProcessor::underruns() const
{
    return pipelineUnderruns();
}

uint64_t RustPluginProcessor::pipelineUnderruns() const
{
    CadencePipelineStats stats {};
    cadence_bridge_pipeline_stats (handle, &stats);
    return stats.underruns;
}

bool RustPluginProcessor::streamingStats (CadenceStreamingStats& stats) const
{
    return cadence_bridge_streaming_stats (handle, &stats) == 0 && (stats.flags & 1u) != 0;
}

bool RustPluginProcessor::realtimeSchedulingActive() const
{
    CadencePipelineStats stats {};
    cadence_bridge_pipeline_stats (handle, &stats);
    return (stats.scheduling_flags & 3u) != 0;
}

void RustPluginProcessor::setAudioWorkgroup (void* workgroup, uint32_t generation)
{
    cadence_bridge_set_workgroup (handle, workgroup, generation);
}

bool RustPluginProcessor::audioWorkgroupActive() const
{
    CadencePipelineStats stats {};
    cadence_bridge_pipeline_stats (handle, &stats);
    return (stats.scheduling_flags & 2u) != 0;
}

void RustPluginProcessor::setNativeEditorOpen (bool open)
{
    cadence_bridge_set_gui_request (handle, open ? 1u : 0u);
}

uint32_t RustPluginProcessor::nativeEditorState() const
{
    return cadence_bridge_gui_state (handle);
}

bool RustPluginProcessor::isBusesLayoutSupported (const BusesLayout& layout) const
{
    if (layout.outputBuses.size() != 1 || layout.getMainOutputChannelSet().size() != outputs)
        return false;
    const int busCount = (inputs > 0 ? 1 : 0) + (sidechainInputs > 0 ? 1 : 0);
    if (layout.inputBuses.size() != busCount)
        return false;
    int bus = 0;
    if (inputs > 0)
    {
        const auto set = layout.getChannelSet (true, bus++);
        if (set.size() != inputs) return false;
    }
    if (sidechainInputs > 0)
    {
        const auto set = layout.getChannelSet (true, bus);
        if (! set.isDisabled() && set.size() != sidechainInputs) return false;
    }
    return true;
}

void RustPluginProcessor::prepareToPlay (double sampleRate, int block)
{
    // Consume only the graph's first matching prepare after off-thread loading.
    if (std::exchange (preparedForAttachment, false) && juce::exactlyEqual (sampleRate, rate) && block == preparedBlock)
        return;
    failed.store (cadence_bridge_prepare (handle, sampleRate,
        static_cast<uint32_t> (juce::jlimit (1, CADENCE_MAX_BLOCK, block))) != 0);
    updatePreparedDetails (sampleRate, block);
}

void RustPluginProcessor::updatePreparedDetails (double sampleRate, int block)
{
    rate = sampleRate;
    preparedBlock = juce::jlimit (1, CADENCE_MAX_BLOCK, block);
    setRateAndBufferSizeDetails (sampleRate, block);
    CadenceDynamicStats dynamic {};
    if (cadence_bridge_dynamic_stats (handle, &dynamic) == 0)
    {
        intrinsicLatency = static_cast<int> (dynamic.plugin_latency_frames);
        tailSamples = static_cast<double> (dynamic.plugin_tail_frames);
        lastDynamicLatency = intrinsicLatency;
        lastDynamicTail = static_cast<int> (tailSamples);
    }
    CadencePipelineStats stats {};
    cadence_bridge_pipeline_stats (handle, &stats);
    setLatencySamples (intrinsicLatency + static_cast<int> (stats.latency_frames));
    tail = (tailSamples + static_cast<double> (stats.latency_frames)) / rate;
    samplePosition = 0;
}

bool RustPluginProcessor::prepareForLoading (double sampleRate, int block, const juce::MemoryBlock& state,
                                            const std::atomic<bool>* cancelled)
{
    const auto maximum = static_cast<uint32_t> (juce::jlimit (1, CADENCE_MAX_BLOCK, block));
    if (cadence_bridge_prepare_cancellable (handle, sampleRate, maximum, checkCancelled,
            const_cast<std::atomic<bool>*> (cancelled)) != 0)
        return false;
    updatePreparedDetails (sampleRate, block);
    if (state.getSize() > 0 && ! restoreState (state.getData(), static_cast<int> (state.getSize()), cancelled))
        return false;
    preparedForAttachment = true;
    return true;
}
void RustPluginProcessor::releaseResources() { cadence_bridge_reset (handle); }

void RustPluginProcessor::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi)
{
    if (hasFailed()) { buffer.clear(); midi.clear(); return; }
    if (! isNonRealtime() && buffer.getNumSamples() > preparedBlock)
    { failed.store (true); buffer.clear(); midi.clear(); return; }
    double tempo = 120.0, ppq = 0.0;
    int64_t position = samplePosition;
    bool playing = false;
    if (auto* playhead = getPlayHead())
        if (auto state = playhead->getPosition())
        {
            tempo = state->getBpm().orFallback (120.0);
            ppq = state->getPpqPosition().orFallback (0.0);
            // Preserve preroll continuity: clamping negative positions to zero
            // makes each callback look like a seek and resets pending notes.
            position = state->getTimeInSamples().orFallback (samplePosition);
            playing = state->getIsPlaying();
        }
    if (! std::isfinite (tempo) || tempo <= 0.0) tempo = 120.0;
    if (! std::isfinite (ppq)) ppq = 0.0;
    for (int offset = 0; offset < buffer.getNumSamples(); offset += CADENCE_MAX_BLOCK)
    {
        const int frames = juce::jmin (CADENCE_MAX_BLOCK, buffer.getNumSamples() - offset);
        const auto mainInput = inputs > 0 ? getBusBuffer (buffer, true, 0) : juce::AudioBuffer<float>();
        const auto sideBus = sidechainInputs > 0 ? sidechainBusIndex() : -1;
        const auto sideInput = sideBus >= 0 ? getBusBuffer (buffer, true, sideBus) : juce::AudioBuffer<float>();
        for (int channel = 0; channel < inputs; ++channel)
        {
            auto* target = inputStorage.data() + channel * frames;
            if (channel < mainInput.getNumChannels()) std::copy_n (mainInput.getReadPointer (channel, offset), frames, target);
            else std::fill_n (target, frames, 0.0f);
        }
        for (int channel = 0; channel < sidechainInputs; ++channel)
        {
            auto* target = sidechainStorage.data() + channel * frames;
            if (channel < sideInput.getNumChannels()) std::copy_n (sideInput.getReadPointer (channel, offset), frames, target);
            else std::fill_n (target, frames, 0.0f);
        }
        uint32_t count = 0;
        for (const auto metadata : midi)
        {
            if (metadata.samplePosition < offset || metadata.samplePosition >= offset + frames) continue;
            const auto* data = metadata.data;
            if (metadata.numBytes < 1 || data[0] < 0x80 || data[0] >= 0xf0) continue;
            const auto kind = data[0] & 0xf0;
            if (metadata.numBytes < (kind == 0xc0 || kind == 0xd0 ? 2 : 3)) continue;
            if (count == CADENCE_MAX_EVENTS) { failed.store (true); buffer.clear(); midi.clear(); return; }
            CadenceEvent event {};
            event.frame = static_cast<uint32_t> (metadata.samplePosition - offset);
            event.channel = static_cast<uint16_t> (data[0] & 0x0f);
            event.data1 = data[1]; event.data2 = metadata.numBytes >= 3 ? data[2] : 0;
            event.value = static_cast<float> (event.data2) / 127.0f;
            switch (kind)
            {
                case 0x80: event.kind = 1; break;
                // Keep note-on velocity zero as NoteOn; the sampler
                // distinguishes it so the synthesized release velocity is
                // 0x8000 rather than an explicit note-off's data byte.
                case 0x90: event.kind = 0; break;
                case 0xa0: event.kind = 2; break;
                case 0xb0: event.kind = 3; break;
                case 0xc0: event.kind = 4; event.value = static_cast<float> (event.data1) / 127.0f; break;
                case 0xd0: event.kind = 6; event.value = static_cast<float> (event.data1) / 127.0f; break;
                case 0xe0: event.kind = 5; event.value = static_cast<float> ((data[2] << 7) | data[1]) / 8192.0f - 1.0f; break;
                default: continue;
            }
            eventStorage[count++] = event;
        }
        // Parameter updates precede MIDI at frame zero. Other updates are
        // deferred if a dense MIDI block consumes this block's event budget.
        uint32_t parameterCount = 0;
        for (auto* parameter : controls)
        {
            if (count + parameterCount >= CADENCE_MAX_EVENTS) break;
            if (parameter->dirty.exchange (false, std::memory_order_acquire))
            {
                const auto event = CadenceEvent { 0, 7, 0, parameter->id, 0, 0, parameter->convertTo0to1 (parameter->get()) };
                // Shift only once below; temporary events occupy spare tail.
                eventStorage[count + parameterCount++] = event;
            }
        }
        if (parameterCount > 0)
            std::rotate (eventStorage.begin(), eventStorage.begin() + count, eventStorage.begin() + count + parameterCount);
        count += parameterCount;
        auto append = [&] (CadenceEvent event) {
            if (count == CADENCE_MAX_EVENTS) return false;
            eventStorage[count++] = event; return true;
        };
        bool automationFits = true;
        for (size_t i = 0; i < automationCount; ++i)
        {
            const auto& event = automationStorage[i];
            const auto id = controls[static_cast<size_t> (event.parameter)]->id;
            if (event.duration == 0)
            {
                if (event.frame >= offset && event.frame < offset + frames)
                    automationFits &= append ({ static_cast<uint32_t> (event.frame - offset), 7, 0, id, 0, 0, event.value });
                continue;
            }
            const int end = event.frame + event.duration;
            if (end < offset || event.frame >= offset + frames) continue;
            auto valueAt = [&] (int frame) { return static_cast<float> (event.start + (event.value - event.start)
                * juce::jlimit (0.0, 1.0, static_cast<double> (frame - event.frame) / event.duration)); };
            const int first = juce::jmax (offset, event.frame);
            const int last = juce::jmin (offset + frames, end);
            if (last > first)
            {
                automationFits &= append ({ static_cast<uint32_t> (first - offset), 7, 0, id, 0, 0, valueAt (first) });
                automationFits &= append ({ static_cast<uint32_t> (first - offset), 8, 0, id,
                    static_cast<uint16_t> (last - first), 0, valueAt (last) });
            }
            if (end >= offset && end < offset + frames)
                automationFits &= append ({ static_cast<uint32_t> (end - offset), 7, 0, id, 0, 0, event.value });
        }
        if (! automationFits) { failed.store (true); automationCount = 0; buffer.clear(); midi.clear(); return; }
        // Stable tie order without allocating: ordinary dirty values first,
        // explicit timed values second, and parameters before same-frame MIDI.
        for (uint32_t i = 0; i < count; ++i) eventStorage[i].padding = static_cast<uint16_t> (i);
        std::sort (eventStorage.begin(), eventStorage.begin() + count, [] (const auto& a, const auto& b) {
            if (a.frame != b.frame) return a.frame < b.frame;
            const bool ap = a.kind == 7 || a.kind == 8, bp = b.kind == 7 || b.kind == 8;
            return ap != bp ? ap : a.padding < b.padding;
        });
        for (uint32_t i = 0; i < count; ++i) eventStorage[i].padding = 0;
        CadenceContext context { rate, static_cast<uint32_t> (frames), tempo, (playing ? 1u : 0u) | (isNonRealtime() ? 2u : 0u),
                                 position + static_cast<int64_t> (offset), ppq + offset * tempo / (60.0 * rate) };
        if (cadence_bridge_process_sidechain (handle, &context, inputStorage.data(), static_cast<uint32_t> (inputs),
                sidechainStorage.data(), static_cast<uint32_t> (sidechainInputs),
                outputStorage.data(), static_cast<uint32_t> (outputs), static_cast<uint32_t> (frames), eventStorage.data(), count) != 0)
        {
            failed.store (true); buffer.clear(); midi.clear(); return;
        }
        takeOutputMidi (offset);
        for (int channel = 0; channel < buffer.getNumChannels(); ++channel)
        {
            if (channel < outputs) std::copy_n (outputStorage.data() + channel * frames, frames, buffer.getWritePointer (channel, offset));
            else buffer.clear (channel, offset, frames);
        }
    }
    automationCount = 0;
    samplePosition += static_cast<int64_t> (buffer.getNumSamples());
    midi.clear();
    emitPendingMidi (midi, buffer.getNumSamples());
    std::swap (pendingMidi, nextMidi);
    nextMidi->count = 0;
    nextMidi->sysexLength = 0;
    CadenceDynamicStats dynamic {};
    if (cadence_bridge_dynamic_stats (handle, &dynamic) == 0
        && (static_cast<int> (dynamic.plugin_latency_frames) != lastDynamicLatency
            || static_cast<int> (dynamic.plugin_tail_frames) != lastDynamicTail))
    {
        lastDynamicLatency = static_cast<int> (dynamic.plugin_latency_frames);
        lastDynamicTail = static_cast<int> (dynamic.plugin_tail_frames);
        triggerAsyncUpdate();
    }
    const auto generation = cadence_bridge_parameter_generation (handle);
    if (generation != lastParameterGeneration)
    {
        lastParameterGeneration = generation;
        triggerAsyncUpdate();
    }
}

bool RustPluginProcessor::queueParameterPoint (int parameter, int frame, float value) noexcept
{
    if (! juce::isPositiveAndBelow (parameter, static_cast<int> (controls.size())) || controls[static_cast<size_t> (parameter)]->readOnly
        || frame < 0 || ! std::isfinite (value) || automationCount == automationStorage.size()) return false;
    automationStorage[automationCount++] = { parameter, frame, 0, juce::jlimit (0.0f, 1.0f, value), value }; return true;
}
bool RustPluginProcessor::queueParameterRamp (int parameter, int frame, int duration, float end) noexcept
{
    if (duration <= 0 || duration > 1'048'576 || automationCount == automationStorage.size() || ! std::isfinite (end)) return false;
    for (size_t i = automationCount; i > 0; --i)
    {
        const auto& start = automationStorage[i - 1];
        if (start.parameter == parameter && start.frame == frame && start.duration == 0)
        { automationStorage[automationCount++] = { parameter, frame, duration, juce::jlimit (0.0f, 1.0f, end), start.value }; return true; }
    }
    return false;
}
void RustPluginProcessor::setTimedParameterValue (int parameter, float value) noexcept
{
    if (juce::isPositiveAndBelow (parameter, static_cast<int> (controls.size())) && std::isfinite (value))
        controls[static_cast<size_t> (parameter)]->timed (juce::jlimit (0.0f, 1.0f, value));
}
void RustPluginProcessor::refreshTimedParameterListeners()
{
    for (auto* parameter : controls) if (parameter->uiPending.exchange (false))
        parameter->sendValueChangedMessageToListeners (static_cast<juce::AudioProcessorParameter*> (parameter)->getValue());
}

juce::AudioProcessorEditor* RustPluginProcessor::createEditor() { return new RustEditor (*this, formatName); }

void RustPluginProcessor::getStateInformation (juce::MemoryBlock& destination)
{
    const juce::ScopedLock lock (getCallbackLock());
    uint32_t schema = 0;
    const auto size = cadence_bridge_save (handle, stateStorage.data(), stateStorage.size(), &schema);
    stateError.store (size < 0);
    if (size < 0) { destination = lastState; return; }
    juce::XmlElement xml ("CADENCE_RUST_STATE");
    xml.setAttribute ("schema", static_cast<int> (schema));
    xml.setAttribute ("data", juce::MemoryBlock (stateStorage.data(), static_cast<size_t> (size)).toBase64Encoding());
    for (auto* parameter : controls)
    {
        auto* p = xml.createNewChildElement ("PARAM");
        p->setAttribute ("id", parameter->paramID);
        p->setAttribute ("value", parameter->convertTo0to1 (parameter->get()));
    }
    copyXmlToBinary (xml, destination);
    lastState = destination;
}

void RustPluginProcessor::setStateInformation (const void* data, int size)
{
    const juce::ScopedLock lock (getCallbackLock());
    restoreState (data, size, nullptr);
}

bool RustPluginProcessor::restoreState (const void* data, int size, const std::atomic<bool>* cancelled)
{
    auto xml = getXmlFromBinary (data, size);
    if (xml == nullptr || ! xml->hasTagName ("CADENCE_RUST_STATE")) return false;
    juce::MemoryBlock state;
    if (! state.fromBase64Encoding (xml->getStringAttribute ("data")) || state.getSize() > CADENCE_MAX_STATE) return false;
    if (cadence_bridge_load_cancellable (handle, static_cast<const uint8_t*> (state.getData()), state.getSize(),
                             static_cast<uint32_t> (xml->getIntAttribute ("schema")), checkCancelled,
                             const_cast<std::atomic<bool>*> (cancelled)) != 0)
    {
        failed.store (true); return false;
    }
    for (auto* entry : xml->getChildIterator())
        for (auto* parameter : controls)
            if (entry->getStringAttribute ("id") == parameter->paramID)
                parameter->setValueNotifyingHost (juce::jlimit (0.0f, 1.0f, static_cast<float> (entry->getDoubleAttribute ("value"))));
    lastState.replaceAll (data, static_cast<size_t> (size));
    return true;
}
}
