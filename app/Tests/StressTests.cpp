#include "BridgeTests.h"
#include "RustPluginProcessor.h"
#include <chrono>
#include <iostream>
#include <stdexcept>
#include <thread>

#if JUCE_MAC
 #include <os/workgroup.h>
 #include <juce_audio_basics/native/juce_AudioWorkgroup_mac.h>
#endif

namespace
{
void check (bool condition, const char* message)
{
    if (! condition) throw std::runtime_error (message);
}
}

void runStressTests()
{
#if JUCE_MAC
    juce::AudioDeviceManager devices;
    if (devices.initialiseWithDefaultDevices (0, 2).isNotEmpty())
    {
        std::cout << "SKIP stress: no default output device\n";
        return;
    }
    const auto workgroup = devices.getDeviceAudioWorkgroup();
    auto* handle = juce::getRealAudioWorkgroupHandle (workgroup);
    if (handle == nullptr)
    {
        std::cout << "SKIP stress: default output device has no workgroup\n";
        return;
    }

    juce::String error;
    auto description = cadence::RustPluginProcessor::describe ("builtin:sine", "Protocol", error);
    check (error.isEmpty(), "Describe stress fixture");
    auto processor = cadence::RustPluginProcessor::open (description, error);
    check (processor != nullptr, "Open stress fixture");
    processor->setNonRealtime (false);
    processor->prepareToPlay (48000, 256);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    const auto cycles = juce::jlimit (1, 1000,
        juce::SystemStats::getEnvironmentVariable ("CADENCE_STRESS_CYCLES", "25").getIntValue());
    for (int cycle = 0; cycle < cycles; ++cycle)
    {
        const auto joinedGeneration = static_cast<uint32_t> (cycle * 2 + 1);
        processor->setAudioWorkgroup (handle, joinedGeneration);
        bool joined = false;
        for (int attempt = 0; attempt < 80 && ! joined; ++attempt)
        {
            midi.clear();
            processor->processBlock (buffer, midi);
            std::this_thread::sleep_for (std::chrono::milliseconds (2));
            joined = processor->audioWorkgroupActive();
        }
        check (joined, "Stress cycle joined the workgroup");
        processor->setAudioWorkgroup (nullptr, joinedGeneration + 1);
        bool left = false;
        for (int attempt = 0; attempt < 80 && ! left; ++attempt)
        {
            midi.clear();
            processor->processBlock (buffer, midi);
            std::this_thread::sleep_for (std::chrono::milliseconds (2));
            left = ! processor->audioWorkgroupActive();
        }
        check (left, "Stress cycle left the workgroup");
        check (! processor->hasFailed(), "Stress cycle left the worker healthy");
    }
    std::cout << "PASS long-session workgroup churn: " << cycles << " join/leave cycles\n";
#else
    std::cout << "SKIP stress: macOS only\n";
#endif
}
