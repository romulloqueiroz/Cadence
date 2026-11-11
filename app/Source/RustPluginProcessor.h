#pragma once
#include <juce_audio_processors/juce_audio_processors.h>
#include <cadence_bridge.h>

namespace cadence
{
class RustPluginProcessor final : public juce::AudioProcessor, private juce::AsyncUpdater, private juce::Timer
{
public:
    static std::unique_ptr<RustPluginProcessor> open (const juce::PluginDescription&, juce::String& error, const std::atomic<bool>* cancelled = nullptr);
    static juce::PluginDescription describe (const juce::String& source, const juce::String& format,
                                            juce::String& error, const juce::String& selector = {});
    static std::vector<juce::PluginDescription> describeAll (const juce::String& source,
                                                             const juce::String& format,
                                                             juce::String& error);
    bool prepareForLoading (double, int, const juce::MemoryBlock&, const std::atomic<bool>*);
    static bool supports (const juce::String& format);
    static juce::File workerFile();
    ~RustPluginProcessor() override;
    const juce::String getName() const override { return name; }
    void prepareToPlay (double, int) override;
    void releaseResources() override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    bool isBusesLayoutSupported (const BusesLayout&) const override;
    bool acceptsMidi() const override { return true; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return tail; }
    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}
    void getStateInformation (juce::MemoryBlock&) override;
    void setStateInformation (const void*, int) override;
    bool hasFailed() const;
    juce::String failureReason() const;
    uint64_t underruns() const;
    uint64_t pipelineUnderruns() const;
    bool streamingStats (CadenceStreamingStats&) const;
    bool realtimeSchedulingActive() const;
    void setAudioWorkgroup (void* workgroup, uint32_t generation);
    bool audioWorkgroupActive() const;
    bool needsReload() const { return cadence_bridge_reload_requested (handle) != 0; }
    void setNativeEditorOpen (bool open);
    uint32_t nativeEditorState() const;
    int mainInputChannels() const { return inputs; }
    int sidechainInputChannels() const { return sidechainInputs; }
    int sidechainBusIndex() const { return inputs > 0 ? 1 : 0; }
    void refreshDynamicTiming();
    void refreshParameterValues();
    void takeOutputMidi (int blockOffset);
    void emitPendingMidi (juce::MidiBuffer&, int frames);
    bool producesMidi() const override { return true; }
    bool queueParameterPoint (int parameter, int frame, float normalized) noexcept;
    bool queueParameterRamp (int parameter, int frame, int duration, float endNormalized) noexcept;
    void setTimedParameterValue (int parameter, float normalized) noexcept;
    void clearQueuedAutomation() noexcept { automationCount = 0; }
    void refreshTimedParameterListeners();
    bool hasStateError() const { return stateError.load(); }
    static std::vector<juce::PluginDescription> bundledDescriptions();
    uint32_t workerId() const { return cadence_bridge_worker_id (handle); }

private:
    struct Parameter;
    RustPluginProcessor (CadenceBridge*, const juce::var& metadata, juce::String format);
    static CadenceBridge* connect (const juce::String& source, const juce::String& selector, juce::String& error, const std::atomic<bool>* cancelled = nullptr);
    void updatePreparedDetails (double, int);
    void handleAsyncUpdate() override;
    void timerCallback() override;
    bool restoreState (const void*, int, const std::atomic<bool>*);
    static juce::var metadata (CadenceBridge*);

    CadenceBridge* handle = nullptr;
    juce::String name, formatName;
    int inputs = 0, outputs = 2, sidechainInputs = 0, intrinsicLatency = 0, preparedBlock = 512;
    double rate = 48000.0, tail = 0.0, tailSamples = 0.0;
    int64_t samplePosition = 0;
    int lastDynamicLatency = 0, lastDynamicTail = 0;
    int polledLatency = 0, polledTail = 0;
    uint64_t lastParameterGeneration = 0;
    struct OutputMidi
    {
        std::array<CadenceEvent, CADENCE_MAX_EVENTS> events {};
        uint32_t count = 0;
        std::array<uint8_t, CADENCE_MAX_OUTPUT_SYSEX> sysex {};
        uint32_t sysexLength = 0;
    };
    std::unique_ptr<OutputMidi> pendingMidi = std::make_unique<OutputMidi>();
    std::unique_ptr<OutputMidi> nextMidi = std::make_unique<OutputMidi>();
    std::atomic<bool> failed { false }, stateError { false };
    bool preparedForAttachment = false;
    std::vector<Parameter*> controls;
    std::vector<float> inputStorage, sidechainStorage, outputStorage;
    std::array<CadenceEvent, CADENCE_MAX_EVENTS> eventStorage {};
    struct ParameterEvent { int parameter = 0, frame = 0, duration = 0; float value = 0, start = 0; };
    std::array<ParameterEvent, CADENCE_MAX_EVENTS> automationStorage {};
    size_t automationCount = 0;
    std::array<uint8_t, CADENCE_MAX_STATE> stateStorage {};
    juce::MemoryBlock lastState;
};
}
