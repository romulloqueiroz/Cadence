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

void runWorkgroupTests()
{
#if JUCE_MAC
    juce::AudioDeviceManager devices;
    if (devices.initialiseWithDefaultDevices (0, 2).isNotEmpty())
    {
        std::cout << "SKIP Audio Workgroup: no default output device available\n";
        return;
    }
    const auto workgroup = devices.getDeviceAudioWorkgroup();
    auto* workgroupHandle = juce::getRealAudioWorkgroupHandle (workgroup);
    if (workgroupHandle == nullptr)
    {
        std::cout << "SKIP Audio Workgroup: default output device has no workgroup\n";
        return;
    }

    juce::String error;
    auto description = cadence::RustPluginProcessor::describe ("builtin:sine", "Protocol", error);
    check (error.isEmpty() && description.name.isNotEmpty(), "Describe workgroup fixture");
    auto processor = cadence::RustPluginProcessor::open (description, error);
    check (processor != nullptr, "Open workgroup fixture");
    processor->setNonRealtime (false);
    processor->prepareToPlay (48000, 256);

    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    auto pump = [&] (int blocks)
    {
        for (int block = 0; block < blocks; ++block)
        {
            buffer.clear();
            midi.clear();
            if (block == 0) midi.addEvent (juce::MidiMessage::noteOn (1, 69, static_cast<juce::uint8> (100)), 0);
            processor->processBlock (buffer, midi);
            std::this_thread::sleep_for (std::chrono::milliseconds (5));
        }
    };

    processor->setAudioWorkgroup (workgroupHandle, 1);
    bool joined = false;
    for (int attempt = 0; attempt < 40 && ! joined; ++attempt)
    {
        pump (1);
        joined = processor->audioWorkgroupActive();
    }
    check (joined, "Worker joined the host Audio Workgroup");
    check (! processor->hasFailed(), "Workgroup membership leaves the worker healthy");

    processor->setAudioWorkgroup (nullptr, 2);
    bool left = false;
    for (int attempt = 0; attempt < 40 && ! left; ++attempt)
    {
        pump (1);
        left = ! processor->audioWorkgroupActive();
    }
    check (left, "Worker left the cleared Audio Workgroup");
    check (! processor->hasFailed(), "Clearing the workgroup leaves the worker healthy");
    std::cout << "PASS macOS Audio Workgroup: cross-process join, health and clear\n";
#else
    std::cout << "SKIP Audio Workgroup: macOS only\n";
#endif
}
