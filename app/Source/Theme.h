#pragma once

#include <juce_gui_basics/juce_gui_basics.h>

namespace cadence
{
namespace colours
{
inline const juce::Colour background { 0xff0e0f12 };
inline const juce::Colour surface    { 0xff15171c };
inline const juce::Colour surfaceAlt { 0xff1a1d24 };
inline const juce::Colour border     { 0xff262a33 };
inline const juce::Colour text       { 0xffd9dee7 };
inline const juce::Colour textDim    { 0xff7e8796 };
inline const juce::Colour accent     { 0xffb6f09c };
inline const juce::Colour accentAlt  { 0xff7ce7e1 };
inline const juce::Colour danger     { 0xfff27c7c };
}

class CadenceLookAndFeel : public juce::LookAndFeel_V4
{
public:
    CadenceLookAndFeel();

    void drawButtonBackground (juce::Graphics&, juce::Button&, const juce::Colour&,
                               bool shouldDrawButtonAsHighlighted,
                               bool shouldDrawButtonAsDown) override;
    void drawButtonText (juce::Graphics&, juce::TextButton&,
                         bool shouldDrawButtonAsHighlighted,
                         bool shouldDrawButtonAsDown) override;
    juce::Font getTextButtonFont (juce::TextButton&, int buttonHeight) override;
};
}
