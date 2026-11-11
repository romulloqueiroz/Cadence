#include "PluginProcessor.h"
#include "PluginEditor.h"
#include "RustPluginProcessor.h"
#if JUCE_MAC
 #include <os/workgroup.h>
 #include <juce_audio_basics/native/juce_AudioWorkgroup_mac.h>
#endif

namespace
{
struct InputRouting
{
    int mainChannels = 0, sidechainChannels = 0;
    std::array<int, 8> sidechainBuses {};
    int sidechainBusCount = 0;
};

InputRouting getInputRouting (juce::AudioProcessor* processor)
{
    InputRouting routing;
    if (processor == nullptr)
        return routing;
    if (auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (processor))
    {
        routing.mainChannels = rust->mainInputChannels();
        if (rust->sidechainInputChannels() > 0)
        {
            routing.sidechainBuses[static_cast<size_t> (routing.sidechainBusCount++)] = rust->sidechainBusIndex();
            routing.sidechainChannels = rust->sidechainInputChannels();
        }
        return routing;
    }

    const auto buses = processor->getBusCount (true);
    auto isSidechain = [] (const juce::String& name)
    {
        return name.containsIgnoreCase ("sidechain") || name.containsIgnoreCase ("side-chain");
    };
    for (int bus = 0; bus < buses; ++bus)
    {
        const auto channels = processor->getChannelCountOfBus (true, bus);
        if (channels <= 0) continue;
        const auto* info = processor->getBus (true, bus);
        if (info != nullptr && isSidechain (info->getName()))
        {
            if (routing.sidechainBusCount < static_cast<int> (routing.sidechainBuses.size()))
                routing.sidechainBuses[static_cast<size_t> (routing.sidechainBusCount++)] = bus;
            routing.sidechainChannels += channels;
        }
        else if (routing.mainChannels == 0)
            routing.mainChannels = channels;
    }
    if (routing.sidechainBusCount == 0 && buses > 1)
    {
        for (int bus = 1; bus < buses; ++bus)
        {
            const auto channels = processor->getChannelCountOfBus (true, bus);
            if (channels <= 0) continue;
            if (routing.sidechainBusCount < static_cast<int> (routing.sidechainBuses.size()))
                routing.sidechainBuses[static_cast<size_t> (routing.sidechainBusCount++)] = bus;
            routing.sidechainChannels += channels;
        }
    }
    return routing;
}
}

CadenceAudioProcessor::CadenceAudioProcessor()
    : CadenceAudioProcessor ({ cadence::PluginScanner::bundledHelper(),
                              juce::File::getSpecialLocation (juce::File::userApplicationDataDirectory)
                                  .getChildFile ("Application Support/Cadence") })
{
    const auto legacy = juce::File::getSpecialLocation (juce::File::userApplicationDataDirectory)
                            .getChildFile ("Cadence/plugins.xml");
    if (! cacheFile().existsAsFile() && legacy.existsAsFile())
    {
        settingsDirectory.createDirectory();
        if (legacy.copyFileTo (cacheFile()))
            loadCache();
    }
    for (const auto& description : cadence::RustPluginProcessor::bundledDescriptions())
        if (! hasPlugin (description)) plugins.push_back (description);
    setStatus (juce::String (plugins.size()) + " plugins ready");
    pluginsVersion.fetch_add (1, std::memory_order_relaxed);
}

CadenceAudioProcessor::CadenceAudioProcessor (cadence::PluginScanner::Options options)
    : AudioProcessor (BusesProperties()
                          .withInput ("Sidechain", juce::AudioChannelSet::stereo(), false)
                          .withOutput ("Output", juce::AudioChannelSet::stereo(), true)),
      settingsDirectory (options.directory)
{
    automation = std::make_unique<cadence::Automation> (*this);
    formats.addFormat (std::make_unique<juce::VST3PluginFormat>());
   #if JUCE_MAC && JUCE_PLUGINHOST_AU
    formats.addFormat (std::make_unique<juce::AudioUnitPluginFormat>());
   #endif

    audioInputNode = graph.addNode (
        std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::audioInputNode));
    audioOutputNode = graph.addNode (
        std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::audioOutputNode));
    midiInputNode = graph.addNode (
        std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::midiInputNode));
    midiOutputNode = graph.addNode (
        std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::midiOutputNode));

    setLatencySamples (0);
    loadCache();
    scanner = std::make_unique<cadence::PluginScanner> (
        std::move (options),
        [this] { triggerAsyncUpdate(); });
    lastScannerStatus = scanner->snapshot().status;
    loader = std::make_unique<cadence::PluginLoader> ([this] { triggerAsyncUpdate(); });
}

