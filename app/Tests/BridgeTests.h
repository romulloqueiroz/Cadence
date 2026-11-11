#pragma once
#include <JuceHeader.h>
#include "PluginProcessor.h"
void runBridgeTests (const juce::String& vst, const juce::String& au, const juce::String& scanner);
void runPipelineTests();
void runLoadingTests();
void runAutomationTests();
void runWorkgroupTests();
void runStressTests();

inline bool cadenceWaitForLoading (CadenceAudioProcessor& processor)
{
    const auto deadline = juce::Time::getMillisecondCounterHiRes() + 5000;
    while (processor.isLoading() && juce::Time::getMillisecondCounterHiRes() < deadline)
        juce::MessageManager::getInstance()->runDispatchLoopUntil (10);
    return ! processor.isLoading();
}

inline juce::File cadenceTestTempDirectory()
{
    const auto path = juce::SystemStats::getEnvironmentVariable ("CADENCE_TEST_TMPDIR", {});
    return path.isNotEmpty() ? juce::File (path) : juce::File::getSpecialLocation (juce::File::tempDirectory);
}
