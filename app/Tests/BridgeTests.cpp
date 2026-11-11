#include <CoreFoundation/CoreFoundation.h>
#include <AudioToolbox/AudioToolbox.h>
#include <CoreMIDI/CoreMIDI.h>
#include "BridgeTests.h"
#include "PluginProcessor.h"
#include "RustPluginProcessor.h"
#include <csignal>
#include <iostream>
#include <stdexcept>
#include <unistd.h>
#include <sys/wait.h>

namespace
{
void check (bool condition, const juce::String& message)
{
    if (! condition) throw std::runtime_error (message.toStdString());
}

juce::PluginDescription describe (const juce::String& source, const juce::String& format)
{
    juce::String error;
    auto d = cadence::RustPluginProcessor::describe (source, format, error);
    check (error.isEmpty() && d.name.isNotEmpty(), "Describe: " + error);
    return d;
}

std::unique_ptr<cadence::RustPluginProcessor> open (const juce::PluginDescription& d)
{
    juce::String error;
    auto p = cadence::RustPluginProcessor::open (d, error);
    check (p != nullptr, "Open: " + error);
    p->setNonRealtime (true);
    p->prepareToPlay (48000.0, 256);
    check (! p->hasFailed(), "Prepare succeeds");
    return p;
}

// Feed the event block, then advance by the reported delay. All functional
// comparisons run offline so wall-clock worker scheduling cannot affect them.
void alignedBlock (juce::AudioProcessor& processor, juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi)
{
    processor.processBlock (buffer, midi);
    const auto blocks = (processor.getLatencySamples() + buffer.getNumSamples() - 1) / buffer.getNumSamples();
    for (int i = 0; i < blocks; ++i) { midi.clear(); processor.processBlock (buffer, midi); }
}

void clapNotesParametersAndState()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    juce::String enumerationError;
    auto variants = cadence::RustPluginProcessor::describeAll (path.getFullPathName(), "CLAP", enumerationError);
    check (enumerationError.isEmpty() && variants.size() == 3, "CLAP module enumerates every selectable plugin");
    check (variants[0].name == "Bridge Test Tone" && variants[1].name == "Bridge Test Tone 2"
           && variants[2].name == "Bridge Test Sidechain",
           "CLAP variants keep stable names and ids");
    auto second = open (variants[1]);
    check (second->getName() == "Bridge Test Tone 2", "Selecting the second CLAP variant loads that descriptor");
    auto description = describe (path.getFullPathName(), "CLAP");
    auto p = open (description);
    check (p->getParameters().size() == 1, "CLAP parameter metadata is exposed");
    CadenceStreamingStats notStreaming {};
    check (! p->streamingStats (notStreaming), "CLAP does not report synthetic streaming diagnostics");
    check (p->getParameters()[0]->getName (64) == "Gain", "CLAP parameter name");
    check (dynamic_cast<juce::AudioProcessorParameterWithID*> (p->getParameters()[0])->paramID == "65537",
           "CLAP serialized parameter identity preserves the original ID");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (3, 60, uint8_t (100)), 23);
    midi.addEvent (juce::MidiMessage::noteOff (3, 60), 119);
    alignedBlock (*p, buffer, midi);
    check (! p->hasFailed(), "CLAP audio exchange succeeds");
    for (int frame = 0; frame < 256; ++frame)
        check (std::abs (buffer.getSample (0, frame) - (frame >= 23 && frame < 119 ? 0.5f : 0.0f)) < 0.0001f,
               "Sample-accurate CLAP note on/off and default parameter");

    auto* gain = p->getParameters()[0];
    gain->setValueNotifyingHost (0.25f);
    midi.addEvent (juce::MidiMessage::noteOn (3, 60, uint8_t (100)), 0);
    alignedBlock (*p, buffer, midi);
    check (std::abs (buffer.getSample (0, 32) - 0.25f) < 0.0001f,
           "CLAP dense parameter ID maps to original ID above 65535");
    juce::MemoryBlock state;
    p->getStateInformation (state);
    check (state.getSize() > 0 && ! p->hasStateError(), "Capture CLAP state");
    gain->setValueNotifyingHost (0.75f);
    alignedBlock (*p, buffer, midi);
    check (std::abs (buffer.getSample (0, 32) - 0.75f) < 0.0001f, "Automate CLAP parameter");
    p->setStateInformation (state.getData(), static_cast<int> (state.getSize()));
    alignedBlock (*p, buffer, midi);
    check (std::abs (buffer.getSample (0, 32) - 0.25f) < 0.0001f && std::abs (gain->getValue() - 0.25f) < 0.0001f,
           "Restore CLAP opaque state and Inspector value");
    p->prepareToPlay (44100, 256);
    alignedBlock (*p, buffer, midi);
    check (! p->hasFailed(), "CLAP sample-rate change reactivates correctly");
    std::unique_ptr<juce::AudioProcessorEditor> editor (p->createEditor());
    editor->setSize (400, 220);
    const auto image = editor->createComponentSnapshot (editor->getLocalBounds());
    juce::File ("/tmp/cadence-rust-inspector.png").deleteFile();
    if (auto output = juce::File ("/tmp/cadence-rust-inspector.png").createOutputStream())
        juce::PNGImageFormat().writeImageToStream (image, *output);
    editor.reset();
    const int pid = static_cast<int> (p->workerId());
    ::kill (pid, SIGKILL);
    p->setNonRealtime (false);
    const auto start = juce::Time::getMillisecondCounterHiRes();
    p->processBlock (buffer, midi);
    const auto failureDeadline = juce::Time::getMillisecondCounterHiRes() + 250;
    while (! p->hasFailed() && juce::Time::getMillisecondCounterHiRes() < failureDeadline) juce::Thread::sleep (1);
    p->processBlock (buffer, midi);
    check (p->hasFailed() && buffer.getMagnitude (0, 256) == 0.0f, "Worker crash produces silence");
    check (p->failureReason() != "Worker stopped", "Rust failure reason reaches the inspector bridge");
    check (juce::Time::getMillisecondCounterHiRes() - start < 250, "Worker crash wait is bounded");
    p.reset();
    check (::kill (pid, 0) != 0, "Worker crash is reaped");
    auto hung = open (description);
    const auto hungPid = static_cast<pid_t> (hung->workerId());
    hung->setNonRealtime (false);
    hung->getParameters()[0]->setValueNotifyingHost (0.999f); // Test fixture deliberately spins.
    const auto callbackStart = juce::Time::getMillisecondCounterHiRes();
    hung->processBlock (buffer, midi);
    check (juce::Time::getMillisecondCounterHiRes() - callbackStart < 10.0, "Hung worker does not hold the audio callback");
    check (buffer.getMagnitude (0, 256) == 0.0f, "Hung worker is silenced");
    siginfo_t info {};
    const auto deadline = juce::Time::getMillisecondCounterHiRes() + 250;
    do
    {
        ::waitid (P_PID, static_cast<id_t> (hungPid), &info, WEXITED | WNOHANG | WNOWAIT);
        if (info.si_pid == hungPid) break;
        juce::Thread::sleep (1);
    } while (juce::Time::getMillisecondCounterHiRes() < deadline);
    check (info.si_pid == hungPid, "Watchdog kills hung worker without waiting for plugin removal");
    hung.reset();
    check (::kill (hungPid, 0) != 0, "Hung worker is reaped on removal");
    std::cout << "PASS CLAP timed notes, parameter-ID mapping, state, sample-rate change, editor, crash/hang isolation\n";
}

