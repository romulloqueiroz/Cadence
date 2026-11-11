#include "Theme.h"

namespace cadence
{
CadenceLookAndFeel::CadenceLookAndFeel()
{
    setColour (juce::ResizableWindow::backgroundColourId, colours::background);
    setColour (juce::DocumentWindow::textColourId, colours::text);
    setColour (juce::Label::textColourId, colours::text);
    setColour (juce::TextButton::textColourOffId, colours::text);
    setColour (juce::TextButton::textColourOnId, colours::background);
    setColour (juce::TextButton::buttonColourId, colours::surfaceAlt);
    setColour (juce::TextButton::buttonOnColourId, colours::accent);
    setColour (juce::ListBox::backgroundColourId, colours::surface);
    setColour (juce::ListBox::outlineColourId, colours::border);
    setColour (juce::ListBox::textColourId, colours::text);
    setColour (juce::ScrollBar::thumbColourId, colours::border.brighter (0.3f));
    setColour (juce::ScrollBar::backgroundColourId, colours::surface);
    setColour (juce::ComboBox::backgroundColourId, colours::surfaceAlt);
    setColour (juce::ComboBox::outlineColourId, colours::border);
    setColour (juce::ComboBox::textColourId, colours::text);
    setColour (juce::PopupMenu::backgroundColourId, colours::surfaceAlt);
    setColour (juce::PopupMenu::textColourId, colours::text);
    setColour (juce::PopupMenu::highlightedBackgroundColourId, colours::accent.withAlpha (0.18f));
    setColour (juce::TextEditor::backgroundColourId, colours::surfaceAlt);
    setColour (juce::TextEditor::textColourId, colours::text);
    setColour (juce::TextEditor::outlineColourId, colours::border);
    setColour (juce::Slider::thumbColourId, colours::accent);
    setColour (juce::Slider::trackColourId, colours::accent);
    setColour (juce::Slider::backgroundColourId, colours::border);
    setColour (juce::Slider::textBoxTextColourId, colours::text);
    setColour (juce::Slider::textBoxBackgroundColourId, colours::surfaceAlt);
    setColour (juce::Slider::textBoxOutlineColourId, colours::border);
    setColour (juce::ToggleButton::tickColourId, colours::accent);
}

void CadenceLookAndFeel::drawButtonBackground (juce::Graphics& g, juce::Button& button,
                                               const juce::Colour&,
                                               bool shouldDrawButtonAsHighlighted,
                                               bool shouldDrawButtonAsDown)
{
    auto bounds = button.getLocalBounds().toFloat().reduced (0.5f);
    const auto radius = 4.0f;
    auto fill = button.getToggleState() ? colours::accent : colours::surfaceAlt;

    if (shouldDrawButtonAsDown)
        fill = fill.darker (0.12f);
    else if (shouldDrawButtonAsHighlighted)
        fill = fill.brighter (0.08f);

    g.setColour (fill);
    g.fillRoundedRectangle (bounds, radius);

    g.setColour (button.getToggleState() ? colours::accent : colours::border);
    g.drawRoundedRectangle (bounds, radius, 1.0f);
}

void CadenceLookAndFeel::drawButtonText (juce::Graphics& g, juce::TextButton& button,
                                         bool, bool)
{
    g.setFont (getTextButtonFont (button, button.getHeight()));
    g.setColour (button.getToggleState() ? colours::background : colours::text);
    g.drawFittedText (button.getButtonText(), button.getLocalBounds().reduced (6, 0),
                      juce::Justification::centred, 1);
}

juce::Font CadenceLookAndFeel::getTextButtonFont (juce::TextButton&, int buttonHeight)
{
    return juce::Font (juce::FontOptions (juce::jmin (13.0f, static_cast<float> (buttonHeight) * 0.55f),
                                          juce::Font::bold));
}
}
