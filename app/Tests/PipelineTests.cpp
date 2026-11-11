#include "BridgeTests.h"
#include "PluginProcessor.h"
#include "RustPluginProcessor.h"
#include <chrono>
#include <exception>
#include <functional>
#include <iostream>
#include <stdexcept>
#include <thread>

namespace
{
void require (bool condition, const juce::String& message)
{
    if (! condition) throw std::runtime_error (message.toStdString());
}
juce::PluginDescription describe (const juce::String& source)
{
    juce::String error;
    auto result = cadence::RustPluginProcessor::describe (source, "Protocol", error);
    require (error.isEmpty() && result.name.isNotEmpty(), error);
    return result;
}
std::unique_ptr<cadence::RustPluginProcessor> gain (int block, bool offline)
{
    juce::String error;
    auto result = cadence::RustPluginProcessor::open (describe ("builtin:gain"), error);
    require (result != nullptr, error);
    result->setNonRealtime (offline);
    result->prepareToPlay (48000, block);
    require (result->getLatencySamples() == block, "One-buffer latency is reported to JUCE");
    require (result->getTailLengthSeconds() >= static_cast<double> (block) / 48000.0, "Buffered audio is included in the tail");
    return result;
}

// DAWs can deliver latency-compensated MIDI before project sample zero.
struct PrerollPlayHead final : juce::AudioPlayHead
{
    juce::int64 sample = -512;
    juce::Optional<PositionInfo> getPosition() const override
    {
        PositionInfo info;
        info.setTimeInSamples (sample);
        info.setPpqPosition (static_cast<double> (sample) / 24000.0);
        info.setIsPlaying (true);
        return info;
    }
};

void negativePreroll()
{
    auto processor = gain (256, true);
    PrerollPlayHead playhead;
    processor->setPlayHead (&playhead);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    for (int block = 0; block < 5; ++block)
    {
        for (int channel = 0; channel < 2; ++channel)
            juce::FloatVectorOperations::fill (buffer.getWritePointer (channel), 0.25f, 256);
        processor->processBlock (buffer, midi);
        require (! processor->hasFailed(), "Worker survives negative preroll");
        for (int frame = 0; frame < 256; ++frame)
            require (std::abs (buffer.getSample (0, frame) - (block == 0 ? 0.0f : 0.25f)) < 0.00001f,
                     "Negative preroll through zero must preserve delayed audio");
        playhead.sample += 256;
    }
    processor->setPlayHead (nullptr);

    juce::String error;
    auto sine = cadence::RustPluginProcessor::open (describe ("builtin:sine"), error);
    require (sine != nullptr, error);
    sine->setNonRealtime (true);
    sine->prepareToPlay (48000, 256);
    playhead.sample = -512;
    sine->setPlayHead (&playhead);
    for (int block = 0; block < 5; ++block)
    {
        buffer.clear(); midi.clear();
        if (block == 0) midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 0);
        sine->processBlock (buffer, midi);
        require (! sine->hasFailed(), "Sine worker survives preroll");
        require (block == 0 ? buffer.getMagnitude (0, 256) == 0.0f : buffer.getMagnitude (0, 256) > 0.1f,
                 "A note delivered during preroll must keep sounding across zero");
        playhead.sample += 256;
    }
    sine->setPlayHead (nullptr);
    std::cout << "PASS negative preroll preserves delayed audio and held notes across zero\n";
}