juce::MemoryBlock protocolChain()
{
    const auto worker = cadence::RustPluginProcessor::workerFile();
    const auto sine = describe ("builtin:sine", "Protocol");
    const auto gain = describe ("builtin:gain", "Protocol");
    check (describe (worker.getSiblingFile ("gain.dylib").getFullPathName(), "Protocol").name == gain.name,
           "External native protocol library and portable bundled source both load");
    const auto directory = cadenceTestTempDirectory()
        .getNonexistentChildFile ("Cadence bridge chain tests", "", false);
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    directory.createDirectory();
    juce::KnownPluginList library;
    library.addType (sine); library.addType (gain);
    library.createXml()->writeTo (directory.getChildFile ("plugins.xml"));
    CadenceAudioProcessor processor ({ worker, directory });
    processor.setNonRealtime (true);
    processor.prepareToPlay (48000, 256);
    juce::String error;
    check (processor.addPlugin (sine, error) && processor.addPlugin (gain, error), "Add protocol instrument and effect: " + error);
    check (processor.getSlotProcessor (1)->getParameters().size() == 1, "Native SDK parameter extension reaches JUCE");
    processor.getSlotProcessor (1)->getParameters()[0]->setValueNotifyingHost (48.0f / 72.0f); // -12 dB
    juce::MessageManager::getInstance()->runDispatchLoopUntil (40);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 37);
    alignedBlock (processor, buffer, midi);
    check (buffer.getMagnitude (0, 37) == 0.0f, "Protocol MIDI offset is preserved through graph");
    check (buffer.getMagnitude (37, 219) > 0.01f, "Protocol instrument renders through native gain effect");
    juce::MemoryBlock state;
    processor.getStateInformation (state);
    check (state.getSize() > 0, "Save bridged chain");
    processor.setStateInformation (state.getData(), static_cast<int> (state.getSize()));
    check (cadenceWaitForLoading (processor), "Asynchronous chain recall completes");
    check (processor.getNumSlots() == 2, "Restore protocol chain");
    check (std::abs (processor.getSlotProcessor (1)->getParameters()[0]->getValue() - 48.0f / 72.0f) < 0.0001f,
           "Restore native gain parameter and plugin state");
    midi.clear(); midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 0);
    alignedBlock (processor, buffer, midi);
    check (buffer.getMagnitude (0, 256) > 0.01f, "Restored chain plays");
    processor.setSlotBypassed (1, true);
    processor.moveSlot (1, -1);
    processor.moveSlot (0, 1);
    processor.setSlotBypassed (1, false);
    std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
    for (auto* child : editor->getChildren())
        if (auto* list = dynamic_cast<juce::ListBox*> (child); list != nullptr && list->getName() == "Chain") list->selectRow (1);
    const auto image = editor->createComponentSnapshot (editor->getLocalBounds());
    juce::File ("/tmp/cadence-bridge-preview.png").deleteFile();
    if (auto output = juce::File ("/tmp/cadence-bridge-preview.png").createOutputStream())
        juce::PNGImageFormat().writeImageToStream (image, *output);
    processor.removeSlot (1);
    check (processor.getNumSlots() == 1, "Remove plugin with open sandbox inspector");
    editor.reset();
    std::cout << "PASS native protocol parameters, graph audio/MIDI, chain state, bypass/reorder, inspector removal\n";
    return state;
}