CadenceAudioProcessor::~CadenceAudioProcessor()
{
    scanner.reset();
    loader.reset();
    cancelPendingUpdate();
   #if JUCE_MAC && JUCE_AUDIOWORKGROUP_TYPES_AVAILABLE
    auto* clearSentinel = reinterpret_cast<void*> (static_cast<uintptr_t> (1));
    if (auto* pending = pendingAudioWorkgroup.exchange (nullptr))
        if (pending != clearSentinel) os_release (pending);
    if (currentAudioWorkgroup != nullptr) { os_release (currentAudioWorkgroup); currentAudioWorkgroup = nullptr; }
   #endif
    automation.reset();
}

void CadenceAudioProcessor::audioWorkgroupContextChanged (const juce::AudioWorkgroup& workgroup)
{
   #if JUCE_MAC && JUCE_AUDIOWORKGROUP_TYPES_AVAILABLE
    auto* native = juce::getRealAudioWorkgroupHandle (workgroup);
    auto* clearSentinel = reinterpret_cast<void*> (static_cast<uintptr_t> (1));
    auto* value = native != nullptr ? native : clearSentinel;
    if (native != nullptr) os_retain (native);
    if (auto* previous = pendingAudioWorkgroup.exchange (value))
        if (previous != clearSentinel) os_release (previous);
    workgroupRevision.fetch_add (1, std::memory_order_acq_rel);
    // Coalesce updates on the message thread; no allocation or lock is taken
    // on the host audio callback.
    triggerAsyncUpdate();
   #else
    juce::ignoreUnused (workgroup);
   #endif
}

void CadenceAudioProcessor::applyAudioWorkgroupTo (juce::AudioProcessor* processor)
{
    if (auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (processor))
        rust->setAudioWorkgroup (currentAudioWorkgroup, audioWorkgroupGeneration);
}

void CadenceAudioProcessor::prepareToPlay (double sampleRate, int samplesPerBlock)
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    setRateAndBufferSizeDetails (sampleRate, samplesPerBlock);
    configurationGeneration.fetch_add (1);
    graph.setNonRealtime (isNonRealtime());
    graph.setPlayConfigDetails (getTotalNumInputChannels(), 2, sampleRate, samplesPerBlock);
    graph.prepareToPlay (sampleRate, samplesPerBlock);
    sidechainInputPreserve.setSize (juce::jmax (2, getTotalNumInputChannels()), samplesPerBlock);

    for (auto& slot : chain)
        if (auto* processor = slot.node != nullptr ? slot.node->getProcessor() : nullptr)
            processor->prepareToPlay (sampleRate, samplesPerBlock);

    rebuildConnectionsLocked();
}

void CadenceAudioProcessor::setNonRealtime (bool offline) noexcept
{
    if (offline == isNonRealtime()) return;
    AudioProcessor::setNonRealtime (offline);
    loadOffline.store (offline);
    configurationGeneration.fetch_add (1);
    graph.setNonRealtime (offline);
}

uint64_t CadenceAudioProcessor::getUnderruns() const
{
    uint64_t total = 0;
    for (const auto& slot : chain)
        if (auto* bridge = dynamic_cast<cadence::RustPluginProcessor*> (slot.node != nullptr ? slot.node->getProcessor() : nullptr)) total += bridge->underruns();
    return total;
}

void CadenceAudioProcessor::releaseResources()
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    graph.releaseResources();
}

bool CadenceAudioProcessor::isBusesLayoutSupported (const BusesLayout& layouts) const
{
    const auto input = layouts.getMainInputChannelSet();
    const auto output = layouts.getMainOutputChannelSet();
    return output == juce::AudioChannelSet::stereo()
        && (input == juce::AudioChannelSet::disabled()
            || input == juce::AudioChannelSet::stereo());
}

