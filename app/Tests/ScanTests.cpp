#include "PluginProcessor.h"
#include <csignal>
#include <iostream>
#include <stdexcept>
#include <unistd.h>

#include "BridgeTests.h"

namespace
{
void require (bool condition, const char* message)
{
    if (! condition)
        throw std::runtime_error (message);
}

bool until (const std::function<bool()>& predicate, int timeout = 5000, bool pump = false)
{
    const auto end = juce::Time::getMillisecondCounterHiRes() + timeout;
    while (! predicate())
    {
        if (juce::Time::getMillisecondCounterHiRes() > end)
            return false;
        if (pump)
            juce::MessageManager::getInstance()->runDispatchLoopUntil (10);
        else
            juce::Thread::sleep (10);
    }
    return true;
}

struct Workspace
{
    juce::File directory = cadenceTestTempDirectory()
        .getNonexistentChildFile ("Cadence scan tests with spaces", "", false);
    Workspace() { require (directory.createDirectory().wasOk(), "create temporary directory"); }
    ~Workspace() { directory.deleteRecursively(); }
    cadence::ScanCandidate candidate (const juce::String& name) const
    {
        return { "VST3", directory.getChildFile (name + ".vst3").getFullPathName() };
    }
    cadence::PluginScanner::Options options (int timeout = 1500) const
    {
        return { juce::File::getSpecialLocation (juce::File::currentExecutableFile), directory, timeout };
    }
};

juce::File pidFile (const cadence::ScanCandidate& candidate)
{
    return juce::File (candidate.path + ".pid");
}

int childPid (const cadence::ScanCandidate& candidate)
{
    require (until ([&] { return pidFile (candidate).existsAsFile(); }), "child started");
    const auto pid = pidFile (candidate).loadFileAsString().getIntValue();
    require (pid > 0, "child pid is valid");
    return pid;
}

void reaped (int pid)
{
    require (until ([&] { return ::kill (pid, 0) != 0; }, 2000), "child was killed and reaped");
}

cadence::ScanSnapshot finished (cadence::PluginScanner& scanner)
{
    require (until ([&] { return ! scanner.snapshot().running; }), "scan completed");
    return scanner.snapshot();
}

// The test executable doubles as a controlled worker. It never loads installed
// plugins. Each fixture records its pid, so cancellation tests detect orphans.
int fixture (const char* format, const char* path, const char* destination)
{
    const cadence::ScanCandidate candidate { format, path };
    pidFile (candidate).replaceWithText (juce::String (::getpid()));
    if (candidate.name().startsWith ("hang"))
        for (;;) juce::Thread::sleep (100);
    if (candidate.name() == "crash")
        std::raise (SIGKILL);
    if (candidate.name() == "malformed")
        return juce::File (destination).replaceWithText ("<broken") ? 0 : 1;

    juce::XmlElement xml ("CADENCE_SCAN");
    const int count = candidate.name() == "multi" ? 2 : 1;
    for (int i = 0; i < count; ++i)
    {
        juce::PluginDescription description;
        description.name = candidate.name() + (i == 0 ? "" : " second");
        description.pluginFormatName = candidate.name() == "wrong-format" ? "Other" : format;
        description.fileOrIdentifier = path;
        description.manufacturerName = "Test";
        description.uniqueId = description.name.hashCode();
        description.numOutputChannels = 2;
        xml.addChildElement (description.createXml().release());
    }
    return xml.writeTo (juce::File (destination)) ? 0 : 1;
}

void isolationAndPersistence()
{
    Workspace work;
    const auto good = work.candidate ("good plugin");
    const auto bad = work.candidate ("crash");
    const auto hang = work.candidate ("hang-timeout");
    cadence::PluginScanner scanner (work.options (500));
    require (scanner.start ({ bad, hang, good, work.candidate ("multi"), work.candidate ("malformed"),
                              work.candidate ("wrong-format") }, {}), "start scan");
    const auto result = finished (scanner);
    require (result.plugins.size() == 3, "continue past crash/hang and enumerate all types");
    require (result.skipped.size() == 4, "remember crash, timeout, malformed and mismatched output");
    require (result.skipped[1].reason == "Timed out", "report timeout");
    reaped (childPid (hang));
    reaped (childPid (bad));

    pidFile (bad).deleteFile();
    cadence::PluginScanner restored (work.options());
    require (restored.snapshot().skipped.size() == 4, "load skip list after restart");
    restored.start ({ bad }, result.plugins);
    require (finished (restored).plugins.size() == 3, "preserve prior catalogue on failure");
    require (! pidFile (bad).existsAsFile(), "ordinary single-candidate scan must respect quarantine");

    // A user retry does launch the quarantined candidate, but preserves the
    // skip if it still fails. No broad implicit retry can reopen prompt floods.
    restored.start ({ bad }, result.plugins, true);
    require (finished (restored).skipped.size() == 4, "failed explicit retry remains skipped");
    require (pidFile (bad).existsAsFile(), "explicit retry launches helper");
    restored.start ({ good }, result.plugins);
    require (finished (restored).plugins.size() == 3, "rescan deduplicates descriptors");
}

void cancelSkipAndDestruction()
{
    Workspace work;
    const auto first = work.candidate ("good");
    const auto hang = work.candidate ("hang-cancel");
    const auto never = work.candidate ("never");
    cadence::PluginScanner scanner (work.options (15000));
    scanner.start ({ first, hang, never }, {});
    const int pid = childPid (hang);
    require (! scanner.start ({ never }, {}), "reject overlapping scan in one instance");
    {
        cadence::PluginScanner other (work.options());
        other.start ({ never }, {});
        require (finished (other).status.contains ("Another Cadence instance"), "exclude scans in another instance");
        require (! pidFile (never).existsAsFile(), "overlapping instance launches no helper");
    }
    scanner.cancel();
    const auto cancelled = finished (scanner);
    require (cancelled.plugins.size() == 1, "cancel retains completed results");
    require (cancelled.status.startsWith ("Scan cancelled"), "report cancellation");
    require (! pidFile (never).existsAsFile(), "cancel does not probe later plugins");
    reaped (pid);

    const auto skip = work.candidate ("hang-skip");
    scanner.start ({ skip, never }, cancelled.plugins);
    const int skipPid = childPid (skip);
    scanner.skipCurrent();
    const auto skipped = finished (scanner);
    require (skipped.plugins.size() == 2, "skip continues scanning");
    require (skipped.skipped.back().reason == "Skipped by user", "save user skip");
    reaped (skipPid);

    const auto destroy = work.candidate ("hang-destruction");
    int destroyPid = 0;
    {
        cadence::PluginScanner scoped (work.options (15000));
        scoped.start ({ destroy }, {});
        destroyPid = childPid (destroy);
    }
    reaped (destroyPid);
}

void retryRecoveryAndInfrastructure()
{
    Workspace work;
    const auto recovered = work.candidate ("recovered");
    // Simulate a parent that died after journalling but before completing.
    juce::XmlElement journal ("CADENCE_SKIPPED");
    auto* item = journal.createNewChildElement ("PLUGIN");
    item->setAttribute ("format", recovered.format);
    item->setAttribute ("path", recovered.path);
    item->setAttribute ("reason", "Interrupted scan");
    journal.writeTo (work.directory.getChildFile ("skipped-plugins.xml"));
    cadence::PluginScanner scanner (work.options());
    scanner.start ({ recovered }, {});
    require (finished (scanner).plugins.empty(), "interrupted plugin is skipped after restart");
    scanner.start ({ recovered }, {}, true);
    const auto retried = finished (scanner);
    require (retried.plugins.size() == 1 && retried.skipped.empty(), "successful retry clears skip");

    auto missingOptions = work.options();
    missingOptions.helper = work.directory.getChildFile ("missing-helper");
    cadence::PluginScanner missing (missingOptions);
    missing.start ({ recovered }, retried.plugins);
    const auto absent = finished (missing);
    require (absent.status.contains ("helper is missing") && absent.skipped.empty(), "missing helper is not plugin failure");

    auto brokenOptions = work.options();
    brokenOptions.helper = work.directory.getChildFile ("non-executable-helper");
    brokenOptions.helper.replaceWithText ("not an executable");
    cadence::PluginScanner broken (brokenOptions);
    broken.start ({ recovered }, retried.plugins);
    const auto launchFailure = finished (broken);
    require (launchFailure.status.contains ("Could not launch") && launchFailure.skipped.empty()
             && launchFailure.plugins.size() == 1, "launch failure preserves catalogue and skip list");

    work.directory.getChildFile ("skipped-plugins.xml").replaceWithText ("broken recovery file");
    scanner.start ({ recovered }, retried.plugins);
    require (finished (scanner).status.contains ("Cannot read"), "corrupt skip list stops safely");
}

void uiAndCache()
{
    Workspace work;
    CadenceAudioProcessor processor (work.options());
    std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
    const auto candidate = work.candidate ("UI plugin");
    processor.retrySkippedPlugin (candidate);
    require (until ([&] { return ! processor.isScanning() && processor.getPlugins().size() == 1; },
                    5000, true), "processor receives async results");
    // Give the editor timer a turn, then select row zero. Without updateContent
    // the ListBox still has zero rows even though its model can see the plugin.
    juce::MessageManager::getInstance()->runDispatchLoopUntil (80);
    juce::ListBox* browser = nullptr;
    juce::TextButton* scan = nullptr;
    for (auto* child : editor->getChildren())
    {
        if (auto* list = dynamic_cast<juce::ListBox*> (child); list != nullptr && list->getName() == "Library")
            browser = list;
        if (auto* button = dynamic_cast<juce::TextButton*> (child); button != nullptr && button->getButtonText() == "Scan")
            scan = button;
    }
    require (browser != nullptr && scan != nullptr, "browser and scan control exist");
    browser->selectRow (0);
    require (browser->getSelectedRow() == 0, "browser updates after background scan");
    editor.reset();

    CadenceAudioProcessor restored (work.options());
    require (restored.getPlugins().size() == 1, "cache survives processor restart");
    const auto hang = work.candidate ("hang-ui");
    editor.reset (processor.createEditor());
    processor.retrySkippedPlugin (hang);
    const int pid = childPid (hang);
    juce::MessageManager::getInstance()->runDispatchLoopUntil (80);
    juce::TextButton* cancel = nullptr;
    juce::TextButton* skip = nullptr;
    for (auto* child : editor->getChildren())
        if (auto* button = dynamic_cast<juce::TextButton*> (child))
        {
            if (button->getButtonText() == "Cancel") cancel = button;
            if (button->getButtonText() == "Skip plugin") skip = button;
        }
    require (cancel != nullptr && skip != nullptr && skip->isVisible() && skip->isEnabled(),
             "cancel and skip stay available during a hung scan");

    const auto image = editor->createComponentSnapshot (editor->getLocalBounds());
    juce::File ("/tmp/cadence-scanning-preview.png").deleteFile();
    if (auto output = juce::File ("/tmp/cadence-scanning-preview.png").createOutputStream())
        juce::PNGImageFormat().writeImageToStream (image, *output);
    cancel->triggerClick();
    require (until ([&] { return ! processor.isScanning(); }, 3000, true), "cancel button stops worker");
    reaped (pid);
    editor.reset();
    juce::MessageManager::getInstance()->runDispatchLoopUntil (30);
}
}