void kontaktIfInstalled()
{
    const juce::File piano ("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki");
    if (! piano.existsAsFile()) { std::cout << "SKIP installed Kontakt Piano Uno test (content unavailable)\n"; return; }
    auto p = open (describe (piano.getFullPathName(), "Kontakt"));
    check (p->getParameters().size() == 1, "Kontakt gain control is exposed");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 60, uint8_t (100)), 17);
    float peak = 0;
    for (int block = 0; block < 96; ++block)
    {
        alignedBlock (*p, buffer, midi);
        check (! p->hasFailed(), "Kontakt worker remains healthy");
        if (block == 0) check (buffer.getMagnitude (0, 17) == 0.0f, "Kontakt MIDI offset");
        peak = juce::jmax (peak, buffer.getMagnitude (0, 256));
    }
    check (peak > 0.01f, "Kontakt samples produce audible output");
    CadenceStreamingStats stats {};
    check (p->streamingStats (stats), "Kontakt streaming telemetry is available through the C bridge");
    check (stats.resident_bytes > 0 && stats.cache_capacity_bytes == 16u * 1024u * 1024u,
           "Resident and allocated cache memory are distinct");
    check (stats.pages_read > 0 && stats.read_count >= stats.pages_read && stats.max_read_ns > 0,
           "Disk page delivery timing reaches the inspector bridge");
    check (stats.active_voices > 0 && stats.active_voices <= stats.voice_capacity,
           "Active sampler voices are published");
    check (stats.underrun_blocks == 0 && stats.underrun_frames == 0 && p->pipelineUnderruns() == 0,
           "Streaming misses and pipeline dropouts are separate and zero");
    std::unique_ptr<juce::AudioProcessorEditor> inspector (p->createEditor());
    juce::Label* diagnostics = nullptr;
    for (auto* child : inspector->getChildren())
        if (child->getName() == "Streaming diagnostics") diagnostics = dynamic_cast<juce::Label*> (child);
    check (diagnostics != nullptr && diagnostics->isVisible(), "Kontakt inspector displays streaming diagnostics");
    check (diagnostics->getText().contains ("Resident:") && diagnostics->getText().contains ("Page time")
           && diagnostics->getText().contains ("Streaming misses:"), "Inspector includes memory, timing and distinct misses");
    check (inspector->getLocalBounds().contains (diagnostics->getBounds()), "Streaming diagnostics fit inspector bounds");
    const auto snapshot = inspector->createComponentSnapshot (inspector->getLocalBounds(), true);
    const auto preview = juce::File::getCurrentWorkingDirectory().getChildFile ("streaming-inspector-preview.png");
    if (auto stream = preview.createOutputStream())
    {
        stream->setPosition (0); stream->truncate();
        juce::PNGImageFormat png;
        check (png.writeImageToStream (snapshot, *stream), "Streaming inspector snapshot is written");
    }
    else check (false, "Streaming inspector snapshot can be created");
    std::cout << "PASS streaming inspector: memory, voices, cache, timing and separate misses\n";
    std::cout << "PASS sandboxed Kontakt Piano Uno playback, MIDI offsets, controls; peak " << peak << '\n';
}

