#pragma once

#include <JuceHeader.h>
#include "PluginProcessor.h"
#include "Theme.h"

class LevelMeter : public juce::Component
{
public:
    void setLevel (float newLevel);
    void paint (juce::Graphics&) override;

private:
    float level = 0.0f;
};

class CadenceAudioProcessorEditor : public juce::AudioProcessorEditor,
                                    private juce::Timer
{
public:
    explicit CadenceAudioProcessorEditor (CadenceAudioProcessor&);
    ~CadenceAudioProcessorEditor() override;

    void paint (juce::Graphics&) override;
    void resized() override;
    void clearSlotEditor();

private:
    struct BrowserModel : public juce::ListBoxModel
    {
        explicit BrowserModel (CadenceAudioProcessorEditor& ownerToUse) : owner (ownerToUse) {}

        int getNumRows() override;
        void paintListBoxItem (int row, juce::Graphics&, int width, int height,
                               bool rowIsSelected) override;
        void listBoxItemDoubleClicked (int row, const juce::MouseEvent&) override;

        CadenceAudioProcessorEditor& owner;
    };

    struct ChainModel : public juce::ListBoxModel
    {
        explicit ChainModel (CadenceAudioProcessorEditor& ownerToUse) : owner (ownerToUse) {}

        int getNumRows() override;
        void paintListBoxItem (int row, juce::Graphics&, int width, int height,
                               bool rowIsSelected) override;
        void listBoxItemDoubleClicked (int row, const juce::MouseEvent&) override;
        void selectedRowsChanged (int lastRowSelected) override;

        CadenceAudioProcessorEditor& owner;
    };

    void timerCallback() override;
    void refreshBrowser();
    void refreshChain (int select);
    void refreshAutomation();
    void assignAutomationFromBox (int control);
    void showSlot (int index);
    void addSelected();
    void scan();
    void showSkipped();
    void openSource();
    int selectedChain() const;
    int selectedBrowser() const;

    CadenceAudioProcessor& audioProcessor;
    cadence::CadenceLookAndFeel theme;

    juce::Label title;
    juce::Label subtitle;
    juce::Label status;
    juce::Label libraryLabel;
    juce::Label chainLabel;
    juce::Label inspectorLabel;
    juce::Label automationLabel;
    juce::Label automationHint;
    juce::Label meterLabelLeft;
    juce::Label meterLabelRight;
    juce::Label libraryHint;
    juce::Label chainHint;
    juce::Label inspectorHint;

    juce::TextButton scanButton { "Scan" };
    juce::TextButton openButton { "Open..." };
    juce::TextButton cancelLoadButton { "Cancel loading" };
    juce::TextButton skipButton { "Skip plugin" };
    juce::TextButton skippedButton { "Skipped (0)" };
    juce::TextButton addButton { "Add" };
    juce::TextButton bypassButton { "Bypass" };
    juce::TextButton removeButton { "Remove" };
    juce::TextButton upButton { "Up" };
    juce::TextButton downButton { "Down" };

    juce::ListBox browserList { "Library", nullptr };
    juce::ListBox chainList { "Chain", nullptr };
    BrowserModel browserModel { *this };
    ChainModel chainModel { *this };

    struct AutomationEntry { int slot = 0; juce::String key; };
    std::array<juce::Label, cadence::Automation::numControls> automationNumbers;
    std::array<juce::ComboBox, cadence::Automation::numControls> automationBoxes;
    std::array<juce::TextButton, cadence::Automation::numControls> automationClearButtons;
    std::array<std::vector<AutomationEntry>, cadence::Automation::numControls> automationEntries;

    juce::Viewport inspectorViewport;
    juce::Component inspectorContent;
    struct SlotEditorDeleter
    {
        void operator() (juce::AudioProcessorEditor* editor) const
        {
            if (editor == nullptr) return;
            editor->getAudioProcessor()->editorBeingDeleted (editor);
            delete editor;
        }
    };
    std::unique_ptr<juce::AudioProcessorEditor, SlotEditorDeleter> currentEditor;

    LevelMeter meterLeft;
    LevelMeter meterRight;
    uint32_t lastPluginsVersion = 0;
    uint32_t lastChainVersion = 0;
    std::unique_ptr<juce::FileChooser> sourceChooser;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (CadenceAudioProcessorEditor)
};