int main (int argc, char** argv)
{
    if (argc == 4 && (juce::String (argv[1]) == "VST3" || juce::String (argv[1]) == "AudioUnit"))
        return fixture (argv[1], argv[2], argv[3]);
    juce::ScopedJuceInitialiser_GUI init;
    try
    {
        if (argc == 2 && juce::String (argv[1]) == "--pipeline-only") { runPipelineTests(); return 0; }
        if (argc == 2 && juce::String (argv[1]) == "--loading-only") { runLoadingTests(); return 0; }
        if (argc == 2 && juce::String (argv[1]) == "--automation-only") { runAutomationTests(); return 0; }
        if (argc == 2 && juce::String (argv[1]) == "--workgroup-only") { runWorkgroupTests(); return 0; }
        if (argc == 2 && juce::String (argv[1]) == "--stress-only") { runStressTests(); return 0; }
        runLoadingTests();
        runAutomationTests();
        runWorkgroupTests();
        isolationAndPersistence(); std::cout << "PASS crash, timeout, malformed result, persistence, retry, deduplication\n";
        cancelSkipAndDestruction(); std::cout << "PASS cancellation, skip, partial results, destruction, child reaping\n";
        retryRecoveryAndInfrastructure(); std::cout << "PASS interrupted scan recovery, successful retry, missing helper, corrupt journal\n";
        uiAndCache(); std::cout << "PASS live browser refresh, cache reload, cancel/skip controls\n";
        runBridgeTests (argc >= 2 ? juce::String (argv[1]) : juce::String(), argc >= 3 ? juce::String (argv[2]) : juce::String(),
                        argc >= 4 ? juce::String (argv[3]) : juce::String());
    }
    catch (const std::exception& error)
    {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
    return 0;
}
