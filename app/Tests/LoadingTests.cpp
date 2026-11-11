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


namespace
{
void legacyRackRecall (const juce::File& directory)
{
    const auto fixture = juce::File (__FILE__).getSiblingFile ("fixtures/legacy-cadence-chain.xml");
    auto xml = juce::parseXML (fixture);
    check (xml != nullptr && xml->hasTagName ("CADENCE"), "Read independently retained legacy rack state");
    juce::MemoryBlock state; juce::AudioProcessor::copyXmlToBinary (*xml, state);
    for (bool prepareFirst : { false, true })
    {
        CadenceAudioProcessor rack ({ cadence::RustPluginProcessor::workerFile(), directory });
        rack.setNonRealtime (true);
        if (prepareFirst) rack.prepareToPlay (48000, 256);
        rack.setStateInformation (state.getData(), static_cast<int> (state.getSize()));
        check (until ([&] { return rack.getNumSlots() == 2 && ! rack.isLoading(); }), "Legacy rack restores both slots");
        if (! prepareFirst) rack.prepareToPlay (48000, 256);
        for (int slot = 0; slot < 2; ++slot)
        {
            auto* plugin = dynamic_cast<cadence::RustPluginProcessor*> (rack.getSlotProcessor (slot));
            check (plugin != nullptr && ! plugin->hasFailed(), "Legacy state produces healthy loaded workers");
            std::cout << "LEGACY slot " << slot << " control " << plugin->getParameters()[0]->getValue() << '\n';
        }
        juce::AudioBuffer<float> buffer (2, 256); juce::MidiBuffer midi;
        float peak = 0;
        for (int block = 0; block < 300; ++block)
        {
            buffer.clear(); midi.clear();
            if (block == 0) midi.addEvent (juce::MidiMessage::noteOn (1, 60, static_cast<juce::uint8> (100)), 0);
            if (block == 200) midi.addEvent (juce::MidiMessage::noteOff (1, 60), 0);
            rack.processBlock (buffer, midi); peak = juce::jmax (peak, buffer.getMagnitude (0, 256));
        }
        std::cout << "LEGACY prepareFirst=" << prepareFirst << " peak=" << peak << '\n';
        check (peak > 0.01f, "Legacy saved Sine/Gain rack produces audible output");
    }
}

void kontaktRecovery (const juce::File& directory)
{
    const auto instrument = directory.getChildFile ("Recovery instrument.nki");
    const auto root = directory.getChildFile ("Moved library");
    const auto sample = root.getChildFile ("Samples/tone.wav");
    check (sample.getParentDirectory().createDirectory().wasOk(), "Create relocated sample folder");
    juce::WavAudioFormat wav;
    auto stream = sample.createOutputStream();
    std::unique_ptr<juce::AudioFormatWriter> writer (wav.createWriterFor (stream.release(), 48000, 2, 16, {}, 0));
    check (writer != nullptr, "Create recovery sample");
    juce::AudioBuffer<float> tone (2, 16384);
    for (int i = 0; i < tone.getNumSamples(); ++i)
        for (int ch = 0; ch < 2; ++ch) tone.setSample (ch, i, 0.25f * std::sin (static_cast<float> (i) * 0.1f));
    check (writer->writeFromAudioSampleBuffer (tone, 0, tone.getNumSamples()), "Write recovery sample");
    writer.reset();
    const juce::String xml = R"(<K2_Container><Programs><K2_Program name="Recovery"><Groups><K2_Group index="0" name="Normal"/></Groups><Zones><K2_Zone groupIdx="0"><Parameters><V name="rootKey" value="60"/></Parameters><Sample><V name="file" value="/former/library/Samples/tone.wav"/></Sample></K2_Zone></Zones></K2_Program></Programs></K2_Container>)";
    juce::MemoryOutputStream compressed;
    { juce::GZIPCompressorOutputStream zlib (compressed, 6, 15); zlib.writeText (xml, false, false, nullptr); zlib.flush(); }
    juce::MemoryBlock file (0xaa, true);
    const auto magic = juce::ByteOrder::swapIfBigEndian (static_cast<uint32_t> (0x7fa89012));
    std::memcpy (file.getData(), &magic, 4);
    file.append (compressed.getData(), compressed.getDataSize());
    check (instrument.replaceWithData (file.getData(), file.getSize()), "Write independent XML instrument");
    CadenceAudioProcessor rack ({ cadence::RustPluginProcessor::workerFile(), directory });
    rack.setNonRealtime (true); rack.prepareToPlay (48000, 64);
    rack.loadPluginAsync (cadence::RustPluginProcessor::kontaktDescription (instrument));
    check (cadenceWaitForLoading (rack) && rack.getSlotProcessor (0) == nullptr, "Missing sample leaves a recoverable failed slot");
    check (rack.getSlotLoadStatus (0).contains ("Locate samples"), "Missing sample gives a recovery action");
    const auto uuid = rack.getSlotUuid (0);
    rack.setSlotBypassed (0, true);
    check (rack.relocateKontaktSlot (uuid, {}, root) == 0 && cadenceWaitForLoading (rack), "User-selected sample root retries the failed load");
    auto* loaded = dynamic_cast<cadence::RustPluginProcessor*> (rack.getSlotProcessor (0));
    check (loaded != nullptr && ! loaded->hasFailed(), "Relocated instrument worker is healthy");
    check (rack.getSlotUuid (0) == uuid && rack.isSlotBypassed (0), "Recovery preserves slot identity and bypass");
    check (loaded->compatibilityNotices().contains ("Kontakt playback subset"), "Compatibility notice crosses worker metadata");
    rack.loadPluginAsync (cadence::RustPluginProcessor::kontaktDescription (instrument));
    check (cadenceWaitForLoading (rack) && rack.getSlotProcessor (1) != nullptr, "Opening a known instrument reuses its saved sample locations");
    rack.removeSlot (1);
    {
        std::unique_ptr<juce::AudioProcessorEditor> panel (loaded->createEditor());
        bool visible = false;
        for (auto* child : panel->getChildren())
            if (auto* notice = dynamic_cast<juce::TextEditor*> (child))
                visible |= notice->getName() == "Instrument compatibility" && notice->isVisible() && notice->getText().contains ("scripts");
        check (visible, "Inspector exposes a readable compatibility notice");
        const auto image = panel->createComponentSnapshot (panel->getLocalBounds());
        if (auto output = cadenceTestTempDirectory().getChildFile ("cadence-kontakt-compatibility.png").createOutputStream())
            juce::PNGImageFormat().writeImageToStream (image, *output);
    }
    loaded->getParameters()[0]->setValueNotifyingHost (0.73f);
    juce::AudioBuffer<float> output (2, 64); juce::MidiBuffer notes;
    notes.addEvent (juce::MidiMessage::noteOn (1, 60, static_cast<juce::uint8> (100)), 0);
    float peak = 0;
    for (int i = 0; i < 8; ++i) { loaded->processBlock (output, notes); notes.clear(); peak = juce::jmax (peak, output.getMagnitude (0, 64)); }
    check (peak > 0.0001f, "Recovered samples produce audible output");
    juce::MemoryBlock saved; rack.getStateInformation (saved);
    const auto moved = directory.getChildFile ("Renamed instrument.nki");
    check (instrument.moveFileTo (moved), "Move instrument after saving project");
    CadenceAudioProcessor restored ({ cadence::RustPluginProcessor::workerFile(), directory });
    restored.setNonRealtime (true); restored.prepareToPlay (48000, 64);
    restored.setStateInformation (saved.getData(), static_cast<int> (saved.getSize()));
    check (until ([&] { return restored.getNumSlots() == 1 && ! restored.isLoading(); }) && restored.getSlotProcessor (0) == nullptr, "Moved instrument retains failed saved slot");
    check (restored.relocateKontaktSlot (uuid, moved, {}) == 0 && cadenceWaitForLoading (restored), "Locate instrument retries with saved sample roots");
    auto* recalled = dynamic_cast<cadence::RustPluginProcessor*> (restored.getSlotProcessor (0));
    check (recalled != nullptr && ! recalled->hasFailed(), "Moved instrument recalls successfully");
    check (std::abs (recalled->getParameters()[0]->getValue() - 0.73f) < 0.0001f && restored.isSlotBypassed (0), "Recovery restores saved parameter state and bypass");
    std::cout << "PASS Kontakt recovery: missing samples, scoped search, instrument move, notices, audible playback and saved-state recall\n";
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
    legacyRackRecall (directory);
    kontaktRecovery (directory);
    std::cout << "PASS asynchronous loading: responsive UI, live audio, cancellation/reaping, configuration changes, pending-state recall, stale results, failure and destruction\n";
}