void scanImports (const juce::String& helper)
{
    if (helper.isEmpty()) return;
    const auto directory = cadenceTestTempDirectory()
        .getNonexistentChildFile ("Cadence real import tests", "", false);
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    cadence::PluginScanner scanner ({ juce::File (helper), directory, 20000 });
    const auto fixture = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    const juce::File piano ("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki");
    std::vector<cadence::ScanCandidate> sources { { "Protocol", "builtin:sine" }, { "Protocol", "builtin:gain" }, { "CLAP", fixture.getFullPathName() } };
    if (piano.existsAsFile()) sources.push_back ({ "Kontakt", piano.getFullPathName() });
    scanner.start (sources, {});
    const auto deadline = juce::Time::getMillisecondCounterHiRes() + 30000;
    while (scanner.isRunning() && juce::Time::getMillisecondCounterHiRes() < deadline) juce::Thread::sleep (10);
    const auto result = scanner.snapshot();
    // The CLAP fixture module deliberately contains three selectable plugins.
    const auto expected = sources.size() + 2;
    check (! result.running && result.plugins.size() == expected && result.skipped.empty(),
           "Packaged scanner imports protocol, CLAP, and Kontakt: " + result.status);
    std::cout << "PASS packaged scanner imports protocol, CLAP and installed Kontakt sources\n";
}

static void nativeAutomationTest (const juce::PluginDescription& description, const juce::String& format)
{
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence native AU automation", "", false);
    check (directory.createDirectory().wasOk(), "Create native AU automation workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    CadenceAudioProcessor outer ({ cadence::RustPluginProcessor::workerFile(), directory });
    outer.setNonRealtime (true);
    outer.prepareToPlay (48000, 256);
    juce::String error;
    check (outer.addPlugin (description, error), "Add native AU target: " + error);
    auto* native = outer.getSlotProcessor (0);
    check (native != nullptr && ! native->getParameters().isEmpty(), "Native AU exposes parameters");
    const auto targets = outer.getAutomationTargets (0);
    check (! targets.empty(), "Native AU exposes automatable targets");
    check (outer.assignAutomation (0, 0, targets[0].key, error), "Assign outer control to native AU target: " + error);
    auto* macro = outer.getParameters()[0];
    macro->setValueNotifyingHost (0.25f);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    outer.processBlock (buffer, midi);
    check (std::abs (native->getParameters()[0]->getValue() - 0.25f) < 0.001f,
           "Native AU receives the baseline host value");
    check (outer.queueHostAutomation (0, 100, 0.75f), "Queue offset native AU automation");
    outer.processBlock (buffer, midi);
    check (std::abs (native->getParameters()[0]->getValue() - 0.75f) < 0.001f,
           "Native target receives scheduled offset automation");
    if (format == "VST3")
    {
        if (auto* target = dynamic_cast<cadence::NativeAutomationTarget*> (native))
            check (target->nativeAutomationEventsApplied() > 0,
                   "VST3 target consumed queued timed automation events");
        else
            check (false, "VST3 target exposes the native automation interface");
    }
    std::cout << "PASS native " << format << " scheduled automation delivery\n";
}

static void clapStructuralReload()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence structural reload", "", false);
    check (directory.createDirectory().wasOk(), "Create structural reload workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    CadenceAudioProcessor processor ({ cadence::RustPluginProcessor::workerFile(), directory });
    processor.setNonRealtime (true);
    processor.prepareToPlay (48000, 256);
    juce::String error;
    check (processor.addPlugin (describe (path.getFullPathName(), "CLAP"), error), "Add reload fixture: " + error);
    auto* before = processor.getSlotProcessor (0);
    check (before != nullptr, "Reload fixture loaded");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 125, static_cast<juce::uint8> (100)), 0);
    processor.processBlock (buffer, midi);
    const auto deadline = juce::Time::getMillisecondCounterHiRes() + 5000;
    while (juce::Time::getMillisecondCounterHiRes() < deadline)
    {
        juce::MessageManager::getInstance()->runDispatchLoopUntil (10);
        auto* current = processor.getSlotProcessor (0);
        if (current != nullptr && current != before && ! processor.isLoading())
        {
            check (! dynamic_cast<cadence::RustPluginProcessor*> (current)->hasFailed(),
                   "Reloaded CLAP slot is healthy");
            std::cout << "PASS CLAP structural parameter/port reload\n";
            return;
        }
    }
    check (false, "CLAP structural reload did not complete");
}