void CadenceAudioProcessor::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi)
{
    juce::ScopedNoDenormals noDenormals;
    const juce::SpinLock::ScopedTryLockType lock (graphLock);
    if (! lock.isLocked()) { buffer.clear(); midi.clear(); return; }

    const int frames = buffer.getNumSamples();
    auto input = getTotalNumInputChannels() > 0 ? getBusBuffer (buffer, true, 0) : juce::AudioBuffer<float>();
    if (input.getNumChannels() > 0
        && (sidechainInputPreserve.getNumChannels() < input.getNumChannels()
            || sidechainInputPreserve.getNumSamples() < frames))
    {
        buffer.clear();
        midi.clear();
        return;
    }
    if (input.getNumChannels() > 0)
        for (int channel = 0; channel < input.getNumChannels(); ++channel)
            sidechainInputPreserve.copyFrom (channel, 0, input, channel, 0, frames);

    buffer.clear();
    if (restorePending.load()) { midi.clear(); return; }
    if (! automation->beginBlock (frames)) { midi.clear(); return; }
    if (input.getNumChannels() > 0)
    {
        for (int channel = 0; channel < input.getNumChannels(); ++channel)
            input.copyFrom (channel, 0, sidechainInputPreserve, channel, 0, frames);
        // Writing through the bus view does not update the parent buffer's
        // cleared flag; the graph's input node would otherwise discard it.
        buffer.setNotClear();
    }
    graph.setPlayHead (getPlayHead());

    if (graph.getSampleRate() > 0.0)
        graph.processBlock (buffer, midi);

    bool reload = false;
    for (auto& slot : chain)
    {
        if (auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (
                slot.node != nullptr ? slot.node->getProcessor() : nullptr))
            if (rust->needsReload()) { reload = true; break; }
    }
    if (reload && ! reloadPending.exchange (true))
        triggerAsyncUpdate();

    for (int channel = 0; channel < 2; ++channel)
    {
        const auto level = channel < buffer.getNumChannels()
                               ? buffer.getMagnitude (channel, 0, buffer.getNumSamples())
                               : 0.0f;
        outputLevel[channel].store (level, std::memory_order_relaxed);
    }
}

juce::AudioProcessorEditor* CadenceAudioProcessor::createEditor()
{
    return new CadenceAudioProcessorEditor (*this);
}

void CadenceAudioProcessor::rebuildConnections()
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    rebuildConnectionsLocked();
}

void CadenceAudioProcessor::rebuildConnectionsLocked()
{
    for (const auto& connection : graph.getConnections())
        graph.removeConnection (connection);

    if (audioOutputNode == nullptr || midiInputNode == nullptr || midiOutputNode == nullptr)
        return;

    const int graphInputChannels = getTotalNumInputChannels();
    std::vector<juce::AudioProcessorGraph::Node::Ptr> active;

    for (auto& slot : chain)
    {
        if (slot.bypassed || slot.node == nullptr)
            continue;

        active.push_back (slot.node);
        graph.addConnection ({ { midiInputNode->nodeID, juce::AudioProcessorGraph::midiChannelIndex },
                               { slot.node->nodeID, juce::AudioProcessorGraph::midiChannelIndex } });
        if (slot.node->getProcessor() != nullptr && slot.node->getProcessor()->producesMidi())
            graph.addConnection ({ { slot.node->nodeID, juce::AudioProcessorGraph::midiChannelIndex },
                                   { midiOutputNode->nodeID, juce::AudioProcessorGraph::midiChannelIndex } });
    }

    int latency = 0;
    double tail = 0.0;
    for (const auto& node : active)
    {
        latency += node->getProcessor()->getLatencySamples();
        tail += node->getProcessor()->getTailLengthSeconds();
    }
    setLatencySamples (latency);
    tailSeconds.store (tail);

    juce::AudioProcessorGraph::Node::Ptr previous;

    for (auto& node : active)
    {
        auto* processor = node->getProcessor();
        const auto routing = getInputRouting (processor);
        const int mainChannels = routing.mainChannels;

        if (previous != nullptr && processor != nullptr && mainChannels > 0)
        {
            const auto channels = juce::jmin (2, mainChannels,
                                              previous->getProcessor()->getTotalNumOutputChannels());

            for (int channel = 0; channel < channels; ++channel)
                graph.addConnection ({ { previous->nodeID, channel }, { node->nodeID, channel } });
        }

        if (audioInputNode != nullptr && processor != nullptr && graphInputChannels > 0)
        {
            for (int side = 0; side < routing.sidechainBusCount; ++side)
            {
                const auto bus = routing.sidechainBuses[static_cast<size_t> (side)];
                const auto busChannels = processor->getChannelCountOfBus (true, bus);
                const int offset = processor->getChannelIndexInProcessBlockBuffer (true, bus, 0);
                const auto channels = juce::jmin (2, busChannels, graphInputChannels);
                for (int channel = 0; channel < channels; ++channel)
                    graph.addConnection ({ { audioInputNode->nodeID, channel },
                                           { node->nodeID, offset + channel } });
            }
        }

        previous = node;
    }

    if (previous != nullptr)
    {
        const auto channels = juce::jmin (2, previous->getProcessor()->getTotalNumOutputChannels());
        for (int channel = 0; channel < channels; ++channel)
            graph.addConnection ({ { previous->nodeID, channel }, { audioOutputNode->nodeID, channel } });
    }
}

bool CadenceAudioProcessor::isSlotMidiOutputRouted (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))
        || chain[static_cast<size_t> (index)].node == nullptr || midiOutputNode == nullptr)
        return false;
    for (const auto& connection : graph.getConnections())
        if (connection.source.nodeID == chain[static_cast<size_t> (index)].node->nodeID
            && connection.source.channelIndex == juce::AudioProcessorGraph::midiChannelIndex
            && connection.destination.nodeID == midiOutputNode->nodeID)
            return true;
    return false;
}

