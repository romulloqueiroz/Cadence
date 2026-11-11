#include <juce_audio_processors/juce_audio_processors.h>
#include "RustPluginProcessor.h"

class ScanWorker final : public juce::JUCEApplication
{
public:
    const juce::String getApplicationName() override { return "Cadence Scanner"; }
    const juce::String getApplicationVersion() override { return "1.0"; }
    bool moreThanOneInstanceAllowed() override { return true; }

    void initialise (const juce::String&) override
    {
        // Run after the application's message loop has started. Some plugin
        // factories need Cocoa/message-thread services even for discovery.
        juce::MessageManager::callAsync ([this] { probe(); });
    }
    void shutdown() override {}

private:
    void probe()
    {
        const auto args = getCommandLineParameterArray();
        if (args.size() != 3 || ! juce::File::isAbsolutePath (args[2]))
        {
            setApplicationReturnValue (2);
            quit();
            return;
        }

        juce::XmlElement result ("CADENCE_SCAN");
        std::unique_ptr<juce::AudioPluginFormat> format;
        if (args[0] == "VST3")
            format = std::make_unique<juce::VST3PluginFormat>();
       #if JUCE_MAC && JUCE_PLUGINHOST_AU
        else if (args[0] == "AudioUnit")
            format = std::make_unique<juce::AudioUnitPluginFormat>();
       #endif
        if (cadence::RustPluginProcessor::supports (args[0]))
        {
            juce::String error;
            auto descriptions = cadence::RustPluginProcessor::describeAll (args[1], args[0], error);
            if (error.isNotEmpty()) result.setAttribute ("error", error);
            else if (descriptions.empty()) result.setAttribute ("error", "No compatible plugins found");
            else for (auto& description : descriptions)
                result.addChildElement (description.createXml().release());
        }
        else if (format == nullptr)
            result.setAttribute ("error", "Unsupported plugin format");
        else
        {
            juce::OwnedArray<juce::PluginDescription> descriptions;
            format->findAllTypesForFile (descriptions, args[1]);
            for (auto* description : descriptions)
                result.addChildElement (description->createXml().release());
            if (descriptions.isEmpty())
                result.setAttribute ("error", "Plugin could not be loaded or needs activation");
        }

        // Commit only after plugin destruction also completed successfully.
        format.reset();
        juce::TemporaryFile temporary { juce::File (args[2]) };
        if (! result.writeTo (temporary.getFile()) || ! temporary.overwriteTargetFileWithTemporary())
            setApplicationReturnValue (3);
        quit();
    }
};

START_JUCE_APPLICATION (ScanWorker)
