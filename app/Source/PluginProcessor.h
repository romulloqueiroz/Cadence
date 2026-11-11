#pragma once

#include <JuceHeader.h>

#include "PluginScanner.h"
#include "PluginLoader.h"
#include "Automation.h"

class CadenceAudioProcessor : public juce::AudioProcessor,
                               private juce::AsyncUpdater, public cadence::HostAutomationClient
{
public:
    CadenceAudioProcessor();
    explicit CadenceAudioProcessor (cadence::PluginScanner::Options);
    ~CadenceAudioProcessor() override;

    void prepareToPlay (double sampleRate, int samplesPerBlock) override;
    void releaseResources() override;
    void setNonRealtime (bool) noexcept override;
    void audioWorkgroupContextChanged (const juce::AudioWorkgroup&) override;
    uint64_t getUnderruns() const;
    bool isBusesLayoutSupported (const BusesLayout& layouts) const override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }

    const juce::String getName() const override { return "Cadence"; }
    bool acceptsMidi() const override { return true; }
    bool producesMidi() const override { return true; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return tailSeconds.load(); }

    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

    void getStateInformation (juce::MemoryBlock& destData) override;
    void setStateInformation (const void* data, int sizeInBytes) override;

    void scanPlugins();
    void cancelScan();
    void skipCurrentPlugin();
    void retrySkippedPlugin (const cadence::ScanCandidate&);
    std::vector<cadence::SkippedPlugin> getSkippedPlugins() const;
    size_t getSkippedPluginCount() const { return scanner->skippedCount(); }
    bool canSkipPlugin() const;
    const std::vector<juce::PluginDescription>& getPlugins() const { return plugins; }

    bool addPlugin (const juce::PluginDescription& description, juce::String& error);
    void loadPluginAsync (const juce::PluginDescription&);
    void cancelLoading();
    bool isLoading() const { return loadingCount.load() > 0 || restorePending.load(); }
    juce::String getSlotLoadStatus (int) const;
    std::vector<cadence::Automation::Target> getAutomationTargets (int slot) const;
    bool assignAutomation (int control, int slot, const juce::String& key, juce::String& error);
    void clearAutomation (int control);
    juce::String getAutomationAssignment (int control) const;
    cadence::Automation::Assignment automationAssignment (int control) const { return automation->assignment (control); }
    bool queueHostAutomation (int, int, float) noexcept override;
    bool queueHostAutomationRamp (int, int, int, float, float) noexcept override;
    void removeSlot (int index);
    void moveSlot (int index, int delta);
    void setSlotBypassed (int index, bool bypassed);
    bool isSlotBypassed (int index) const;
    bool isSlotSidechainRouted (int index) const;
    bool isSlotMidiOutputRouted (int index) const;
    int getNumSlots() const;
    juce::String getSlotName (int index) const;
    juce::String getSlotUuid (int index) const;
    juce::PluginDescription getSlotDescription (int index) const;
    juce::AudioProcessor* getSlotProcessor (int index) const;
    juce::AudioProcessorEditor* createSlotEditor (int index);

    float getOutputLevel (int channel) const;
    juce::String getStatus() const { return status; }
    void setStatus (const juce::String& text) { status = text; }
    uint32_t getPluginsVersion() const { return pluginsVersion.load (std::memory_order_relaxed); }
    uint32_t getChainVersion() const { return chainVersion.load (std::memory_order_relaxed); }
    bool isScanning() const { return scanner->isRunning(); }

private:
    struct Slot
    {
        juce::AudioProcessorGraph::Node::Ptr node;
        juce::PluginDescription description;
        juce::String uuid = juce::Uuid().toString();
        bool bypassed = false;
        uint64_t loadId = 0;
        bool loading = false;
        juce::MemoryBlock pendingState;
        juce::String loadError;
        std::shared_ptr<std::atomic<bool>> cancelled;
    };

    void handleAsyncUpdate() override;
    void applyAudioWorkgroupTo (juce::AudioProcessor*);
    void reloadChangedSlots();
    void reloadSlot (int index);
    void queueSlotLoad (uint64_t);
    void applyPendingState();
    void loadCache();
    void saveCache();
    juce::File cacheFile() const;
    void rebuildConnections();
    void rebuildConnectionsLocked();
    bool hasPlugin (const juce::PluginDescription&) const;

    juce::AudioPluginFormatManager formats;
    juce::AudioProcessorGraph graph;
    juce::AudioProcessorGraph::Node::Ptr audioInputNode;
    juce::AudioProcessorGraph::Node::Ptr audioOutputNode;
    juce::AudioProcessorGraph::Node::Ptr midiInputNode;
    juce::AudioProcessorGraph::Node::Ptr midiOutputNode;

    std::vector<Slot> chain;
    std::unique_ptr<cadence::Automation> automation;
    std::vector<juce::PluginDescription> plugins;
    juce::SpinLock graphLock;
    juce::AudioBuffer<float> sidechainInputPreserve;
    std::atomic<float> outputLevel[2] { { 0.0f }, { 0.0f } };
    juce::String status;
    std::atomic<double> tailSeconds { 0.0 };
    std::unique_ptr<cadence::PluginLoader> loader;
    uint64_t nextLoadId = 1;
    std::atomic<uint64_t> configurationGeneration { 0 };
    std::atomic<int> loadingCount { 0 };
    juce::CriticalSection restoreMutex;
    juce::MemoryBlock requestedState;
    uint64_t restoreRevision = 0;
    std::atomic<bool> restorePending { false };
    std::atomic<bool> loadOffline { false };
    std::atomic<bool> reloadPending { false };
    juce::String lastScannerStatus;

    juce::File settingsDirectory;
    std::unique_ptr<cadence::PluginScanner> scanner;
    uint64_t scanCatalogueVersion = 0;
    std::atomic<uint32_t> pluginsVersion { 0 };
    std::atomic<uint32_t> chainVersion { 0 };
    std::atomic<void*> pendingAudioWorkgroup { nullptr };
    std::atomic<uint64_t> workgroupRevision { 0 };
    void* currentAudioWorkgroup = nullptr;
    uint32_t audioWorkgroupGeneration = 0;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (CadenceAudioProcessor)
};
