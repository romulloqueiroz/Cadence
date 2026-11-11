#include <JuceHeader.h>
#include <iostream>
#include <unistd.h>

extern "C" int cadenceCaptureWindowId(void*);

namespace {
constexpr double rate = 48000.0;
constexpr int block = 256;
constexpr int frames = 480000;

struct Playhead : juce::AudioPlayHead {
    int64_t frame = 0;
    juce::Optional<PositionInfo> getPosition() const override {
        PositionInfo position;
        position.setTimeInSamples(frame);
        position.setTimeInSeconds(double(frame) / rate);
        position.setBpm(120.0);
        position.setPpqPosition(double(frame) / rate * 2.0);
        position.setTimeSignature(TimeSignature {4, 4});
        position.setIsPlaying(true);
        return position;
    }
};

class CaptureWindow : public juce::DocumentWindow {
public:
    CaptureWindow(juce::AudioProcessorEditor* editor, std::function<void()> capture, std::function<void()> cancel)
        : DocumentWindow("Kontakt reference: load Piano Uno - Concert, then Capture", juce::Colours::black, closeButton), onCancel(std::move(cancel)) {
        auto content = new juce::Component;
        // Some native views report a temporary 10x10 size until attached.
        // Give the view a usable frame before its asynchronous initialization.
        const auto width = juce::jmax(1100, editor->getWidth());
        const auto height = juce::jmax(760, editor->getHeight());
        content->setSize(width, height+44);
        content->addAndMakeVisible(editor);
        editor->setBounds(0,44,width,height);
        button = std::make_unique<juce::TextButton>("Capture loaded Piano Uno - Concert (note 60, velocity 100)");
        button->onClick = std::move(capture);
        content->addAndMakeVisible(*button);
        button->setBounds(0,0,width,40);
        ownedEditor.reset(editor);
        setUsingNativeTitleBar(true);
        setContentOwned(content,true);
        centreWithSize(content->getWidth(),content->getHeight());
        juce::Process::makeForegroundProcess();
        setVisible(true);
        toFront(true);
    }
    void closeButtonPressed() override { onCancel(); }
private:
    std::function<void()> onCancel;
    std::unique_ptr<juce::AudioProcessorEditor> ownedEditor;
    std::unique_ptr<juce::TextButton> button;
};

class Capture : public juce::JUCEApplication, private juce::Timer {
public:
    bool finished = false;
    int exitCode = 0;
    const juce::String getApplicationName() override { return "Cadence Kontakt Capture"; }
    const juce::String getApplicationVersion() override { return "1"; }
    void initialise(const juce::String& command) override {
        // Let the application's native launch/menu lifecycle finish before
        // creating the third-party GUI and its dispatch services.
        juce::MessageManager::callAsync([this, command] { startSession(command); });
    }
    void startSession(const juce::String& command) {
        juce::File(CADENCE_CAPTURE_BUILD_ROOT).getChildFile("kontakt-capture-bootstrap.log").replaceWithText("initialise: " + command);
        auto args = juce::StringArray::fromTokens(command, true);
        args.removeEmptyStrings();
        if (!args.contains("--output")) {
            // A real app bundle can be selected by the computer-control bridge.
            // Its pending request is data, rather than a shell command.
            const auto requestFile = juce::File(CADENCE_CAPTURE_BUILD_ROOT)
                .getChildFile("kontakt-capture-request.json");
            const auto request = juce::JSON::parse(requestFile.loadFileAsString());
            if (auto object = request.getDynamicObject()) {
                args.add("--output"); args.add(object->getProperty("output").toString().quoted());
                if (object->hasProperty("state")) { args.add("--state"); args.add(object->getProperty("state").toString().quoted()); }
                if (object->hasProperty("component")) { args.add("--component"); args.add(object->getProperty("component").toString().quoted()); }
                if (object->hasProperty("description")) { args.add("--description"); args.add(object->getProperty("description").toString().quoted()); }
                if (bool(object->getProperty("interactive"))) args.add("--interactive");
            }
        }
        juce::String statePath, componentPath, descriptionPath;
        bool interactive = false, scanOnly = false;
        allowPreroll = args.contains("--allow-preroll");
        for (int i = 0; i < args.size(); ++i) {
            auto a = args[i].unquoted();
            if (a == "--output" && i+1 < args.size()) output = juce::File(args[++i].unquoted());
            else if (a == "--state" && i+1 < args.size()) statePath = args[++i].unquoted();
            else if (a == "--component" && i+1 < args.size()) componentPath = args[++i].unquoted();
            else if (a == "--description" && i+1 < args.size()) descriptionPath = args[++i].unquoted();
            else if (a == "--scan") scanOnly = true;
            else if (a == "--interactive") interactive = true;
        }
        if (output == juce::File()) { fail("--output directory required"); return; }
        if (!output.createDirectory()) { fail("cannot create output directory"); return; }
        output.getChildFile("phase.txt").replaceWithText("discovering native plugin");
        format = std::make_unique<juce::VST3PluginFormat>();
        juce::OwnedArray<juce::PluginDescription> descriptions;
        if (scanOnly) {
            format->findAllTypesForFile(descriptions, "/Library/Audio/Plug-Ins/VST3/Kontakt 8.vst3");
            if (descriptions.isEmpty()) { fail("Kontakt VST3 discovery failed"); return; }
            if (!descriptions[0]->createXml()->writeTo(output.getChildFile("plugin-description.xml"))) { fail("cannot save plugin description"); return; }
            std::cout << "native_scan_saved=" << descriptions[0]->name << std::endl;
            quit();
            return;
        }
        auto descriptionXml = juce::XmlDocument::parse(juce::File(descriptionPath));
        auto description = new juce::PluginDescription;
        descriptions.add(description);
        if (!descriptionXml || !description->loadFromXml(*descriptionXml)) { fail("valid --description from a separate --scan process required"); return; }
        juce::String error;
        plugin = format->createInstanceFromDescription(*descriptions[0], rate, block, error);
        if (!plugin) { fail("Kontakt instantiate: " + error); return; }
        juce::DynamicObject::Ptr metadata = new juce::DynamicObject;
        metadata->setProperty("plugin_name", plugin->getName());
        metadata->setProperty("plugin_version", descriptions[0]->version);
        metadata->setProperty("sample_rate", rate);
        metadata->setProperty("block_size", block);
        metadata->setProperty("frames", frames);
        metadata->setProperty("note", 60);
        metadata->setProperty("velocity", 100);
        metadata->setProperty("note_on_frame", 4800);
        metadata->setProperty("repeat_mode", "same native instance after reset; no state reload");
        metadata->setProperty("preview_drain_frames", frames);
        metadata->setProperty("allow_nonzero_preroll", allowPreroll);
        metadata->setProperty("note_off_frame", 292800); // 0.1s start + 6s duration
        output.getChildFile("settings.json").replaceWithText(juce::JSON::toString(juce::var(metadata.get())));
        output.getChildFile("phase.txt").replaceWithText("native instantiated; building editor");
        std::cout << "loaded=" << plugin->getName() << " version=" << descriptions[0]->version << std::endl;
        if (statePath.isNotEmpty()) {
            juce::MemoryBlock state;
            if (!juce::File(statePath).loadFileAsData(state)) { fail("cannot read saved native state"); return; }
            pendingState = state;
        } else if (componentPath.isNotEmpty()) {
            juce::MemoryBlock state, envelope;
            if (!juce::File(componentPath).loadFileAsData(state)) { fail("cannot read component-state candidate"); return; }
            juce::XmlElement xml("VST3PluginState");
            xml.createNewChildElement("IComponent")->addTextElement(state.toBase64Encoding());
            juce::AudioProcessor::copyXmlToBinary(xml, envelope);
            pendingState = envelope;
        }
        {
            // Initialize and attach Kontakt's GUI dispatcher before restoration.
            auto editor = plugin->createEditorIfNeeded();
            if (!editor) { fail("Kontakt has no editor"); return; }
            window = std::make_unique<CaptureWindow>(editor, [this] {
                output.getChildFile("operator-confirmation.txt").replaceWithText("Operator confirms the native editor has loaded Piano Uno - Concert.nki before capture.");
                output.getChildFile("render.flag").replaceWithText("capture");
            }, [this] { fail("operator cancelled native capture"); });
            // Explicit render.flag lets the operator keep the native editor open.
            std::cout << "Load the exact NKI in the native editor; create " << output.getChildFile("render.flag").getFullPathName() << " when ready." << std::endl;
            interactiveMode = interactive;
        }
        output.getChildFile("phase.txt").replaceWithText("editor attached; servicing GUI; waiting for capture/restore");
        juce::DynamicObject::Ptr windowInfo = new juce::DynamicObject;
        windowInfo->setProperty("pid", int(getpid()));
        windowInfo->setProperty("window_id", cadenceCaptureWindowId(window->getWindowHandle()));
        windowInfo->setProperty("visible", window->isVisible());
        windowInfo->setProperty("showing", window->isShowing());
        windowInfo->setProperty("bounds", window->getScreenBounds().toString());
        output.getChildFile("native-window.json").replaceWithText(juce::JSON::toString(juce::var(windowInfo.get())));
        std::cout << "editor_attached=" << (window != nullptr) << std::endl;
        startTimer(100);
    }
    void shutdown() override {
        stopTimer();
        window.reset();
        if (plugin) plugin->releaseResources();
        plugin.reset();
        format.reset();
    }
    void systemRequestedQuit() override { fail("operator cancelled native capture"); }
    void quit() { finished = true; stopTimer(); setApplicationReturnValue(exitCode); juce::JUCEApplication::quit(); }
private:
    void fail(const juce::String& message) {
        std::cerr << message << std::endl;
        if (output != juce::File()) output.getChildFile("error.txt").replaceWithText(message);
        exitCode = 1;
        quit();
    }
    void timerCallback() override {
        // Service native GUI setup before a synchronous native state call.
        if (++ticks < 20) return;
        if (pendingState.getSize() != 0) {
            // Native restoration may run a nested message loop. Consume the
            // request and stop this timer before entering the plugin so a
            // nested dispatch cannot recursively restore the same state.
            stopTimer();
            juce::MemoryBlock restoring;
            restoring.swapWith(pendingState);
            output.getChildFile("phase.txt").replaceWithText("restoring native state (single request)");
            std::cout << "restoring_native_state_after_editor_setup" << std::endl;
            plugin->setStateInformation(restoring.getData(), int(restoring.getSize()));
            ticks = 0;
            output.getChildFile("phase.txt").replaceWithText("native state restored; servicing preset setup");
            std::cout << "native_state_restore_returned" << std::endl;
            startTimer(100);
            return;
        }
        if (interactiveMode && !output.getChildFile("render.flag").existsAsFile()) return;
        stopTimer();
        if (window) {
            auto image = window->createComponentSnapshot(window->getLocalBounds());
            if (auto stream = output.getChildFile("native-editor.png").createOutputStream()) {
                juce::PNGImageFormat png;
                png.writeImageToStream(image, *stream);
            }
        }
        output.getChildFile("phase.txt").replaceWithText("saving native state");
        juce::MemoryBlock state;
        plugin->getStateInformation(state);
        if (!output.getChildFile("native-state.bin").replaceWithData(state.getData(), state.getSize())) { fail("cannot save native state"); return; }
        if (auto xml = juce::AudioProcessor::getXmlFromBinary(state.getData(), int(state.getSize()))) {
            if (auto component = xml->getChildByName("IComponent")) {
                juce::MemoryBlock bytes;
                if (bytes.fromBase64Encoding(component->getAllSubText())) output.getChildFile("component-state.bin").replaceWithData(bytes.getData(), bytes.getSize());
            }
        }
        auto layout = plugin->getBusesLayout();
        for (auto& channel : layout.inputBuses) channel = juce::AudioChannelSet::disabled();
        for (int bus = 0; bus < layout.outputBuses.size(); ++bus) layout.outputBuses.getReference(bus) = bus == 0 ? juce::AudioChannelSet::stereo() : juce::AudioChannelSet::disabled();
        if (!plugin->setBusesLayout(layout)) { fail("Kontakt refused stereo main bus"); return; }
        plugin->setNonRealtime(true);
        plugin->setRateAndBufferSizeDetails(rate, block);
        plugin->prepareToPlay(rate, block);
        juce::DynamicObject::Ptr configuration = new juce::DynamicObject;
        configuration->setProperty("reported_latency_samples", plugin->getLatencySamples());
        configuration->setProperty("reported_tail_seconds", plugin->getTailLengthSeconds());
        configuration->setProperty("output_channels", plugin->getTotalNumOutputChannels());
        configuration->setProperty("preview_drain_frames", frames);
        configuration->setProperty("repeat_mode", "same instance after reset; no reload");
        output.getChildFile("audio-configuration.json").replaceWithText(juce::JSON::toString(juce::var(configuration.get())));
        Playhead playhead;
        plugin->setPlayHead(&playhead);
        // Selecting a factory preset can enqueue browser preview audio. Drain
        // it with silent preroll before either measured take. Note/sound-off
        // messages clear any audition-held notes without changing controllers.
        {
            juce::AudioBuffer<float> warmup(juce::jmax(2, plugin->getTotalNumOutputChannels()), block);
            for (int done = 0; done < frames; done += block) {
                warmup.clear();
                juce::MidiBuffer midi;
                if (done == 0) for (int channel = 1; channel <= 16; ++channel) {
                    midi.addEvent(juce::MidiMessage::controllerEvent(channel,120,0),0);
                    midi.addEvent(juce::MidiMessage::controllerEvent(channel,123,0),0);
                }
                playhead.frame = done-frames;
                plugin->processBlock(warmup,midi);
            }
        }
        output.getChildFile("phase.txt").replaceWithText("rendering two native passes");
        for (int pass = 0; pass < 2; ++pass) {
        plugin->reset();
        juce::AudioBuffer<float> audio(juce::jmax(2, plugin->getTotalNumOutputChannels()), block);
        juce::AudioBuffer<float> result(2, frames);
        for (int done = 0; done < frames; done += block) {
            auto count = juce::jmin(block, frames-done);
            audio.clear();
            juce::MidiBuffer midi;
            for (auto event : { std::pair<int, juce::MidiMessage>(4800, juce::MidiMessage::noteOn(1,60,juce::uint8(100))),
                                std::pair<int, juce::MidiMessage>(292800, juce::MidiMessage::noteOff(1,60)) })
                if (event.first >= done && event.first < done+count) midi.addEvent(event.second, event.first-done);
            playhead.frame = done;
            juce::AudioBuffer<float> portion(audio.getArrayOfWritePointers(), audio.getNumChannels(), 0, count);
            plugin->processBlock(portion, midi);
            for (int ch = 0; ch < 2; ++ch) result.copyFrom(ch,done,audio,ch,0,count);
        }
        auto file = output.getChildFile(pass == 0 ? "native.wav" : "native-repeat.wav");
        file.deleteFile();
        juce::WavAudioFormat wav;
        std::unique_ptr<juce::OutputStream> stream = file.createOutputStream();
        auto options = juce::AudioFormatWriterOptions().withSampleRate(rate).withNumChannels(2)
            .withBitsPerSample(32).withSampleFormat(juce::AudioFormatWriterOptions::SampleFormat::floatingPoint);
        auto writer = wav.createWriterFor(stream, options);
        if (!writer || !writer->writeFromAudioSampleBuffer(result,0,frames)) { fail("cannot save native WAV"); return; }
        writer.reset();
        auto peak = result.getMagnitude(0,frames);
        auto prerollPeak = result.getMagnitude(0,4800);
        std::cout << "pass=" << pass << " frames=" << frames << " peak=" << peak << " preroll_peak=" << prerollPeak << " state_bytes=" << state.getSize() << std::endl;
        if (prerollPeak > 1e-6f && !allowPreroll) { plugin->setPlayHead(nullptr); fail("pre-note audio remains: native take rejected"); return; }
        if (peak < 1e-6f) { plugin->setPlayHead(nullptr); fail("silent render: preset loading is not established"); return; }
        }
        plugin->setPlayHead(nullptr);
        output.getChildFile("phase.txt").replaceWithText("native capture finished");
        quit();
    }
    juce::File output;
    juce::MemoryBlock pendingState;
    std::unique_ptr<juce::VST3PluginFormat> format;
    std::unique_ptr<juce::AudioPluginInstance> plugin;
    std::unique_ptr<juce::DocumentWindow> window;
    bool interactiveMode = false;
    bool allowPreroll = false;
    int ticks = 0;
};
}
START_JUCE_APPLICATION(Capture)
