#include "PluginLoader.h"
#include "RustPluginProcessor.h"

namespace cadence
{
struct PluginLoader::Shared
{
    std::mutex mutex;
    std::condition_variable wake;
    std::deque<Request> queued;
    std::vector<Result> completed;
    std::vector<std::weak_ptr<std::atomic<bool>>> cancellations;
    std::function<void()> changed;
    bool stopping = false;
};

PluginLoader::PluginLoader (std::function<void()> changed) : shared (std::make_shared<Shared>())
{
    shared->changed = std::move (changed);
    worker = std::thread ([state = shared]
    {
        for (;;)
        {
            Request request;
            {
                std::unique_lock<std::mutex> lock (state->mutex);
                state->wake.wait (lock, [&] { return state->stopping || ! state->queued.empty(); });
                if (state->stopping) return;
                request = std::move (state->queued.front());
                state->queued.pop_front();
            }
            if (request.cancelled->load()) continue;
            Result result { std::move (request), {}, {} };
            try
            {
                auto plugin = RustPluginProcessor::open (result.request.description, result.error,
                                                        result.request.cancelled.get());
                if (plugin != nullptr)
                {
                    plugin->setNonRealtime (result.request.settings.offline);
                    plugin->enableAllBuses();
                    if (plugin->prepareForLoading (result.request.settings.rate, result.request.settings.block,
                                                  result.request.state, result.request.cancelled.get()))
                        result.processor = std::move (plugin);
                    else
                        result.error = "Instrument preparation or saved-state restore failed";
                }
            }
            catch (const std::exception& error) { result.error = error.what(); }
            catch (...) { result.error = "Instrument loading failed"; }
            deliver (state, std::move (result));
        }
    });
}

PluginLoader::~PluginLoader()
{
    {
        const std::lock_guard<std::mutex> lock (shared->mutex);
        shared->stopping = true;
        shared->changed = {};
        for (const auto& weak : shared->cancellations)
            if (auto cancel = weak.lock()) cancel->store (true);
    }
    shared->wake.notify_all();
    worker.join();
}

void PluginLoader::deliver (const std::shared_ptr<Shared>& state, Result result)
{
    const std::lock_guard<std::mutex> lock (state->mutex);
    if (state->stopping || result.request.cancelled->load()) return;
    state->completed.push_back (std::move (result));
    if (state->changed) state->changed();
}

void PluginLoader::enqueue (Request request)
{
    {
        const std::lock_guard<std::mutex> lock (shared->mutex);
        if (shared->stopping) return;
        auto& tokens = shared->cancellations;
        tokens.erase (std::remove_if (tokens.begin(), tokens.end(), [] (const auto& token) { return token.expired(); }), tokens.end());
        tokens.push_back (request.cancelled);
        if (RustPluginProcessor::supports (request.description.pluginFormatName))
        {
            shared->queued.push_back (std::move (request));
            shared->wake.notify_one();
            return;
        }
    }
    // Native AU/VST3 construction must follow JUCE's message-thread contract.
    // Keep its format manager alive until the asynchronous factory returns,
    // even if the owning rack or editor has already gone away.
    auto formats = std::make_shared<juce::AudioPluginFormatManager>();
    formats->addFormat (std::make_unique<juce::VST3PluginFormat>());
   #if JUCE_MAC && JUCE_PLUGINHOST_AU
    formats->addFormat (std::make_unique<juce::AudioUnitPluginFormat>());
   #endif
    const auto description = request.description;
    const auto settings = request.settings;
    formats->createPluginInstanceAsync (description, settings.rate, settings.block,
        [state = shared, formats, job = std::move (request)]
        (std::unique_ptr<juce::AudioPluginInstance> plugin, const juce::String& error) mutable
        {
            if (job.cancelled->load()) return;
            if (plugin != nullptr)
            {
                plugin->setNonRealtime (job.settings.offline);
                plugin->enableAllBuses();
                plugin->prepareToPlay (job.settings.rate, job.settings.block);
                if (job.state.getSize() > 0)
                    plugin->setStateInformation (job.state.getData(), static_cast<int> (job.state.getSize()));
            }
            deliver (state, { std::move (job), std::move (plugin), error });
        });
}

std::vector<PluginLoader::Result> PluginLoader::takeCompleted()
{
    const std::lock_guard<std::mutex> lock (shared->mutex);
    std::vector<Result> result;
    result.swap (shared->completed);
    return result;
}
}