static void clapMidiOutput()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    auto p = open (describe (path.getFullPathName(), "CLAP"));
    p->setNonRealtime (true);
    p->prepareToPlay (48000, 256);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 126, static_cast<juce::uint8> (100)), 0);
    bool sawNote = false, sawSysex = false;
    for (int block = 0; block < 4; ++block)
    {
        p->processBlock (buffer, midi);
        for (const auto metadata : midi)
        {
            const auto message = metadata.getMessage();
            if (message.isNoteOn() && message.getNoteNumber() == 64) sawNote = true;
            if (message.isSysEx()) sawSysex = true;
        }
        midi.clear();
    }
    check (sawNote, "CLAP output MIDI note reaches the host");
    check (sawSysex, "CLAP output SysEx reaches the host");
    std::cout << "PASS CLAP MIDI output and SysEx\n";
}

static void nativeMidiOutputTest (const juce::PluginDescription& description, const juce::String& format)
{
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence native MIDI", "", false);
    check (directory.createDirectory().wasOk(), "Create native MIDI workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    CadenceAudioProcessor outer ({ cadence::RustPluginProcessor::workerFile(), directory });
    outer.setNonRealtime (true);
    outer.prepareToPlay (48000, 256);
    juce::String error;
    check (outer.addPlugin (description, error), "Add native MIDI target: " + error);
    check (outer.producesMidi(), "Outer rack exposes MIDI output");
    check (outer.isSlotMidiOutputRouted (0), "Native " + format + " MIDI output is routed to the host");
    std::cout << "PASS native " << format << " MIDI output routing\n";
}

static void nativeSidechainRoutingTest (const juce::PluginDescription& description, const juce::String& format)
{
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence native sidechain", "", false);
    check (directory.createDirectory().wasOk(), "Create native sidechain workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    CadenceAudioProcessor outer ({ cadence::RustPluginProcessor::workerFile(), directory });
    outer.setNonRealtime (true);
    outer.enableAllBuses();
    outer.prepareToPlay (48000, 256);
    juce::String error;
    check (outer.addPlugin (description, error), "Add native sidechain target: " + error);
    check (outer.getSlotProcessor (0) != nullptr, "Native sidechain target loaded");
    check (outer.isSlotSidechainRouted (0),
           "Native " + format + " sidechain bus is connected to the host sidechain input");
    std::cout << "PASS native " << format << " sidechain routing\n";
}

void nested (const juce::String& path, const juce::String& format, const juce::MemoryBlock& state)
{
    CFBundleRef bundle = nullptr;
    const juce::ScopeGuard releaseBundle { [&] { if (bundle != nullptr) CFRelease (bundle); } };
    if (format == "AudioUnit")
    {
        const auto bytes = path.toUTF8();
        auto url = CFURLCreateFromFileSystemRepresentation (nullptr, reinterpret_cast<const UInt8*> (bytes.getAddress()),
                                                            static_cast<CFIndex> (bytes.sizeInBytes() - 1), true);
        bundle = CFBundleCreate (nullptr, url); CFRelease (url);
        check (bundle != nullptr && CFBundleLoadExecutable (bundle), "Load built AU bundle for process-local registration");
        auto factory = reinterpret_cast<AudioComponentFactoryFunction> (
            CFBundleGetFunctionPointerForName (bundle, CFSTR ("CadenceAUFactory")));
        check (factory != nullptr, "Built AU exports its factory");
        AudioComponentDescription component { kAudioUnitType_MusicDevice, 0x43646e31, 0x4361646e, 0, 0 };
        check (AudioComponentRegister (&component, CFSTR ("Cadence: Cadence"), 256, factory) != nullptr,
               "Register built AU in this test process");
    }
    std::unique_ptr<juce::AudioPluginFormat> loader;
    if (format == "VST3") loader = std::make_unique<juce::VST3PluginFormat>();
    else loader = std::make_unique<juce::AudioUnitPluginFormat>();
    juce::OwnedArray<juce::PluginDescription> descriptions;
    loader->findAllTypesForFile (descriptions, path);
    check (! descriptions.isEmpty(), "Find nested Cadence " + format);
    nativeAutomationTest (*descriptions[0], format);
    nativeSidechainRoutingTest (*descriptions[0], format);
    nativeMidiOutputTest (*descriptions[0], format);
    juce::String error;
    auto instance = loader->createInstanceFromDescription (*descriptions[0], 48000, 256, error);
    check (instance != nullptr, "Load nested Cadence " + format + ": " + error);
    instance->setNonRealtime (true);
    instance->prepareToPlay (48000, 256);
    AudioUnit audioUnit = nullptr;
    if (format == "AudioUnit")
    {
        // JUCE's AU host stores its own offline flag but does not forward the
        // AU property. A real AU host such as Logic explicitly sets it.
        struct UnitVisitor final : juce::ExtensionsVisitor
        {
            AudioUnit unit = nullptr;
            void visitAudioUnitClient (const AudioUnitClient& client) override
            {
                unit = client.getAudioUnitHandle();
            }
        } visitor;
        instance->getExtensions (visitor);
        audioUnit = visitor.unit;
        UInt32 offline = 1;
        check (visitor.unit != nullptr && AudioUnitSetProperty (visitor.unit,
                    kAudioUnitProperty_OfflineRender, kAudioUnitScope_Global, 0, &offline, sizeof (offline)) == noErr,
               "AU host sets offline render property");
    }
    // Host wrappers serialize their own envelopes around the processor state.
    // Supply the format's actual envelope, as a DAW would.
    juce::MemoryBlock envelope;
    if (format == "VST3")
    {
        juce::XmlElement xml ("VST3PluginState");
        xml.createNewChildElement ("IComponent")->addTextElement (state.toBase64Encoding());
        juce::AudioProcessor::copyXmlToBinary (xml, envelope);
    }
    else
    {
        juce::MemoryBlock initial;
        instance->getStateInformation (initial);
        auto data = CFDataCreate (nullptr, static_cast<const UInt8*> (initial.getData()), static_cast<CFIndex> (initial.getSize()));
        auto properties = CFPropertyListCreateWithData (nullptr, data, kCFPropertyListMutableContainers, nullptr, nullptr);
        CFRelease (data);
        check (properties != nullptr && CFGetTypeID (properties) == CFDictionaryGetTypeID(), "AU wrapper state dictionary");
        auto payload = CFDataCreate (nullptr, static_cast<const UInt8*> (state.getData()), static_cast<CFIndex> (state.getSize()));
        CFDictionarySetValue (static_cast<CFMutableDictionaryRef> (const_cast<void*> (properties)), CFSTR ("jucePluginState"), payload);
        auto serialized = CFPropertyListCreateData (nullptr, properties, kCFPropertyListBinaryFormat_v1_0, 0, nullptr);
        envelope.replaceAll (CFDataGetBytePtr (serialized), static_cast<size_t> (CFDataGetLength (serialized)));
        CFRelease (serialized); CFRelease (payload); CFRelease (properties);
    }
    instance->setStateInformation (envelope.getData(), static_cast<int> (envelope.getSize()));
    const auto latencyDeadline = juce::Time::getMillisecondCounterHiRes() + 5000;
    do { juce::MessageManager::getInstance()->runDispatchLoopUntil (10); }
    while (instance->getLatencySamples() != 512 && juce::Time::getMillisecondCounterHiRes() < latencyDeadline);
    check (instance->getLatencySamples() == 512, "Nested host receives updated serial pipeline latency");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 0);
    alignedBlock (*instance, buffer, midi);
    check (buffer.getMagnitude (0, 256) > 0.01f, "Nested " + format + " resolves packaged Rust worker and renders");
    if (audioUnit != nullptr)
        if (__builtin_available (macOS 12.0, *))
        {
            midi.clear();
            midi.addEvent (juce::MidiMessage::noteOff (1, 69), 0);
            alignedBlock (*instance, buffer, midi);
            // Sine Demo has a 50 ms exponential release; allow it to decay.
            for (int block = 0; block < 100; ++block)
            {
                midi.clear(); buffer.clear(); instance->processBlock (buffer, midi);
            }
            check (buffer.getMagnitude (0, 256) < 1.0e-5f, "AU UMP check starts from silence");
            // Exercise both forms used by modern AU hosts, rather than only
            // JUCE's legacy MusicDeviceMIDIEvent delivery path.
            for (auto protocol : { kMIDIProtocol_1_0, kMIDIProtocol_2_0 })
            {
                MIDIEventList list {};
                auto* packet = MIDIEventListInit (&list, protocol);
                const UInt32 on1[] { 0x20904864 };
                const UInt32 on2[] { 0x40904800, 0xc9240000 };
                check (MIDIEventListAdd (&list, sizeof (list), packet, 0,
                           protocol == kMIDIProtocol_1_0 ? 1 : 2,
                           protocol == kMIDIProtocol_1_0 ? on1 : on2) != nullptr, "Build AU UMP note");
                check (MusicDeviceMIDIEventList (audioUnit, 0, &list) == noErr, "AU accepts UMP note");
                midi.clear(); alignedBlock (*instance, buffer, midi);
                check (buffer.getMagnitude (0, 256) > 0.01f, "AU UMP note produces audible output");
                midi.clear(); midi.addEvent (juce::MidiMessage::noteOff (1, 72), 0);
                alignedBlock (*instance, buffer, midi);
                for (int block = 0; block < 100; ++block)
                {
                    midi.clear(); buffer.clear(); instance->processBlock (buffer, midi);
                }
                check (buffer.getMagnitude (0, 256) < 1.0e-5f, "AU UMP note releases");
            }
            std::cout << "PASS AU MIDI 1 and MIDI 2 event-list delivery\n";
        }
    instance->releaseResources();
    std::cout << "PASS nested Cadence " << format << " loads and renders the sandboxed protocol chain\n";
}
}

