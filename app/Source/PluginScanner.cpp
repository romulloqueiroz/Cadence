#include "PluginScanner.h"
#include <cerrno>
#include <csignal>
#include <fcntl.h>
#include <spawn.h>
#include <sys/file.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;

namespace cadence
{
namespace
{
// posix_spawn avoids running fork-time C++ allocations inside a multithreaded
// DAW. Keep the wait status: ChildProcess does not preserve signal exits on macOS.
class ScanProcess
{
public:
    ~ScanProcess() { stop(); }

    bool start (const juce::StringArray& arguments)
    {
        std::vector<char*> argv;
        for (const auto& argument : arguments)
            argv.push_back (const_cast<char*> (argument.toRawUTF8()));
        argv.push_back (nullptr);
        posix_spawn_file_actions_t actions;
        posix_spawnattr_t attributes;
        if (posix_spawn_file_actions_init (&actions) != 0)
            return false;
        const juce::ScopeGuard cleanupActions { [&] { posix_spawn_file_actions_destroy (&actions); } };
        if (posix_spawnattr_init (&attributes) != 0)
            return false;
        const juce::ScopeGuard cleanupAttributes { [&] { posix_spawnattr_destroy (&attributes); } };
        if (posix_spawn_file_actions_addopen (&actions, STDIN_FILENO, "/dev/null", O_RDONLY, 0) != 0
            || posix_spawn_file_actions_addopen (&actions, STDOUT_FILENO, "/dev/null", O_WRONLY, 0) != 0
            || posix_spawn_file_actions_addopen (&actions, STDERR_FILENO, "/dev/null", O_WRONLY, 0) != 0
            || posix_spawnattr_setpgroup (&attributes, 0) != 0
            || posix_spawnattr_setflags (&attributes, POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT) != 0)
            return false;
        running = posix_spawn (&pid, argv.front(), &actions, &attributes, argv.data(), environ) == 0;
        return running;
    }

    bool isRunning()
    {
        if (running)
        {
            pid_t result;
            do { result = waitpid (pid, &status, WNOHANG); } while (result < 0 && errno == EINTR);
            if (result != 0)
            {
                running = false;
                success = result == pid && WIFEXITED (status) && WEXITSTATUS (status) == 0;
            }
        }
        return running;
    }

    void stop()
    {
        if (! running)
            return;
        // The helper and any children it created have a dedicated group.
        ::kill (-pid, SIGKILL);
        while (waitpid (pid, &status, 0) < 0 && errno == EINTR) {}
        running = false;
        success = false;
    }