bool CadenceAudioProcessor::isSlotSidechainRouted (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())) || chain[static_cast<size_t> (index)].node == nullptr)
        return false;
    auto* processor = chain[static_cast<size_t> (index)].node->getProcessor();
    const auto routing = getInputRouting (processor);
    if (routing.sidechainBusCount == 0)
        return false;
    const auto connections = graph.getConnections();
    for (const auto& connection : connections)
    {
        if (connection.destination.nodeID != chain[static_cast<size_t> (index)].node->nodeID)
            continue;
        for (int side = 0; side < routing.sidechainBusCount; ++side)
        {
            const auto bus = routing.sidechainBuses[static_cast<size_t> (side)];
            const int offset = processor->getChannelIndexInProcessBlockBuffer (true, bus, 0);
            const int channels = processor->getChannelCountOfBus (true, bus);
            if (connection.destination.channelIndex >= offset
                && connection.destination.channelIndex < offset + channels)
                return true;
        }
    }
    return false;
}

void CadenceAudioProcessor::scanPlugins()
{
    if (isScanning())
        return;
    handleAsyncUpdate();
    scanner->start (cadence::PluginScanner::discoverCandidates(), plugins);
}

void CadenceAudioProcessor::cancelScan() { scanner->cancel(); }
void CadenceAudioProcessor::skipCurrentPlugin() { scanner->skipCurrent(); }

void CadenceAudioProcessor::retrySkippedPlugin (const cadence::ScanCandidate& candidate)
{
    if (isScanning())
        return;
    handleAsyncUpdate();
    scanner->start ({ candidate }, plugins, true);
}

std::vector<cadence::SkippedPlugin> CadenceAudioProcessor::getSkippedPlugins() const
{
    return scanner->snapshot().skipped;
}

bool CadenceAudioProcessor::canSkipPlugin() const
{
    return scanner->canSkip();
}

void CadenceAudioProcessor::handleAsyncUpdate()
{
    applyPendingState();
    if (reloadPending.exchange (false))
        reloadChangedSlots();
    std::vector<uint64_t> restart;
    std::vector<cadence::RustPluginProcessor*> newWorkers;
    for (auto& result : loader->takeCompleted())
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        const auto found = std::find_if (chain.begin(), chain.end(), [&] (const auto& slot) { return slot.loadId == result.request.id; });
        if (found == chain.end() || ! found->loading || result.request.cancelled->load()) continue;
        if (result.request.settings.generation != configurationGeneration.load())
        {
            restart.push_back (found->loadId);
            continue;
        }
        found->loading = false;
        loadingCount.fetch_sub (1);
        if (result.processor != nullptr)
        {
            found->node = graph.addNode (std::move (result.processor));
            if (found->node != nullptr)
            {
                automation->resolve (found->uuid, found->node->getProcessor());
                if (auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (found->node->getProcessor()))
                    newWorkers.push_back (rust);
                found->pendingState.reset();
                setStatus (found->description.name + " loaded");
                rebuildConnectionsLocked();
            }
            else found->loadError = "Could not add instrument to the rack";
        }
        else found->loadError = result.error.isNotEmpty() ? result.error : "Instrument could not be loaded";
        if (found->loadError.isNotEmpty()) setStatus (found->description.name + ": " + found->loadError);
        chainVersion.fetch_add (1);
    }
    for (auto id : restart) queueSlotLoad (id);
    for (auto* worker : newWorkers) applyAudioWorkgroupTo (worker);
   #if JUCE_MAC && JUCE_AUDIOWORKGROUP_TYPES_AVAILABLE
    for (;;)
    {
        const auto revision = workgroupRevision.load (std::memory_order_acquire);
        auto* next = pendingAudioWorkgroup.exchange (nullptr);
        if (next == nullptr) break;
        if (next == reinterpret_cast<void*> (static_cast<uintptr_t> (1))) next = nullptr;
        if (currentAudioWorkgroup != nullptr) os_release (currentAudioWorkgroup);
        currentAudioWorkgroup = next;
        ++audioWorkgroupGeneration;
        for (auto& slot : chain)
            if (slot.node != nullptr) applyAudioWorkgroupTo (slot.node->getProcessor());
        if (workgroupRevision.load (std::memory_order_acquire) == revision) break;
    }
   #endif
    const auto snapshot = scanner->snapshot();
    if (snapshot.status != lastScannerStatus)
    {
        lastScannerStatus = snapshot.status;
        if (! isLoading()) setStatus (snapshot.status);
    }
    if (scanCatalogueVersion != snapshot.catalogueVersion)
    {
        scanCatalogueVersion = snapshot.catalogueVersion;
        plugins = snapshot.plugins;
        saveCache();
        pluginsVersion.fetch_add (1, std::memory_order_relaxed);
    }
}