static void clapDynamicTiming()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    auto p = open (describe (path.getFullPathName(), "CLAP"));
    p->setNonRealtime (true);
    p->prepareToPlay (48000, 256);
    const auto baseline = p->getLatencySamples();
    auto* gain = p->getParameters()[0];
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;

    gain->setValueNotifyingHost (0.9f);
    p->processBlock (buffer, midi);
    juce::MessageManager::getInstance()->runDispatchLoopUntil (50);
    check (p->getLatencySamples() == baseline + 128,
           "CLAP latency change is reflected in the host without reload");

    gain->setValueNotifyingHost (0.1f);
    p->processBlock (buffer, midi);
    juce::MessageManager::getInstance()->runDispatchLoopUntil (50);
    check (p->getLatencySamples() == baseline,
           "CLAP latency change back to zero is reflected in the host");
    std::cout << "PASS CLAP dynamic latency update\n";
}

static void clapParameterFeedback()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    auto p = open (describe (path.getFullPathName(), "CLAP"));
    p->setNonRealtime (true);
    p->prepareToPlay (48000, 256);
    auto* gain = p->getParameters()[0];
    check (std::abs (gain->getValue() - 0.5f) < 0.0001f, "CLAP initial value");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 127, static_cast<juce::uint8> (100)), 0);
    p->processBlock (buffer, midi);
    juce::MessageManager::getInstance()->runDispatchLoopUntil (50);
    check (std::abs (gain->getValue() - 0.25f) < 0.0001f,
           "CLAP rescan feedback updates the host parameter value");
    std::cout << "PASS CLAP parameter value feedback\n";
}