void variableOfflineBuffers()
{
    auto processor = gain (256, true);
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    uint64_t position = 0;
    const std::array<int, 4> sizes { 17, 64, 128, 256 };
    for (int block = 0; block < 200; ++block)
    {
        const auto frames = sizes[static_cast<size_t> (block) % sizes.size()];
        buffer.setSize (2, frames, false, false, true);
        for (int channel = 0; channel < 2; ++channel)
            for (int frame = 0; frame < frames; ++frame)
                buffer.setSample (channel, frame, 0.2f + static_cast<float> ((position + static_cast<uint64_t> (frame)) % 97) * 0.001f);
        processor->processBlock (buffer, midi);
        require (! processor->hasFailed(), "Offline variable-block worker stays healthy");
        for (int frame = 0; frame < frames; ++frame)
        {
            const auto sample = position + static_cast<uint64_t> (frame);
            const auto expected = sample < 256 ? 0.0f : 0.2f + static_cast<float> ((sample - 256) % 97) * 0.001f;
            require (std::abs (buffer.getSample (0, frame) - expected) < 0.00001f, "Offline output keeps its exact reported sample delay");
        }
        position += static_cast<uint64_t> (frames);
    }
    require (processor->underruns() == 0, "Offline rendering never underruns");
    processor->prepareToPlay (48000, 128);
    buffer.setSize (2, 8192);
    for (int channel = 0; channel < 2; ++channel) juce::FloatVectorOperations::fill (buffer.getWritePointer (channel), 0.25f, 8192);
    processor->processBlock (buffer, midi);
    for (int frame = 0; frame < 8192; ++frame)
        require (std::abs (buffer.getSample (0, frame) - (frame < 128 ? 0.0f : 0.25f)) < 0.00001f,
                 "Large offline callbacks split without changing the delay");
    std::cout << "PASS exact delay with variable/offline/large callbacks and timeline wrap\n";
}

void parameterRecallWithoutPlayback()
{
    // Saving while transport is stopped must retain the UI value even when
    // no callback has delivered that value to the worker yet.
    const auto description = describe ("builtin:gain");
    const auto directory = cadenceTestTempDirectory().getNonexistentChildFile ("Cadence stopped recall", "", false);
    const juce::ScopeGuard cleanup { [&] { directory.deleteRecursively(); } };
    CadenceAudioProcessor original ({ cadence::RustPluginProcessor::workerFile(), directory });
    original.setNonRealtime (true);
    original.prepareToPlay (48000, 256);
    juce::String error;
    require (original.addPlugin (description, error), error);
    original.getSlotProcessor (0)->getParameters()[0]->setValueNotifyingHost (48.0f / 72.0f); // -12 dB
    juce::MemoryBlock state;
    original.getStateInformation (state);
    CadenceAudioProcessor restored ({ cadence::RustPluginProcessor::workerFile(), directory });
    restored.setNonRealtime (true);
    restored.prepareToPlay (48000, 256);
    restored.setStateInformation (state.getData(), static_cast<int> (state.getSize()));
    require (cadenceWaitForLoading (restored), "Asynchronous stopped-parameter recall completes");
    require (restored.getNumSlots() == 1, "Fresh rack restores the saved slot");
    auto* plugin = dynamic_cast<cadence::RustPluginProcessor*> (restored.getSlotProcessor (0));
    require (plugin != nullptr, "Fresh rack restores Rust worker");
    require (std::abs (plugin->getParameters()[0]->getValue() - 48.0f / 72.0f) < 0.00001f,
             "Stopped parameter edit restores in inspector");
    juce::AudioBuffer<float> buffer (2, 256);
    juce::MidiBuffer midi;
    for (int block = 0; block < 3; ++block)
    {
        for (int channel = 0; channel < 2; ++channel)
            juce::FloatVectorOperations::fill (buffer.getWritePointer (channel), 0.5f, 256);
        plugin->processBlock (buffer, midi);
        require (! plugin->hasFailed(), "Restored gain worker remains healthy");
        const auto expected = block == 0 ? 0.0f : 0.5f * std::pow (10.0f, -12.0f / 20.0f);
        for (int channel = 0; channel < 2; ++channel)
            for (int frame = 0; frame < 256; ++frame)
                require (std::abs (buffer.getSample (channel, frame) - expected) < 0.00001f,
                         "Freshly restored worker applies non-default gain to audio");
    }
    std::cout << "PASS stopped non-default gain save/reopen: inspector and measured audio\n";
}