juce::File CadenceAudioProcessor::cacheFile() const
{
    return settingsDirectory.getChildFile ("plugins.xml");
}

void CadenceAudioProcessor::loadCache()
{
    const auto file = cacheFile();
    if (! file.existsAsFile())
        return;

    auto xml = juce::XmlDocument::parse (file);
    if (xml == nullptr)
        return;

    juce::KnownPluginList known;
    known.recreateFromXml (*xml);

    plugins.clear();
    for (const auto& description : known.getTypes())
        plugins.push_back (description);

    setStatus (juce::String (plugins.size()) + " plugins (cached)");
    pluginsVersion.fetch_add (1, std::memory_order_relaxed);
}

void CadenceAudioProcessor::saveCache()
{
    juce::KnownPluginList known;
    for (const auto& description : plugins)
        known.addType (description);

    if (auto xml = known.createXml())
    {
        const auto file = cacheFile();
        file.getParentDirectory().createDirectory();
        juce::TemporaryFile temporary (file);
        if (xml->writeTo (temporary.getFile()))
            temporary.overwriteTargetFileWithTemporary();
    }
}

bool CadenceAudioProcessor::hasPlugin (const juce::PluginDescription& description) const
{
    const auto id = description.createIdentifierString();
    return std::any_of (plugins.begin(), plugins.end(), [&id] (const auto& existing)
    {
        return existing.createIdentifierString() == id;
    });
}

bool CadenceAudioProcessor::addPlugin (const juce::PluginDescription& description, juce::String& error)
{
    const auto sampleRate = getSampleRate() > 0.0 ? getSampleRate() : 44100.0;
    const auto blockSize = getBlockSize() > 0 ? getBlockSize() : 512;

    std::unique_ptr<juce::AudioProcessor> instance;
    if (cadence::RustPluginProcessor::supports (description.pluginFormatName))
        instance = cadence::RustPluginProcessor::open (description, error);
    else
        instance = formats.createPluginInstance (description, sampleRate, blockSize, error);
    if (instance == nullptr)
        return false;

    instance->setNonRealtime (isNonRealtime());
    instance->enableAllBuses();

    juce::AudioProcessor* added = nullptr;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        auto node = graph.addNode (std::move (instance));
        if (node == nullptr)
        {
            error = "Could not add plugin to the graph";
            return false;
        }

        node->getProcessor()->prepareToPlay (sampleRate, blockSize);
        Slot slot;
        slot.node = node;
        slot.description = description;
        chain.push_back (std::move (slot));
        rebuildConnectionsLocked();
        chainVersion.fetch_add (1, std::memory_order_relaxed);
        added = chain.back().node->getProcessor();
    }
    applyAudioWorkgroupTo (added);
    return true;
}

void CadenceAudioProcessor::removeSlot (int index)
{
    if (auto* editor = dynamic_cast<CadenceAudioProcessorEditor*> (getActiveEditor())) editor->clearSlotEditor();
    const juce::SpinLock::ScopedLockType lock (graphLock);
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())))
        return;

    auto& removing = chain[static_cast<size_t> (index)];
    automation->removeSlot (removing.uuid);
    if (removing.cancelled != nullptr) removing.cancelled->store (true);
    if (removing.loading) loadingCount.fetch_sub (1);
    if (chain[static_cast<size_t> (index)].node != nullptr)
        graph.removeNode (chain[static_cast<size_t> (index)].node->nodeID);

    chain.erase (chain.begin() + index);
    rebuildConnectionsLocked();
    chainVersion.fetch_add (1, std::memory_order_relaxed);
}

void CadenceAudioProcessor::moveSlot (int index, int delta)
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    const auto target = index + delta;
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))
        || ! juce::isPositiveAndBelow (target, static_cast<int> (chain.size())))
        return;

    std::swap (chain[static_cast<size_t> (index)], chain[static_cast<size_t> (target)]);
    rebuildConnectionsLocked();
    chainVersion.fetch_add (1, std::memory_order_relaxed);
}

void CadenceAudioProcessor::setSlotBypassed (int index, bool bypassed)
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())))
        return;

    auto& slot = chain[static_cast<size_t> (index)];
    slot.bypassed = bypassed;
    if (bypassed) automation->suspend (slot.uuid);
    else if (slot.node != nullptr) automation->resolve (slot.uuid, slot.node->getProcessor());
    rebuildConnectionsLocked();
    chainVersion.fetch_add (1, std::memory_order_relaxed);
}