static void clapNativeEditorLifecycle()
{
    const auto path = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    auto p = open (describe (path.getFullPathName(), "CLAP"));
    std::unique_ptr<juce::AudioProcessorEditor> editor (p->createEditor());
    auto findButton = [&]() -> juce::TextButton*
    {
        for (auto* child : editor->getChildren())
            if (auto* button = dynamic_cast<juce::TextButton*> (child))
                if (button->getName() == "Native CLAP editor") return button;
        return nullptr;
    };
    auto wait = [&] (const std::function<bool()>& condition, const char* message)
    {
        const auto deadline = juce::Time::getMillisecondCounterHiRes() + 5000;
        while (! condition() && juce::Time::getMillisecondCounterHiRes() < deadline)
            juce::MessageManager::getInstance()->runDispatchLoopUntil (20);
        check (condition(), message);
    };
    wait ([&] { return findButton() != nullptr && findButton()->isEnabled(); }, "Native editor becomes available");
    findButton()->onClick();
    wait ([&] { return (p->nativeEditorState() & 2u) != 0 && findButton()->getButtonText() == "Close native editor"; }, "Native editor opens from Inspector");

    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    midi.addEvent (juce::MidiMessage::noteOn (1, 120, static_cast<juce::uint8> (100)), 0);
    p->processBlock (buffer, midi);
    wait ([&] { return (p->nativeEditorState() & 2u) == 0 && findButton()->getButtonText() == "Open native editor"; }, "Title-bar close updates Inspector button");
    findButton()->onClick();
    wait ([&] { return (p->nativeEditorState() & 2u) != 0; }, "Editor reopens on first click after native close");
    p->editorBeingDeleted (editor.get());
    editor.reset (p->createEditor());
    wait ([&] { return findButton() != nullptr && findButton()->getButtonText() == "Close native editor"; }, "Recreated Inspector adopts actual native window state");
    findButton()->onClick();
    wait ([&] { return (p->nativeEditorState() & 2u) == 0; }, "Inspector close uses documented C ABI close request");
    p->editorBeingDeleted (editor.get());
    editor.reset();
    std::cout << "PASS CLAP native editor button, title-bar close and Inspector recreation\n";
}