void parallelDelayCompensation()
{
    for (int frames : { 64, 128, 256 })
    {
        juce::AudioProcessorGraph graph;
        graph.setNonRealtime (true);
        graph.setPlayConfigDetails (2, 2, 48000, frames);
        auto input = graph.addNode (std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::audioInputNode));
        auto output = graph.addNode (std::make_unique<juce::AudioProcessorGraph::AudioGraphIOProcessor> (
            juce::AudioProcessorGraph::AudioGraphIOProcessor::audioOutputNode));
        auto delayed = graph.addNode (gain (frames, true));
        for (int channel = 0; channel < 2; ++channel)
        {
            require (graph.addConnection ({ { input->nodeID, channel }, { delayed->nodeID, channel } }), "Connect delayed input");
            require (graph.addConnection ({ { delayed->nodeID, channel }, { output->nodeID, channel } }), "Connect delayed output");
            require (graph.addConnection ({ { input->nodeID, channel }, { output->nodeID, channel } }), "Connect undelayed parallel reference");
        }
        graph.setPlayConfigDetails (2, 2, 48000, frames);
        graph.prepareToPlay (48000, frames);
        juce::MessageManager::getInstance()->runDispatchLoopUntil (40);
        require (graph.getLatencySamples() == frames, "Graph reports the maximum path delay");
        juce::AudioBuffer<float> buffer (2, frames);
        juce::MidiBuffer midi;
        for (int block = 0; block < 4; ++block)
        {
            buffer.clear();
            if (block == 0) for (int channel = 0; channel < 2; ++channel) buffer.setSample (channel, 17, 0.5f);
            graph.processBlock (buffer, midi);
            for (int frame = 0; frame < frames; ++frame)
            {
                const auto expected = block == 1 && frame == 17 ? 1.0f : 0.0f;
                require (std::abs (buffer.getSample (0, frame) - expected) < 0.00001f,
                         "JUCE PDC must align delayed and direct impulses sample-for-sample");
            }
        }
        graph.releaseResources();
    }
    std::cout << "PASS parallel-path delay compensation at 64/128/256 samples\n";
}

struct RealtimeCallbackThread final : juce::Thread
{
    explicit RealtimeCallbackThread (std::function<void()> callback)
        : Thread ("Cadence test audio"), task (std::move (callback)) {}
    void run() override
    {
        try { task(); }
        catch (...) { error = std::current_exception(); }
    }
    std::function<void()> task;
    std::exception_ptr error;
};