bool CadenceAudioProcessor::isSlotBypassed (int index) const
{
    return juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))
        && chain[static_cast<size_t> (index)].bypassed;
}

int CadenceAudioProcessor::getNumSlots() const
{
    return static_cast<int> (chain.size());
}

juce::String CadenceAudioProcessor::getSlotName (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())))
        return {};

    return chain[static_cast<size_t> (index)].description.name;
}

juce::String CadenceAudioProcessor::getSlotUuid (int index) const
{
    return juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))
               ? chain[static_cast<size_t> (index)].uuid : juce::String();
}

juce::PluginDescription CadenceAudioProcessor::getSlotDescription (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())))
        return {};
    return chain[static_cast<size_t> (index)].description;
}

juce::AudioProcessor* CadenceAudioProcessor::getSlotProcessor (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size())))
        return nullptr;

    const auto& node = chain[static_cast<size_t> (index)].node;
    return node != nullptr ? node->getProcessor() : nullptr;
}

juce::AudioProcessorEditor* CadenceAudioProcessor::createSlotEditor (int index)
{
    auto* processor = getSlotProcessor (index);
    if (processor == nullptr)
        return nullptr;

    if (auto* editor = processor->createEditorIfNeeded())
        return editor;

    return new juce::GenericAudioProcessorEditor (*processor);
}

float CadenceAudioProcessor::getOutputLevel (int channel) const
{
    return juce::isPositiveAndBelow (channel, 2)
               ? outputLevel[channel].load (std::memory_order_relaxed)
               : 0.0f;
}

void CadenceAudioProcessor::getStateInformation (juce::MemoryBlock& destData)
{
    {
        const juce::ScopedLock lock (restoreMutex);
        if (restorePending.load()) { destData = requestedState; return; }
    }
    const juce::SpinLock::ScopedLockType lock (graphLock);

    juce::XmlElement root ("CADENCE");
    root.setAttribute ("version", 2);
    automation->save (root);

    for (const auto& slot : chain)
    {
        auto* element = root.createNewChildElement ("SLOT");

        if (auto xml = slot.description.createXml())
            element->addChildElement (xml.release());

        element->setAttribute ("uuid", slot.uuid);
        element->setAttribute ("bypassed", slot.bypassed ? 1 : 0);

        if (slot.node != nullptr)
        {
            juce::MemoryBlock state;
            slot.node->getProcessor()->getStateInformation (state);
            element->setAttribute ("state", state.toBase64Encoding());
        }
        else element->setAttribute ("state", slot.pendingState.toBase64Encoding());
    }

    juce::AudioProcessor::copyXmlToBinary (root, destData);
}

void CadenceAudioProcessor::setStateInformation (const void* data, int sizeInBytes)
{
    auto xml = juce::AudioProcessor::getXmlFromBinary (data, sizeInBytes);
    if (xml == nullptr || ! xml->hasTagName ("CADENCE")) return;
    {
        const juce::ScopedLock lock (restoreMutex);
        requestedState.replaceAll (data, static_cast<size_t> (sizeInBytes));
        ++restoreRevision;
        restorePending.store (true);
    }
    automation->invalidate();
    triggerAsyncUpdate();
}

void CadenceAudioProcessor::applyPendingState()
{
    juce::MemoryBlock saved;
    uint64_t revision = 0;
    {
        const juce::ScopedLock lock (restoreMutex);
        if (! restorePending.load()) return;
        saved = requestedState;
        revision = restoreRevision;
    }
    auto xml = juce::AudioProcessor::getXmlFromBinary (saved.getData(), static_cast<int> (saved.getSize()));
    if (auto* editor = dynamic_cast<CadenceAudioProcessorEditor*> (getActiveEditor())) editor->clearSlotEditor();
    std::vector<uint64_t> pending;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        automation->detach();
        for (auto& slot : chain)
        {
            if (slot.cancelled != nullptr) slot.cancelled->store (true);
            if (slot.node != nullptr) graph.removeNode (slot.node->nodeID);
        }
        chain.clear();
        if (xml != nullptr) automation->restore (*xml);
        loadingCount.store (0);
        juce::StringArray seenIds, ambiguousIds;
        if (xml != nullptr)
            for (auto* element : xml->getChildIterator())
            {
                if (! element->hasTagName ("SLOT")) continue;
                auto* descriptionXml = element->getChildElement (0);
                Slot slot;
                if (descriptionXml == nullptr || ! slot.description.loadFromXml (*descriptionXml)) continue;
                const auto savedId = element->getStringAttribute ("uuid");
                if (savedId.isNotEmpty() && ! juce::Uuid (savedId).isNull())
                {
                    slot.uuid = savedId;
                    if (seenIds.contains (savedId)) ambiguousIds.addIfNotAlreadyThere (savedId);
                    seenIds.add (savedId);
                }
                slot.bypassed = element->getBoolAttribute ("bypassed");
                slot.pendingState.fromBase64Encoding (element->getStringAttribute ("state"));
                slot.loadId = nextLoadId++;
                slot.loading = true;
                pending.push_back (slot.loadId);
                chain.push_back (std::move (slot));
                loadingCount.fetch_add (1);
            }
        for (int i = 0; i < cadence::Automation::numControls; ++i)
        {
            const auto assigned = automation->assignment (i).slot;
            if (assigned.isNotEmpty() && (! seenIds.contains (assigned) || ambiguousIds.contains (assigned))) automation->clear (i);
        }
        rebuildConnectionsLocked();
        chainVersion.fetch_add (1);
    }
    for (auto id : pending) queueSlotLoad (id);
    if (pending.empty()) setStatus ("Project restored");
    {
        const juce::ScopedLock lock (restoreMutex);
        if (revision == restoreRevision) restorePending.store (false);
    }
}

