#include <JuceHeader.h>
#include "PluginProcessor.h"

int main()
{
    juce::ScopedJuceInitialiser_GUI init;

    CadenceAudioProcessor processor;

    std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
    editor->setSize (1240, 780);
    const auto image = editor->createComponentSnapshot (editor->getLocalBounds(), true);

    auto file = juce::File ("/tmp/cadence-preview.png");
    file.deleteFile();
    if (auto stream = file.createOutputStream())
    {
        juce::PNGImageFormat png;
        png.writeImageToStream (image, *stream);
    }
    return 0;
}