void realtimeBenchmark()
{
    using Clock = std::chrono::steady_clock;
    const auto callbacks = juce::jlimit (1000, 100000,
        juce::SystemStats::getEnvironmentVariable ("CADENCE_BENCH_CALLBACKS", "1000").getIntValue());
    const auto sine = describe ("builtin:sine");
    const auto effect = describe ("builtin:gain");
    juce::Array<juce::var> results;
    uint64_t totalUnderruns = 0;
    bool allHealthy = true;
    for (int frames : { 64, 128, 256 })
    {
        const auto settings = cadenceTestTempDirectory()
            .getNonexistentChildFile ("Cadence realtime benchmark", "", false);
        const juce::ScopeGuard cleanup { [&] { settings.deleteRecursively(); } };
        CadenceAudioProcessor processor ({ cadence::RustPluginProcessor::workerFile(), settings });
        processor.setNonRealtime (false);
        processor.prepareToPlay (48000, frames);
        juce::String error;
        require (processor.addPlugin (sine, error) && processor.addPlugin (effect, error), "Build realtime chain: " + error);
        juce::MessageManager::getInstance()->runDispatchLoopUntil (40);
        require (processor.getLatencySamples() == 2 * frames, "Serial node delays are summed for the DAW");
        juce::AudioBuffer<float> buffer (2, frames);
        juce::MidiBuffer midi; midi.ensureSize (1024);
        std::vector<double> durations; durations.reserve (static_cast<size_t> (callbacks));
        const auto period = std::chrono::duration<double> (static_cast<double> (frames) / 48000.0);
        double maxWakeLateness = 0;
        float peak = 0.0f;
        RealtimeCallbackThread callbackThread ([&]
        {
            auto next = Clock::now();
            for (int block = 0; block < callbacks; ++block)
            {
                std::this_thread::sleep_until (next);
                midi.clear();
                if (block == 0) midi.addEvent (juce::MidiMessage::noteOn (1, 69, uint8_t (100)), 0);
                if (block == callbacks * 9 / 10) midi.addEvent (juce::MidiMessage::noteOff (1, 69), 0);
                const auto before = Clock::now();
                maxWakeLateness = juce::jmax (maxWakeLateness,
                    std::chrono::duration<double, std::micro> (before - next).count());
                // A late timer wake must not compress subsequent periods into
                // back-to-back callbacks, removing the worker's one-buffer budget.
                next = before + std::chrono::duration_cast<Clock::duration> (period);
                processor.processBlock (buffer, midi);
                const auto after = Clock::now();
                durations.push_back (std::chrono::duration<double, std::micro> (after - before).count());
                peak = juce::jmax (peak, buffer.getMagnitude (0, frames));
                for (int channel = 0; channel < 2; ++channel)
                    for (int frame = 0; frame < frames; ++frame)
                        if (! std::isfinite (buffer.getSample (channel, frame)))
                            require (false, "Realtime audio stays finite");
            }
        });
        const auto periodMs = static_cast<double> (frames) / 48.0;
        require (callbackThread.startRealtimeThread (juce::Thread::RealtimeOptions()
                     .withPeriodMs (periodMs).withProcessingTimeMs (juce::jmin (0.5, periodMs * 0.2))
                     .withMaximumProcessingTimeMs (periodMs)), "Create realtime host callback thread");
        require (callbackThread.waitForThreadToExit (600000), "Realtime benchmark finishes");
        if (callbackThread.error != nullptr) std::rethrow_exception (callbackThread.error);
        std::sort (durations.begin(), durations.end());
        bool healthy = true, scheduled = true;
        for (int slot = 0; slot < processor.getNumSlots(); ++slot)
            if (auto* p = dynamic_cast<cadence::RustPluginProcessor*> (processor.getSlotProcessor (slot))) { healthy &= ! p->hasFailed(); scheduled &= p->realtimeSchedulingActive(); }
        const auto underruns = processor.getUnderruns();
        totalUnderruns += underruns; allHealthy &= healthy && scheduled && peak > 0.01f;
        auto* record = new juce::DynamicObject();
        record->setProperty ("realtime_callback_thread", true);
        record->setProperty ("realtime_workers", scheduled);
        record->setProperty ("buffer_frames", frames); record->setProperty ("sample_rate", 48000);
        record->setProperty ("callbacks", callbacks); record->setProperty ("rack_latency_frames", processor.getLatencySamples());
        record->setProperty ("callback_budget_us", frames * 1000000.0 / 48000.0);
        record->setProperty ("p50_us", durations[static_cast<size_t> (callbacks / 2)]);
        record->setProperty ("p95_us", durations[static_cast<size_t> (callbacks * 95 / 100)]);
        record->setProperty ("p99_us", durations[static_cast<size_t> (callbacks * 99 / 100)]); record->setProperty ("max_us", durations.back());
        record->setProperty ("max_wake_lateness_us", maxWakeLateness);
        record->setProperty ("underruns", static_cast<juce::int64> (underruns)); record->setProperty ("healthy", healthy);
        results.add (juce::var (record));
        std::cout << "BENCH " << frames << " frames: p99 " << durations[static_cast<size_t> (callbacks * 99 / 100)] << " us, max " << durations.back()
                  << " us, underruns " << underruns << ", latency " << processor.getLatencySamples() << " samples" << std::endl;
    }
    juce::File::getCurrentWorkingDirectory().getChildFile ("pipeline-benchmark.json").replaceWithText (juce::JSON::toString (juce::var (results)));
    require (allHealthy, "Realtime workers must remain healthy and audible");
    require (totalUnderruns == 0, "Paced realtime benchmark recorded worker underruns; inspect pipeline-benchmark.json");
}
}

void runPipelineTests()
{
    negativePreroll();
    parameterRecallWithoutPlayback();
    variableOfflineBuffers();
    parallelDelayCompensation();
    realtimeBenchmark();
}