void CadenceAudioProcessor::loadPluginAsync (const juce::PluginDescription& description)
{
    jassert (juce::MessageManager::getInstance()->isThisTheMessageThread());
    applyPendingState();
    auto selected = description;
    if (selected.pluginFormatName == "Kontakt")
        for (const auto& known : plugins)
            if (known.pluginFormatName == "Kontakt" && cadence::RustPluginProcessor::kontaktFile (known) == cadence::RustPluginProcessor::kontaktFile (selected))
            { selected = known; break; }
    if (selected.pluginFormatName == "Kontakt" && ! hasPlugin (selected))
    {
        plugins.push_back (selected);
        pluginsVersion.fetch_add (1);
        saveCache();
    }
    uint64_t id;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        Slot slot;
        slot.description = selected;
        slot.loadId = id = nextLoadId++;
        slot.loading = true;
        chain.push_back (std::move (slot));
        loadingCount.fetch_add (1);
        chainVersion.fetch_add (1);
    }
    queueSlotLoad (id);
}

void CadenceAudioProcessor::reloadChangedSlots()
{
    std::vector<int> indexes;
    for (int index = 0; index < getNumSlots(); ++index)
    {
        if (auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (getSlotProcessor (index)))
            if (rust->needsReload()) indexes.push_back (index);

    }
    for (auto index : indexes) reloadSlot (index);
}

void CadenceAudioProcessor::reloadSlot (int index)
{
    uint64_t id = 0;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))) return;
        auto& slot = chain[static_cast<size_t> (index)];
        if (slot.loading) return;
        auto* processor = slot.node != nullptr ? slot.node->getProcessor() : nullptr;
        juce::MemoryBlock state;
        if (processor != nullptr) processor->getStateInformation (state);
        if (processor != nullptr) slot.pendingState = state;
        if (slot.cancelled != nullptr) slot.cancelled->store (true);
        slot.cancelled = std::make_shared<std::atomic<bool>> (false);
        if (slot.node != nullptr) graph.removeNode (slot.node->nodeID);
        slot.node = nullptr;
        slot.loading = true;
        slot.loadId = nextLoadId++;
        loadingCount.fetch_add (1, std::memory_order_relaxed);
        id = slot.loadId;
        automation->suspend (slot.uuid);
        rebuildConnectionsLocked();
        chainVersion.fetch_add (1, std::memory_order_relaxed);
        setStatus ("Reloading " + slot.description.name + "...");
    }
    queueSlotLoad (id);
}

void CadenceAudioProcessor::queueSlotLoad (uint64_t id)
{
    cadence::PluginLoader::Request request;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        const auto found = std::find_if (chain.begin(), chain.end(), [&] (const auto& slot) { return slot.loadId == id; });
        if (found == chain.end() || ! found->loading) return;
        if (found->cancelled != nullptr) found->cancelled->store (true);
        found->cancelled = std::make_shared<std::atomic<bool>> (false);
        request.id = id;
        found->loadError.clear();
        request.description = found->description;
        request.state = found->pendingState;
        request.cancelled = found->cancelled;
        request.settings.rate = getSampleRate() > 0 ? getSampleRate() : 44100.0;
        request.settings.block = getBlockSize() > 0 ? getBlockSize() : 512;
        // setNonRealtime may be called by the host on another thread. Capture
        // its mode and generation together, or retry rather than attach an old mode.
        uint64_t after;
        do
        {
            request.settings.generation = configurationGeneration.load();
            request.settings.offline = loadOffline.load();
            after = configurationGeneration.load();
        } while (request.settings.generation != after);
        setStatus ("Loading " + found->description.name + "...");
    }
    loader->enqueue (std::move (request));
}