    bool succeeded() const { return success; }

private:
    pid_t pid = -1;
    int status = 0;
    bool running = false;
    bool success = false;
};

class ScanLock
{
public:
    explicit ScanLock (const juce::File& file)
    {
        descriptor = ::open (file.getFullPathName().toRawUTF8(), O_CREAT | O_RDWR | O_CLOEXEC, 0600);
        locked = descriptor >= 0 && ::flock (descriptor, LOCK_EX | LOCK_NB) == 0;
    }
    ~ScanLock() { if (descriptor >= 0) ::close (descriptor); }
    bool acquired() const { return locked; }
private:
    int descriptor = -1;
    bool locked = false;
};

bool saveSkipped (const juce::File& file, const std::vector<SkippedPlugin>& skipped)
{
    juce::XmlElement xml ("CADENCE_SKIPPED");
    for (const auto& item : skipped)
    {
        auto* entry = xml.createNewChildElement ("PLUGIN");
        entry->setAttribute ("format", item.candidate.format);
        entry->setAttribute ("path", item.candidate.path);
        entry->setAttribute ("reason", item.reason);
    }
    juce::TemporaryFile temporary (file);
    return xml.writeTo (temporary.getFile()) && temporary.overwriteTargetFileWithTemporary();
}

bool loadSkipped (const juce::File& file, std::vector<SkippedPlugin>& result)
{
    if (! file.existsAsFile())
        return true;
    auto xml = juce::XmlDocument::parse (file);
    if (xml == nullptr || ! xml->hasTagName ("CADENCE_SKIPPED"))
        return false;
    for (auto* entry : xml->getChildIterator())
        if (entry->hasTagName ("PLUGIN"))
            result.push_back ({ { entry->getStringAttribute ("format"),
                                  entry->getStringAttribute ("path") },
                                entry->getStringAttribute ("reason") });
    return true;
}

void collect (const juce::File& directory, std::vector<ScanCandidate>& candidates,
              juce::StringArray& visited)
{
    if (! directory.isDirectory() || visited.contains (directory.getFullPathName()))
        return;
    visited.add (directory.getFullPathName());

    for (const auto& entry : juce::RangedDirectoryIterator (directory, false, "*", juce::File::findDirectories))
    {
        const auto file = entry.getFile();
        if (file.isSymbolicLink())
            continue;
        if (file.hasFileExtension ("vst3;component;clap"))
        {
            // Never scan Cadence into itself.
            if (file.getFileNameWithoutExtension().equalsIgnoreCase ("Cadence"))
                continue;
            candidates.push_back ({ file.hasFileExtension ("vst3") ? "VST3" : file.hasFileExtension ("clap") ? "CLAP" : "AudioUnit",
                                    file.getFullPathName() });
        }
        else
            collect (file, candidates, visited);
    }
}
}

PluginScanner::PluginScanner (Options value, std::function<void()> callback)
    : options (std::move (value)), changed (std::move (callback))
{
    loadSkipped (options.directory.getChildFile ("skipped-plugins.xml"), state.skipped);
}

PluginScanner::~PluginScanner()
{
    cancel();
    if (thread.joinable())
        thread.join();
}

ScanSnapshot PluginScanner::snapshot() const
{
    const std::lock_guard<std::mutex> lock (mutex);
    return state;
}

bool PluginScanner::isRunning() const
{
    const std::lock_guard<std::mutex> lock (mutex);
    return state.running;
}

bool PluginScanner::canSkip() const
{
    const std::lock_guard<std::mutex> lock (mutex);
    return state.current != 0;
}

size_t PluginScanner::skippedCount() const
{
    const std::lock_guard<std::mutex> lock (mutex);
    return state.skipped.size();
}

void PluginScanner::publish (const std::function<void (ScanSnapshot&)>& update)
{
    {
        const std::lock_guard<std::mutex> lock (mutex);
        update (state);
    }
    if (changed)
        changed();
}

bool PluginScanner::start (std::vector<ScanCandidate> candidates,
                           std::vector<juce::PluginDescription> existing, bool retrySkipped)
{
    if (isRunning())
        return false;
    if (thread.joinable())
        thread.join();
    cancelled.store (false);
    skippedToken.store (0);
    publish ([&] (auto& s)
    {
        s.plugins = std::move (existing);
        s.running = true;
        s.current = 0;
        s.status = "Preparing scan...";
    });
    thread = std::thread ([this, requests = std::move (candidates), retrySkipped] () mutable
    {
        run (std::move (requests), retrySkipped);
    });
    return true;
}

void PluginScanner::cancel() { cancelled.store (true); }

void PluginScanner::skipCurrent()
{
    const std::lock_guard<std::mutex> lock (mutex);
    skippedToken.store (state.current);
}

void PluginScanner::run (std::vector<ScanCandidate> candidates, bool retrySkipped)
{
    auto finish = [&] (const juce::String& text)
    {
        publish ([&] (auto& s) { s.running = false; s.current = 0; s.status = text; });
    };

    if (! options.helper.existsAsFile())
        return finish ("Scanner helper is missing - rebuild or reinstall Cadence");
    if (options.directory.createDirectory().failed())
        return finish ("Cannot create scan settings directory");

    // flock also excludes other instances in the same DAW process; POSIX
    // record locks are process-owned and would let those scans overlap.
    ScanLock scanLock (options.directory.getChildFile ("scan.lock"));
    if (! scanLock.acquired())
        return finish ("Another Cadence instance is scanning, or scan settings are locked");

    const auto skippedFile = options.directory.getChildFile ("skipped-plugins.xml");
    std::vector<SkippedPlugin> skipped;
    if (! loadSkipped (skippedFile, skipped))
        return finish ("Cannot read skipped-plugins.xml - scan stopped");
    publish ([&] (auto& s) { s.skipped = skipped; });

    int omitted = 0;
    int failed = 0;
    int completed = 0;

    for (const auto& candidate : candidates)
    {
        if (cancelled.load())
            break;
        const auto beforeProbe = skipped;
        auto previous = std::find_if (skipped.begin(), skipped.end(), [&] (const auto& item)
        {
            return item.candidate.key() == candidate.key();
        });
        if (previous != skipped.end())
        {
            if (! retrySkipped)
            {
                ++omitted;
                continue;
            }
            skipped.erase (previous);
        }

        // Journal before loading: a host crash/reboot also leaves this entry
        // skipped on the next scan. Success is the only automatic removal.
        skipped.push_back ({ candidate, "Interrupted scan" });
        if (! saveSkipped (skippedFile, skipped))
            return finish ("Cannot save scan recovery information - scan stopped");

        const auto token = ++nextToken;
        publish ([&] (auto& s)
        {
            s.current = token;
            s.status = "Scanning " + juce::String (++completed + omitted) + "/"
                + juce::String (candidates.size()) + " - " + candidate.name();
        });

        juce::TemporaryFile output (".xml");
        ScanProcess child;
        const juce::StringArray arguments { options.helper.getFullPathName(), candidate.format,
                                            candidate.path, output.getFile().getFullPathName() };
        if (! child.start (arguments))
        {
            skipped = beforeProbe;
            saveSkipped (skippedFile, skipped);
            return finish ("Could not launch scanner helper");
        }

        const auto started = juce::Time::getMillisecondCounterHiRes();
        juce::String failure;
        while (child.isRunning())
        {
            if (cancelled.load())
                failure = "Cancelled during scan";
            else if (skippedToken.load() == token)
                failure = "Skipped by user";
            else if (juce::Time::getMillisecondCounterHiRes() - started >= options.timeoutMs)
                failure = "Timed out";
            if (failure.isNotEmpty())
            {
                child.stop();
                break;
            }
            juce::Thread::sleep (20);
        }

        auto xml = output.getFile().getSize() <= 4 * 1024 * 1024
                       ? juce::XmlDocument::parse (output.getFile()) : nullptr;
        std::vector<juce::PluginDescription> descriptions;
        if (failure.isEmpty())
        {
            if (! child.succeeded() || xml == nullptr || ! xml->hasTagName ("CADENCE_SCAN"))
                failure = "Scanner crashed or returned no valid result";
            else if (xml->getStringAttribute ("error").isNotEmpty())
                failure = xml->getStringAttribute ("error");
            else
            {
                for (auto* element : xml->getChildIterator())
                {
                    juce::PluginDescription description;
                    if (description.loadFromXml (*element)
                        && description.pluginFormatName == candidate.format
                        && description.name.isNotEmpty())
                        descriptions.push_back (description);
                }
                if (descriptions.empty())
                    failure = "No compatible plugins found";
            }
        }

        if (failure.isEmpty())
            skipped.pop_back();
        else
        {
            skipped.back().reason = failure;
            ++failed;
        }
        if (! saveSkipped (skippedFile, skipped))
            return finish ("Cannot save scan results - scan stopped");

        publish ([&] (auto& s)
        {
            s.current = 0;
            s.skipped = skipped;
            if (failure.isNotEmpty())
                return;
            for (const auto& description : descriptions)
            {
                const auto id = description.createIdentifierString();
                s.plugins.erase (std::remove_if (s.plugins.begin(), s.plugins.end(), [&] (const auto& old)
                {
                    return old.createIdentifierString() == id;
                }), s.plugins.end());
                s.plugins.push_back (description);
            }
            std::sort (s.plugins.begin(), s.plugins.end(), [] (const auto& a, const auto& b)
            {
                return a.name.compareIgnoreCase (b.name) < 0;
            });
            ++s.catalogueVersion;
        });
    }

    const auto count = juce::String (snapshot().plugins.size());
    finish ((cancelled.load() ? "Scan cancelled - " : "Scan complete - ") + count + " plugins, "
            + juce::String (omitted + failed) + " skipped");
}

std::vector<ScanCandidate> PluginScanner::discoverCandidates()
{
    std::vector<ScanCandidate> result;
    juce::StringArray visited;
    const auto home = juce::File::getSpecialLocation (juce::File::userHomeDirectory);
    for (const auto& root : { juce::File ("/Library/Audio/Plug-Ins"), home.getChildFile ("Library/Audio/Plug-Ins") })
    {
        collect (root.getChildFile ("VST3"), result, visited);
        collect (root.getChildFile ("Components"), result, visited);
        collect (root.getChildFile ("CLAP"), result, visited);
    }
    result.push_back ({ "Protocol", "builtin:sine" });
    const auto contents = juce::File::getSpecialLocation (juce::File::currentExecutableFile).getParentDirectory().getParentDirectory();
    const auto gain = contents.getChildFile ("Resources/ProtocolPlugins/gain.dylib");
    if (gain.existsAsFile()) result.push_back ({ "Protocol", "builtin:gain" });
    auto store = juce::SystemStats::getEnvironmentVariable ("PLATFORM_HOME", "");
    const auto storeDirectory = store.isNotEmpty() ? juce::File (store).getChildFile ("plugins")
        : home.getChildFile (".local/share/platform/plugins");
    for (const auto& manifest : storeDirectory.findChildFiles (juce::File::findFiles, true, "manifest.json"))
        result.push_back ({ "Protocol", manifest.getFullPathName() });
    std::sort (result.begin(), result.end(), [] (const auto& a, const auto& b) { return a.key() < b.key(); });
    return result;
}

juce::File PluginScanner::bundledHelper()
{
    return juce::File::getSpecialLocation (juce::File::currentExecutableFile)
        .getParentDirectory().getSiblingFile ("Helpers")
        .getChildFile ("Cadence Scanner.app/Contents/MacOS/Cadence Scanner");
}
}
