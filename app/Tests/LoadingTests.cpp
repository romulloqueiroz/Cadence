#include "BridgeTests.h"
#include "PluginEditor.h"
#include "RustPluginProcessor.h"
#include <csignal>
#include <iostream>
#include <stdexcept>

namespace
{
void check (bool condition, const char* message)
{
    if (! condition) throw std::runtime_error (message);
}
bool until (const std::function<bool()>& predicate)
{
    const auto end = juce::Time::getMillisecondCounterHiRes() + 5000;
    while (! predicate() && juce::Time::getMillisecondCounterHiRes() < end)
        juce::MessageManager::getInstance()->runDispatchLoopUntil (10);
    return predicate();
}
juce::PluginDescription describe (const juce::String& source, const juce::String& format)
{
    juce::String error;
    auto d = cadence::RustPluginProcessor::describe (source, format, error);
    check (error.isEmpty() && d.name.isNotEmpty(),
           ("Describe loading fixture: " + error).toRawUTF8());
    return d;
}
}

void runLoadingTests()
{
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence async loading", "", false);
    check (directory.createDirectory().wasOk(), "Create loading workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    const auto fixture = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    auto description = describe (fixture.getFullPathName(), "CLAP");
    const auto slow = directory.getChildFile ("slow-load instrument.clap");
    const auto hang = directory.getChildFile ("hang-load instrument.clap");
    check (fixture.copyFileTo (slow) && fixture.copyFileTo (hang), "Copy real slow/hung startup fixtures");
    description.fileOrIdentifier = slow.getFullPathName();
    description.name = "Slow instrument";
    const auto sine = describe ("builtin:sine", "Protocol");
    CadenceAudioProcessor processor ({ cadence::RustPluginProcessor::workerFile(), directory });
    processor.setNonRealtime (true);
    processor.prepareToPlay (48000, 64);
    juce::String error;
    check (processor.addPlugin (sine, error), "Create existing playable rack");
    const auto originalPid = dynamic_cast<cadence::RustPluginProcessor*> (processor.getSlotProcessor (0))->workerId();
    std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
    const auto started = juce::Time::getMillisecondCounterHiRes();
    processor.loadPluginAsync (description);
    check (juce::Time::getMillisecondCounterHiRes() - started < 50, "Loading returns without waiting for plugin initialization");
    check (processor.isLoading() && processor.getNumSlots() == 2 && processor.getSlotProcessor (1) == nullptr,
           "Unfinished instrument is a pending slot, not an active graph node");
    juce::AudioBuffer<float> audio (2, 64);
    juce::MidiBuffer midi;
    int heartbeats = 0;
    while (processor.isLoading() && heartbeats < 500)
    {
        midi.clear();
        if (heartbeats == 0) midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 0);
        processor.processBlock (audio, midi);
        if (heartbeats > 0)
            check (audio.getMagnitude (0, 64) > 0.001f, "Existing instrument keeps playing during background loading");
        juce::MessageManager::getInstance()->runDispatchLoopUntil (10);
        ++heartbeats;
    }
    check (heartbeats >= 5 && ! processor.isLoading() && processor.getSlotProcessor (1) != nullptr,
           "UI dispatch remains live during slow startup and commits the finished instrument");
    processor.removeSlot (1);

    // Cancelling a genuinely hung child must not leave a thread or process alive.
    description.fileOrIdentifier = hang.getFullPathName();
    processor.loadPluginAsync (description);
    const auto marker = juce::File (hang.getFullPathName() + ".loading");
    check (until ([&] { return marker.existsAsFile(); }), "Hung worker entered plugin initialization");
    check (until ([&]
    {
        for (auto* child : editor->getChildren())
            if (auto* button = dynamic_cast<juce::TextButton*> (child); button != nullptr && button->getButtonText() == "Cancel loading")
                return button->isVisible() && button->isEnabled();
        return false;
    }), "Editor exposes cancellation while a worker is loading");
    const auto image = editor->createComponentSnapshot (editor->getLocalBounds());
    const auto preview = cadenceTestTempDirectory().getChildFile ("cadence-async-loading-preview.png");
    preview.deleteFile();
    if (auto output = preview.createOutputStream()) juce::PNGImageFormat().writeImageToStream (image, *output);
    const auto hungPid = marker.loadFileAsString().getIntValue();
    const auto cancelling = juce::Time::getMillisecondCounterHiRes();
    processor.cancelLoading();
    check (juce::Time::getMillisecondCounterHiRes() - cancelling < 50, "Cancel returns promptly");
    check (! processor.isLoading() && processor.getNumSlots() == 1, "Cancel preserves the existing rack");
    check (until ([&] { return ::kill (hungPid, 0) != 0; }), "Cancelled loading worker is killed and reaped");
    check (dynamic_cast<cadence::RustPluginProcessor*> (processor.getSlotProcessor (0))->workerId() == originalPid,
           "Cancellation does not replace the live worker");

    // Audio reconfiguration while loading must not attach an old-rate processor.
    description.fileOrIdentifier = slow.getFullPathName();
    processor.loadPluginAsync (description);
    processor.prepareToPlay (44100, 128);
    check (cadenceWaitForLoading (processor), "Load retries with changed audio settings");
    auto* ready = processor.getSlotProcessor (1);
    check (ready != nullptr && juce::approximatelyEqual (ready->getSampleRate(), 44100.0)
           && ready->getBlockSize() == 128, "Committed instrument matches current audio configuration");
    processor.removeSlot (1);

    // Saving before completion keeps pending descriptions/state/order. Closing
    // the editor has no effect on the processor-owned loading job.
    processor.loadPluginAsync (description);
    editor.reset();
    juce::MemoryBlock saved;
    processor.getStateInformation (saved);
    processor.cancelLoading();
    CadenceAudioProcessor restored ({ cadence::RustPluginProcessor::workerFile(), directory });
    restored.setNonRealtime (true);
    restored.prepareToPlay (44100, 128);
    const auto recallStart = juce::Time::getMillisecondCounterHiRes();
    restored.setStateInformation (saved.getData(), static_cast<int> (saved.getSize()));
    check (juce::Time::getMillisecondCounterHiRes() - recallStart < 50, "Project recall returns before loading");
    juce::MemoryBlock savedAgain;
    restored.getStateInformation (savedAgain);
    check (saved == savedAgain, "Saving before queued recall commits preserves the requested project");
    check (cadenceWaitForLoading (restored) && restored.getNumSlots() == 2 && restored.getSlotProcessor (1) != nullptr,
           "Pending instrument restores in its saved slot without an editor");

    // Removing/replacing the rack invalidates late completions.
    description.fileOrIdentifier = hang.getFullPathName();
    processor.loadPluginAsync (description);
    processor.removeSlot (1);
    processor.loadPluginAsync (description);
    juce::XmlElement empty ("CADENCE"); juce::MemoryBlock emptyState;
    juce::AudioProcessor::copyXmlToBinary (empty, emptyState);
    processor.setStateInformation (emptyState.getData(), static_cast<int> (emptyState.getSize()));
    check (cadenceWaitForLoading (processor) && processor.getNumSlots() == 0,
           "Late completion cannot repopulate a replaced project");

    description.fileOrIdentifier = directory.getChildFile ("missing.clap").getFullPathName();
    processor.loadPluginAsync (description);
    check (cadenceWaitForLoading (processor) && processor.getSlotProcessor (0) == nullptr
           && processor.getSlotLoadStatus (0).isNotEmpty(), "Load failure remains visible and saveable");
    processor.removeSlot (0);

    const auto destroying = juce::Time::getMillisecondCounterHiRes();
    {
        CadenceAudioProcessor doomed ({ cadence::RustPluginProcessor::workerFile(), directory });
        description.fileOrIdentifier = hang.getFullPathName();
        doomed.loadPluginAsync (description);
        juce::MessageManager::getInstance()->runDispatchLoopUntil (20);
    }
    check (juce::Time::getMillisecondCounterHiRes() - destroying < 500,
           "Processor destruction cancels loading instead of waiting for startup timeout");

    // Exercise JUCE's asynchronous native factory and callbacks outliving a rack.
    const auto build = juce::File::getSpecialLocation (juce::File::currentExecutableFile).getParentDirectory().getParentDirectory().getParentDirectory();
    const auto config = juce::File::getSpecialLocation (juce::File::currentExecutableFile).getParentDirectory().getFileName();
    const auto vst = build.getChildFile ("Cadence_artefacts").getChildFile (config).getChildFile ("VST3/Cadence.vst3");
    juce::VST3PluginFormat nativeFormat;
    juce::OwnedArray<juce::PluginDescription> nativeDescriptions;
    nativeFormat.findAllTypesForFile (nativeDescriptions, vst.getFullPathName());
    check (! nativeDescriptions.isEmpty(), "Find built native VST3 fixture");
    CadenceAudioProcessor native ({ cadence::RustPluginProcessor::workerFile(), directory });
    native.setNonRealtime (true);
    native.prepareToPlay (48000, 128);
    native.loadPluginAsync (*nativeDescriptions[0]);
    check (cadenceWaitForLoading (native) && native.getSlotProcessor (0) != nullptr, "Native VST3 asynchronous factory completes");
    native.removeSlot (0);
    native.loadPluginAsync (*nativeDescriptions[0]);
    native.removeSlot (0);
    {
        CadenceAudioProcessor closed ({ cadence::RustPluginProcessor::workerFile(), directory });
        closed.loadPluginAsync (*nativeDescriptions[0]);
    }
    juce::MessageManager::getInstance()->runDispatchLoopUntil (100);
    check (native.getNumSlots() == 0, "Cancelled native callback cannot restore a removed slot");
    std::cout << "PASS asynchronous loading: responsive UI, live audio, cancellation/reaping, configuration changes, pending-state recall, stale results, failure and destruction\n";
}