int CadenceAudioProcessor::relocateKontaktSlot (const juce::String& uuid, const juce::File& instrument,
                                                const juce::File& sampleRoot)
{
    jassert (juce::MessageManager::getInstance()->isThisTheMessageThread());
    if ((instrument != juce::File() && (! instrument.existsAsFile() || ! instrument.hasFileExtension ("nki;nkm")))
        || (sampleRoot != juce::File() && ! sampleRoot.isDirectory())) return -1;
    int index = -1;
    juce::PluginDescription previous, updated;
    {
        const juce::SpinLock::ScopedLockType lock (graphLock);
        for (size_t i = 0; i < chain.size(); ++i)
            if (chain[i].uuid == uuid && chain[i].description.pluginFormatName == "Kontakt" && ! chain[i].loading)
            {
                previous = chain[i].description;
                updated = cadence::RustPluginProcessor::relocateKontakt (previous, instrument, sampleRoot);
                chain[i].description = updated;
                index = static_cast<int> (i);
                break;
            }
    }
    if (index < 0) return -1;
    if (auto* editor = dynamic_cast<CadenceAudioProcessorEditor*> (getActiveEditor())) editor->clearSlotEditor();
    for (auto& item : plugins)
        if (item.createIdentifierString() == previous.createIdentifierString()) item = updated;
    pluginsVersion.fetch_add (1);
    saveCache();
    reloadSlot (index);
    return index;
}

void CadenceAudioProcessor::cancelLoading()
{
    jassert (juce::MessageManager::getInstance()->isThisTheMessageThread());
    {
        const juce::ScopedLock lock (restoreMutex);
        restorePending.store (false);
        ++restoreRevision;
    }
    if (auto* editor = dynamic_cast<CadenceAudioProcessorEditor*> (getActiveEditor())) editor->clearSlotEditor();
    const juce::SpinLock::ScopedLockType lock (graphLock);
    chain.erase (std::remove_if (chain.begin(), chain.end(), [this] (auto& slot)
    {
        if (! slot.loading) return false;
        automation->removeSlot (slot.uuid);
        if (slot.cancelled != nullptr) slot.cancelled->store (true);
        return true;
    }), chain.end());
    loadingCount.store (0);
    chainVersion.fetch_add (1);
    setStatus ("Loading cancelled");
}

juce::String CadenceAudioProcessor::getSlotLoadStatus (int index) const
{
    if (! juce::isPositiveAndBelow (index, static_cast<int> (chain.size()))) return {};
    const auto& slot = chain[static_cast<size_t> (index)];
    return slot.loading ? "Loading..." : slot.loadError;
}

std::vector<cadence::Automation::Target> CadenceAudioProcessor::getAutomationTargets (int slot) const
{
    return cadence::Automation::targets (getSlotProcessor (slot));
}
bool CadenceAudioProcessor::assignAutomation (int control, int slotIndex, const juce::String& key, juce::String& error)
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    if (! juce::isPositiveAndBelow (slotIndex, getNumSlots())) { error = "Choose a loaded plugin"; return false; }
    const auto targets = getAutomationTargets (slotIndex);
    const auto found = std::find_if (targets.begin(), targets.end(), [&] (const auto& p) { return p.key == key; });
    if (found == targets.end()) { error = "Choose an automatable control"; return false; }
    const auto& slot = chain[static_cast<size_t> (slotIndex)];
    if (! automation->assign (control, slot.uuid, *found, slot.node->getProcessor(), error)) return false;
    chainVersion.fetch_add (1); return true;
}
void CadenceAudioProcessor::clearAutomation (int control)
{
    const juce::SpinLock::ScopedLockType lock (graphLock);
    automation->clear (control); chainVersion.fetch_add (1);
}
juce::String CadenceAudioProcessor::getAutomationAssignment (int control) const
{
    const auto assigned = automation->assignment (control);
    if (assigned.slot.isEmpty()) return "Unassigned";
    for (size_t i = 0; i < chain.size(); ++i) if (chain[i].uuid == assigned.slot)
        return juce::String (i + 1) + ". " + chain[i].description.name + " / " + assigned.name
            + (chain[i].node == nullptr ? " (loading)" : dynamic_cast<cadence::RustPluginProcessor*> (chain[i].node->getProcessor()) == nullptr ? " (block rate)" : "");
    return "Unassigned";
}
bool CadenceAudioProcessor::queueHostAutomation (int control, int frame, float value) noexcept { return automation->queue (control, frame, value); }
bool CadenceAudioProcessor::queueHostAutomationRamp (int control, int frame, int duration, float from, float to) noexcept
{ return automation->queueRamp (control, frame, duration, from, to); }