static void clapSidechainRouting()
{
    const auto fixture = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    juce::String error;
    auto variants = cadence::RustPluginProcessor::describeAll (fixture.getFullPathName(), "CLAP", error);
    check (error.isEmpty() && variants.size() == 3, "Find sidechain fixture variant");
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence sidechain", "", false);
    check (directory.createDirectory().wasOk(), "Create sidechain workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };

    CadenceAudioProcessor processor ({ cadence::RustPluginProcessor::workerFile(), directory });
    processor.setNonRealtime (true);
    processor.enableAllBuses();
    processor.prepareToPlay (48000, 256);
    check (processor.getTotalNumInputChannels() == 2, "Enable the host sidechain input bus");
    check (processor.addPlugin (variants[2], error), "Add sidechain fixture: " + error);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    auto runBlock = [&] (float sidechain, bool noteOn)
    {
        for (int channel = 0; channel < 2; ++channel)
            juce::FloatVectorOperations::fill (buffer.getWritePointer (channel), sidechain, 256);
        midi.clear();
        if (noteOn) midi.addEvent (juce::MidiMessage::noteOn (1, 60, static_cast<juce::uint8> (100)), 0);
        processor.processBlock (buffer, midi);
    };

    runBlock (0.5f, true);   // latency
    runBlock (0.5f, false);  // first sounding output
    check (std::abs (buffer.getSample (0, 64) - 0.75f) < 0.0001f,
           "Rack sidechain input reaches the worker and scales the output");
    runBlock (0.0f, false);  // previous block still had 0.5
    runBlock (0.0f, false);  // sidechain cleared
    check (std::abs (buffer.getSample (0, 64) - 0.5f) < 0.0001f,
           "Clearing the rack sidechain input returns the dry gain");
    std::cout << "PASS rack sidechain routing\n";
}

void runBridgeTests (const juce::String& vst, const juce::String& au, const juce::String& scanner)
{
    clapNotesParametersAndState();
    clapDynamicTiming();
    clapParameterFeedback();
    clapNativeEditorLifecycle();
    clapSidechainRouting();
    clapMidiOutput();
    clapStructuralReload();
    const auto state = protocolChain();
    kontaktIfInstalled();
    scanImports (scanner);
    runPipelineTests();
    if (vst.isNotEmpty()) nested (vst, "VST3", state);
    if (au.isNotEmpty())
    {
        const auto fixture = juce::File (__FILE__).getSiblingFile ("fixtures/legacy-cadence-chain.xml");
        auto xml = juce::parseXML (fixture);
        check (xml != nullptr && xml->hasTagName ("CADENCE"), "Read legacy AU rack fixture");
        juce::MemoryBlock legacyState;
        juce::AudioProcessor::copyXmlToBinary (*xml, legacyState);
        nested (au, "AudioUnit", legacyState);
        std::cout << "PASS legacy Logic rack state through the native AU wrapper\n";
    }
}
