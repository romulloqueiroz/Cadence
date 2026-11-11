#include "BridgeTests.h"
#include "PluginEditor.h"
#include "RustPluginProcessor.h"
#include <iostream>
#include <stdexcept>

namespace
{
void check (bool condition, const char* message)
{
    if (! condition) throw std::runtime_error (message);
}
juce::PluginDescription describe (const juce::File& file)
{
    juce::String error;
    auto description = cadence::RustPluginProcessor::describe (file.getFullPathName(), "CLAP", error);
    check (error.isEmpty() && description.name.isNotEmpty(), "Describe automation fixture");
    return description;
}
void fillBlock (CadenceAudioProcessor& processor, bool noteOn)
{
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    if (noteOn)
        midi.addEvent (juce::MidiMessage::noteOn (1, 60, static_cast<juce::uint8> (100)), 0);
    processor.processBlock (buffer, midi);
}
juce::ComboBox* findCombo (juce::Component& parent, const juce::String& name)
{
    if (auto* combo = dynamic_cast<juce::ComboBox*> (&parent))
        if (combo->getName() == name)
            return combo;
    for (int i = 0; i < parent.getNumChildComponents(); ++i)
        if (auto* found = findCombo (*parent.getChildComponent (i), name))
            return found;
    return nullptr;
}
juce::TextButton* findButton (juce::Component& parent, const juce::String& name)
{
    if (auto* button = dynamic_cast<juce::TextButton*> (&parent))
        if (button->getName() == name)
            return button;
    for (int i = 0; i < parent.getNumChildComponents(); ++i)
        if (auto* found = findButton (*parent.getChildComponent (i), name))
            return found;
    return nullptr;
}
}

void runAutomationTests()
{
    const auto fixture = cadence::RustPluginProcessor::workerFile().getSiblingFile ("bridge-fixture.clap");
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence automation", "", false);
    check (directory.createDirectory().wasOk(), "Create automation workspace");
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };

    CadenceAudioProcessor processor ({ cadence::RustPluginProcessor::workerFile(), directory });
    processor.setNonRealtime (true);
    processor.prepareToPlay (48000, 256);
    juce::String error;
    check (processor.addPlugin (describe (fixture), error), "Add automation fixture");
    check (processor.getParameters().size() == cadence::Automation::numControls, "Eight stable host controls are exposed");

    const auto targets = processor.getAutomationTargets (0);
    check (targets.size() == 1 && targets.front().name == "Gain", "Gain target has a stable automation key");
    check (processor.assignAutomation (0, 0, targets.front().key, error), "Assign host control to the gain target");
    check (processor.getAutomationAssignment (0).contains ("Gain"), "Assignment is visible");
    check (! processor.getAutomationAssignment (1).contains ("Gain"), "Only the chosen host control is assigned");

    auto* macro = processor.getParameters()[0];
    macro->setValueNotifyingHost (0.25f);
    fillBlock (processor, true);   // latency block
    fillBlock (processor, false);  // note begins at 0.25

    juce::AudioBuffer<float> observed (2, 256);
    juce::MidiBuffer midi;
    processor.processBlock (observed, midi);
    check (std::abs (observed.getSample (0, 64) - 0.25f) < 0.0001f, "Baseline host value reaches the target");

    check (processor.queueHostAutomation (0, 100, 0.75f), "Queue a sample-offset host automation point");
    fillBlock (processor, false); // worker receives the point
    processor.processBlock (observed, midi);
    check (std::abs (observed.getSample (0, 50) - 0.25f) < 0.0001f,
           "Automation does not leak backwards before its sample offset");
    check (std::abs (observed.getSample (0, 100) - 0.75f) < 0.0001f,
           "Automation changes exactly at its sample offset");
    check (std::abs (observed.getSample (0, 200) - 0.75f) < 0.0001f,
           "Automation value holds after its sample offset");

    juce::MemoryBlock saved;
    processor.getStateInformation (saved);
    {
        CadenceAudioProcessor restored ({ cadence::RustPluginProcessor::workerFile(), directory });
        restored.setNonRealtime (true);
        restored.prepareToPlay (48000, 256);
        restored.setStateInformation (saved.getData(), static_cast<int> (saved.getSize()));
        check (cadenceWaitForLoading (restored) && restored.getNumSlots() == 1, "Restore the automation rack");
        check (restored.getAutomationAssignment (0).contains ("Gain"), "Assignment survives save/reopen");
        check (std::abs (restored.getParameters()[0]->getValue() - 0.75f) < 0.0001f,
               "Host control value survives save/reopen");
        check (restored.getAutomationTargets (0).size() == 1, "Saved key resolves after asynchronous load");
    }

    {
        std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
        auto* combo = findCombo (*editor, "Automation 1");
        auto* clear = findButton (*editor, "Clear Automation 1");
        check (combo != nullptr && clear != nullptr, "Editor exposes automation assignment controls");
        check (combo->getSelectedId() > 1 && combo->isEnabled(), "Editor shows the current assignment");
        clear->triggerClick();
        juce::MessageManager::getInstance()->runDispatchLoopUntil (20);
        check (! processor.getAutomationAssignment (0).contains ("Gain"), "Clear button unmaps the control");
        check (combo->getSelectedId() == 1, "Combo reflects the cleared assignment");
        combo->setSelectedId (2, juce::sendNotificationSync);
        check (processor.getAutomationAssignment (0).contains ("Gain"), "Combo selection assigns the target");
    }

    processor.setSlotBypassed (0, true);
    for (int block = 0; block < 40; ++block)
    {
        processor.queueHostAutomation (0, 0, 0.5f);
        fillBlock (processor, false);
    }
    auto* rust = dynamic_cast<cadence::RustPluginProcessor*> (processor.getSlotProcessor (0));
    check (rust != nullptr && ! rust->hasFailed(), "Bypassed target does not overflow its automation buffer");
    processor.setSlotBypassed (0, false);
    check (processor.getAutomationAssignment (0).contains ("Gain"), "Unbypassing restores the saved assignment");

    processor.removeSlot (0);
    check (! processor.getAutomationAssignment (0).contains ("Gain"), "Removing the target clears its assignment");
    std::cout << "PASS host automation: sample-accurate delivery, recall, editor assignment and safe removal\n";
}
