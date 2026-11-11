#pragma once

#include <juce_audio_processors/juce_audio_processors.h>
#include <atomic>
#include <functional>
#include <mutex>
#include <thread>

namespace cadence
{
struct ScanCandidate
{
    juce::String format;
    juce::String path;

    juce::String key() const { return format + ":" + path; }
    juce::String name() const { return path == "builtin:sine" ? "Sine Demo" : path == "builtin:gain" ? "Gain" : juce::File (path).getFileNameWithoutExtension(); }
};

struct SkippedPlugin
{
    ScanCandidate candidate;
    juce::String reason;
};

struct ScanSnapshot
{
    std::vector<juce::PluginDescription> plugins;
    std::vector<SkippedPlugin> skipped;
    juce::String status;
    bool running = false;
    uint64_t current = 0;
    uint64_t catalogueVersion = 0;
};

// Only the helper loads plugin code. This supervisor owns and reaps one child
// at a time, including when cancellation or destruction interrupts a scan.
class PluginScanner
{
public:
    struct Options
    {
        juce::File helper;
        juce::File directory;
        int timeoutMs = 15000;
    };

    explicit PluginScanner (Options, std::function<void()> changed = {});
    ~PluginScanner();

    bool start (std::vector<ScanCandidate>, std::vector<juce::PluginDescription> existing,
                bool retrySkipped = false);
    void cancel();
    void skipCurrent();
    ScanSnapshot snapshot() const;
    bool isRunning() const;
    bool canSkip() const;
    size_t skippedCount() const;

    static std::vector<ScanCandidate> discoverCandidates();
    static juce::File bundledHelper();

private:
    void run (std::vector<ScanCandidate>, bool retrySkipped);
    void publish (const std::function<void (ScanSnapshot&)>&);

    Options options;
    std::function<void()> changed;
    mutable std::mutex mutex;
    ScanSnapshot state;
    std::thread thread;
    std::atomic<bool> cancelled { false };
    std::atomic<uint64_t> skippedToken { 0 };
    uint64_t nextToken = 0;
};
}
