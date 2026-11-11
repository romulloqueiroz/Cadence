#pragma once
#include <juce_audio_processors/juce_audio_processors.h>
#include <atomic>
#include <condition_variable>
#include <deque>
#include <mutex>
#include <thread>

namespace cadence
{
// Owns loading independently of the editor. Only completed, prepared processors
// cross back to the message thread; the audio graph is never touched here.
class PluginLoader
{
public:
    struct Settings
    {
        double rate = 44100;
        int block = 512;
        bool offline = false;
        uint64_t generation = 0;
    };
    struct Request
    {
        uint64_t id = 0;
        juce::PluginDescription description;
        juce::MemoryBlock state;
        Settings settings;
        std::shared_ptr<std::atomic<bool>> cancelled;
    };
    struct Result
    {
        Request request;
        std::unique_ptr<juce::AudioProcessor> processor;
        juce::String error;
    };
    explicit PluginLoader (std::function<void()> changed);
    ~PluginLoader();
    void enqueue (Request);
    std::vector<Result> takeCompleted();

private:
    struct Shared;
    static void deliver (const std::shared_ptr<Shared>&, Result);
    std::shared_ptr<Shared> shared;
    std::thread worker;
};
}
