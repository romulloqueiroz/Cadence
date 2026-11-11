#include "PluginEditor.h"

namespace
{
juce::Font uiFont (float height, bool bold = false)
{
    return juce::Font (juce::FontOptions (height, bold ? juce::Font::bold : juce::Font::plain));
}
}

void LevelMeter::setLevel (float newLevel)
{
    level = juce::jlimit (0.0f, 1.0f, newLevel);
    repaint();
}

void LevelMeter::paint (juce::Graphics& g)
{
    auto bounds = getLocalBounds().toFloat().reduced (0.5f);
    g.setColour (cadence::colours::surfaceAlt);
    g.fillRoundedRectangle (bounds, 3.0f);
    g.setColour (cadence::colours::accent);
    g.fillRoundedRectangle (bounds.withWidth (bounds.getWidth() * level), 3.0f);
}

int CadenceAudioProcessorEditor::BrowserModel::getNumRows()
{
    return static_cast<int> (owner.audioProcessor.getPlugins().size());
}

void CadenceAudioProcessorEditor::BrowserModel::paintListBoxItem (int row, juce::Graphics& g,
                                                                  int width, int height,
                                                                  bool rowIsSelected)
{
    const auto& plugins = owner.audioProcessor.getPlugins();
    if (! juce::isPositiveAndBelow (row, static_cast<int> (plugins.size())))
        return;

    if (rowIsSelected)
    {
        g.setColour (cadence::colours::accent.withAlpha (0.12f));
        g.fillRect (0, 0, width, height);
        g.setColour (cadence::colours::accent);
        g.fillRect (0, 0, 2, height);
    }

    const auto& description = plugins[static_cast<size_t> (row)];

    g.setColour (cadence::colours::text);
    g.setFont (uiFont (13.0f, true));
    g.drawText (description.name, 12, 3, width - 20, height / 2,
                juce::Justification::centredLeft, true);

    g.setColour (cadence::colours::textDim);
    g.setFont (uiFont (10.5f));
    g.drawText (description.pluginFormatName + juce::String::fromUTF8 ("  ·  ") + description.manufacturerName,
                12, height / 2 - 1, width - 20, height / 2,
                juce::Justification::centredLeft, true);
}

void CadenceAudioProcessorEditor::BrowserModel::listBoxItemDoubleClicked (int, const juce::MouseEvent&)
{
    owner.addSelected();
}

int CadenceAudioProcessorEditor::ChainModel::getNumRows()
{
    return owner.audioProcessor.getNumSlots();
}

void CadenceAudioProcessorEditor::ChainModel::paintListBoxItem (int row, juce::Graphics& g,
                                                                int width, int height,
                                                                bool rowIsSelected)
{
    if (! juce::isPositiveAndBelow (row, owner.audioProcessor.getNumSlots()))
        return;

    const auto bypassed = owner.audioProcessor.isSlotBypassed (row);

    if (rowIsSelected)
    {
        g.setColour (cadence::colours::accent.withAlpha (0.10f));
        g.fillRect (0, 0, width, height);
        g.setColour (cadence::colours::accent);
        g.fillRect (0, 0, 3, height);
    }

    const auto description = owner.audioProcessor.getSlotDescription (row);

    g.setColour (rowIsSelected ? cadence::colours::accent : cadence::colours::textDim);
    g.setFont (uiFont (11.0f, true));
    g.drawText (juce::String (row + 1).paddedLeft ('0', 2), 14, 8, 26, 18,
                juce::Justification::centredLeft, false);

    g.setColour (bypassed ? cadence::colours::textDim : cadence::colours::text);
    g.setFont (uiFont (14.0f, true));
    g.drawText (description.name, 44, 6, width - 100, 22,
                juce::Justification::centredLeft, true);

    g.setColour (cadence::colours::textDim);
    g.setFont (uiFont (10.5f));
    const auto loadingStatus = owner.audioProcessor.getSlotLoadStatus (row);
    g.drawText (loadingStatus.isNotEmpty() ? loadingStatus
                : description.pluginFormatName + juce::String::fromUTF8 ("  ·  ") + description.manufacturerName,
                44, 28, width - 100, 18, juce::Justification::centredLeft, true);

    if (bypassed)
    {
        g.setColour (cadence::colours::danger.withAlpha (0.18f));
        g.fillRoundedRectangle (static_cast<float> (width - 84), 16.0f, 68.0f, 20.0f, 3.0f);
        g.setColour (cadence::colours::danger);
        g.setFont (uiFont (10.0f, true));
        g.drawText ("BYPASS", width - 84, 16, 68, 20, juce::Justification::centred, false);
    }
}

void CadenceAudioProcessorEditor::ChainModel::listBoxItemDoubleClicked (int row, const juce::MouseEvent&)
{
    owner.showSlot (row);
}

void CadenceAudioProcessorEditor::ChainModel::selectedRowsChanged (int lastRowSelected)
{
    if (lastRowSelected >= 0)
        owner.showSlot (lastRowSelected);
}

CadenceAudioProcessorEditor::CadenceAudioProcessorEditor (CadenceAudioProcessor& p)
    : AudioProcessorEditor (&p), audioProcessor (p)
{
    setLookAndFeel (&theme);

    title.setText ("CADENCE", juce::dontSendNotification);
    title.setFont (uiFont (20.0f, true));
    title.setColour (juce::Label::textColourId, cadence::colours::text);

    subtitle.setText ("OPEN INSTRUMENT HOST", juce::dontSendNotification);
    subtitle.setFont (uiFont (10.0f, true));
    subtitle.setColour (juce::Label::textColourId, cadence::colours::textDim);

    status.setFont (uiFont (10.5f));
    status.setColour (juce::Label::textColourId, cadence::colours::textDim);
    status.setJustificationType (juce::Justification::centredRight);

    for (auto* label : { &libraryLabel, &chainLabel, &inspectorLabel })
    {
        label->setFont (uiFont (10.0f, true));
        label->setColour (juce::Label::textColourId, cadence::colours::textDim);
    }

    libraryLabel.setText ("LIBRARY", juce::dontSendNotification);
    chainLabel.setText ("CHAIN", juce::dontSendNotification);
    inspectorLabel.setText ("INSPECTOR", juce::dontSendNotification);

    automationLabel.setText ("DAW AUTOMATION", juce::dontSendNotification);
    automationLabel.setFont (uiFont (10.0f, true));
    automationLabel.setColour (juce::Label::textColourId, cadence::colours::textDim);
    automationHint.setText ("Assign each host control to one instrument parameter", juce::dontSendNotification);
    automationHint.setFont (uiFont (10.5f));
    automationHint.setColour (juce::Label::textColourId, cadence::colours::textDim);
    automationHint.setJustificationType (juce::Justification::centredRight);

    meterLabelLeft.setText ("L", juce::dontSendNotification);
    meterLabelRight.setText ("R", juce::dontSendNotification);
    for (auto* label : { &meterLabelLeft, &meterLabelRight })
    {
        label->setFont (uiFont (10.0f, true));
        label->setColour (juce::Label::textColourId, cadence::colours::textDim);
        label->setJustificationType (juce::Justification::centred);
    }

    browserList.setModel (&browserModel);
    browserList.setRowHeight (36);
    browserList.setOutlineThickness (0);
    browserList.setColour (juce::ListBox::backgroundColourId, cadence::colours::surface);
    chainList.setModel (&chainModel);
    chainList.setRowHeight (54);
    chainList.setOutlineThickness (0);
    chainList.setColour (juce::ListBox::backgroundColourId, cadence::colours::surface);

    inspectorContent.setLookAndFeel (nullptr);
    inspectorViewport.setViewedComponent (&inspectorContent, false);
    inspectorViewport.setScrollBarsShown (true, true);

    scanButton.onClick = [this] { scan(); };
    openButton.onClick = [this] { openSource(); };
    cancelLoadButton.onClick = [this] { audioProcessor.cancelLoading(); refreshChain (-1); };
    cancelLoadButton.setTooltip ("Cancel pending loads and keep the current rack playing");
    openButton.setTooltip ("Open a Kontakt instrument, CLAP plugin, native protocol library, or signed package manifest");
    skipButton.onClick = [this] { audioProcessor.skipCurrentPlugin(); };
    skippedButton.onClick = [this] { showSkipped(); };
    scanButton.setTooltip ("Scan installed plugins; click Cancel to stop and close the current scan helper");
    skipButton.setTooltip ("Close the current scan helper and skip this plugin on future scans");
    skippedButton.setTooltip ("Review skipped plugins and retry one after resolving its activation or loading problem");
    addButton.onClick = [this] { addSelected(); };
    bypassButton.onClick = [this]
    {
        const auto row = selectedChain();
        if (row >= 0)
        {
            audioProcessor.setSlotBypassed (row, ! audioProcessor.isSlotBypassed (row));
            refreshChain (row);
        }
    };
    removeButton.onClick = [this]
    {
        const auto row = selectedChain();
        if (row >= 0)
        {
            audioProcessor.removeSlot (row);
            refreshChain (juce::jmax (0, row - 1));
        }
    };
    upButton.onClick = [this]
    {
        const auto row = selectedChain();
        if (row > 0)
        {
            audioProcessor.moveSlot (row, -1);
            refreshChain (row - 1);
        }
    };
    downButton.onClick = [this]
    {
        const auto row = selectedChain();
        if (row >= 0 && row + 1 < audioProcessor.getNumSlots())
        {
            audioProcessor.moveSlot (row, 1);
            refreshChain (row + 1);
        }
    };

    for (auto* button : { &scanButton, &openButton, &cancelLoadButton, &skipButton, &skippedButton, &addButton, &bypassButton,
                          &removeButton, &upButton, &downButton })
        addAndMakeVisible (button);

    addAndMakeVisible (automationLabel);
    addAndMakeVisible (automationHint);
    for (int control = 0; control < cadence::Automation::numControls; ++control)
    {
        auto& number = automationNumbers[static_cast<size_t> (control)];
        number.setText ("A" + juce::String (control + 1), juce::dontSendNotification);
        number.setFont (uiFont (10.5f, true));
        number.setJustificationType (juce::Justification::centred);
        number.setColour (juce::Label::textColourId, cadence::colours::accent);
        addAndMakeVisible (number);

        auto& box = automationBoxes[static_cast<size_t> (control)];
        box.setName ("Automation " + juce::String (control + 1));
        box.addItem ("Unassigned", 1);
        box.setSelectedId (1, juce::dontSendNotification);
        box.setTooltip ("Host control " + juce::String (control + 1) + " assignment");
        box.onChange = [this, control] { assignAutomationFromBox (control); };
        addAndMakeVisible (box);

        auto& clear = automationClearButtons[static_cast<size_t> (control)];
        clear.setName ("Clear Automation " + juce::String (control + 1));
        clear.setButtonText ("X");
        clear.setTooltip ("Clear Automation " + juce::String (control + 1));
        clear.onClick = [this, control]
        {
            audioProcessor.clearAutomation (control);
            refreshAutomation();
        };
        addAndMakeVisible (clear);
    }

    libraryHint.setText ("No plugins yet - press Scan", juce::dontSendNotification);
    chainHint.setText ("Add a plugin from the library", juce::dontSendNotification);
    inspectorHint.setText ("Select a plugin to edit", juce::dontSendNotification);
    for (auto* hint : { &libraryHint, &chainHint, &inspectorHint })
    {
        hint->setFont (uiFont (11.0f));
        hint->setColour (juce::Label::textColourId, cadence::colours::textDim);
        hint->setJustificationType (juce::Justification::centred);
        hint->setInterceptsMouseClicks (false, false);
        addAndMakeVisible (hint);
    }

    for (auto* component : { static_cast<juce::Component*> (&title),
                             static_cast<juce::Component*> (&subtitle),
                             static_cast<juce::Component*> (&status),
                             static_cast<juce::Component*> (&libraryLabel),
                             static_cast<juce::Component*> (&chainLabel),
                             static_cast<juce::Component*> (&inspectorLabel),
                             static_cast<juce::Component*> (&meterLabelLeft),
                             static_cast<juce::Component*> (&meterLabelRight),
                             static_cast<juce::Component*> (&browserList),
                             static_cast<juce::Component*> (&chainList),
                             static_cast<juce::Component*> (&inspectorViewport),
                             static_cast<juce::Component*> (&meterLeft),
                             static_cast<juce::Component*> (&meterRight) })
        addAndMakeVisible (component);

    for (auto* hint : { &libraryHint, &chainHint, &inspectorHint })
        hint->toFront (false);

    setResizable (true, true);
    setResizeLimits (1000, 650, 2600, 1600);
    setSize (1240, 780);

    refreshBrowser();
    refreshChain (-1);
    refreshAutomation();
    lastPluginsVersion = audioProcessor.getPluginsVersion();
    timerCallback();
    startTimerHz (30);
}

CadenceAudioProcessorEditor::~CadenceAudioProcessorEditor()
{
    currentEditor.reset();
    setLookAndFeel (nullptr);
}

void CadenceAudioProcessorEditor::paint (juce::Graphics& g)
{
    g.fillAll (cadence::colours::background);

    g.setColour (cadence::colours::surface);
    g.fillRect (0, 0, getWidth(), 66);
    g.fillRect (0, getHeight() - 46, getWidth(), 46);

    g.setColour (cadence::colours::border);
    g.drawHorizontalLine (66, 0.0f, static_cast<float> (getWidth()));
    g.drawHorizontalLine (getHeight() - 46, 0.0f, static_cast<float> (getWidth()));
    g.drawVerticalLine (286, 66.0f, static_cast<float> (getHeight() - 46));
    g.drawVerticalLine (getWidth() - 452, 66.0f, static_cast<float> (getHeight() - 46));

    g.setColour (cadence::colours::accent);
    g.fillRect (20, 56, 52, 2);
}

void CadenceAudioProcessorEditor::resized()
{
    auto area = getLocalBounds();
    auto top = area.removeFromTop (66);
    auto bottom = area.removeFromBottom (46);
    auto left = area.removeFromLeft (286);
    auto right = area.removeFromRight (452);
    auto centre = area;

    auto topInner = top.reduced (20, 10);
    auto titleArea = topInner.removeFromLeft (260);
    title.setBounds (titleArea.removeFromTop (26));
    subtitle.setBounds (titleArea.removeFromTop (14));
    scanButton.setBounds (topInner.removeFromRight (96).reduced (0, 8));
    topInner.removeFromRight (8);
    skipButton.setBounds (topInner.removeFromRight (100).reduced (0, 8));
    topInner.removeFromRight (8);
    skippedButton.setBounds (topInner.removeFromRight (112).reduced (0, 8));
    topInner.removeFromRight (8);
    openButton.setBounds (topInner.removeFromRight (84).reduced (0, 8));
    topInner.removeFromRight (8);
    cancelLoadButton.setBounds (topInner.removeFromRight (108).reduced (0, 8));

    auto leftInner = left.reduced (14, 12);
    libraryLabel.setBounds (leftInner.removeFromTop (22));
    addButton.setBounds (leftInner.removeFromBottom (30));
    browserList.setBounds (leftInner.reduced (0, 8));

    auto centreInner = centre.reduced (14, 12);
    chainLabel.setBounds (centreInner.removeFromTop (22));
    auto chainButtons = centreInner.removeFromBottom (30);
    auto automationArea = centreInner.removeFromBottom (juce::jmin (128, juce::jmax (0, centreInner.getHeight() - 120)));
    chainList.setBounds (centreInner.reduced (0, 8));

    auto automationInner = automationArea;
    auto automationHeader = automationInner.removeFromTop (18);
    automationLabel.setBounds (automationHeader.removeFromLeft (130));
    automationHint.setBounds (automationHeader);
    automationInner.removeFromTop (4);
    const auto cellHeight = automationInner.getHeight() / 4;
    for (int row = 0; row < 4; ++row)
    {
        auto line = automationInner.removeFromTop (cellHeight);
        auto leftCell = line.removeFromLeft ((line.getWidth() - 6) / 2);
        line.removeFromLeft (6);
        for (int column = 0; column < 2; ++column)
        {
            const auto control = row * 2 + column;
            auto cell = column == 0 ? leftCell : line;
            automationNumbers[static_cast<size_t> (control)].setBounds (cell.removeFromLeft (24));
            automationClearButtons[static_cast<size_t> (control)].setBounds (cell.removeFromRight (22).reduced (0, 2));
            cell.removeFromRight (4);
            automationBoxes[static_cast<size_t> (control)].setBounds (cell.reduced (0, 2));
        }
    }

    bypassButton.setBounds (chainButtons.removeFromLeft (76));
    chainButtons.removeFromLeft (6);
    removeButton.setBounds (chainButtons.removeFromLeft (76));
    chainButtons.removeFromLeft (6);
    upButton.setBounds (chainButtons.removeFromLeft (58));
    chainButtons.removeFromLeft (6);
    downButton.setBounds (chainButtons.removeFromLeft (58));

    auto rightInner = right.reduced (14, 12);
    inspectorLabel.setBounds (rightInner.removeFromTop (22));
    inspectorViewport.setBounds (rightInner.reduced (0, 8));

    libraryHint.setBounds (browserList.getBounds());
    chainHint.setBounds (chainList.getBounds());
    inspectorHint.setBounds (inspectorViewport.getBounds());

    auto bottomInner = bottom.reduced (20, 10);
    meterLabelLeft.setBounds (bottomInner.removeFromLeft (18));
    meterLeft.setBounds (bottomInner.removeFromLeft (110).reduced (0, 8));
    bottomInner.removeFromLeft (12);
    meterLabelRight.setBounds (bottomInner.removeFromLeft (18));
    meterRight.setBounds (bottomInner.removeFromLeft (110).reduced (0, 8));
    status.setBounds (bottomInner.reduced (16, 0));
}

void CadenceAudioProcessorEditor::timerCallback()
{
    if (lastChainVersion != audioProcessor.getChainVersion())
        refreshChain (selectedChain());
    const auto version = audioProcessor.getPluginsVersion();
    if (lastPluginsVersion != version)
    {
        lastPluginsVersion = version;
        refreshBrowser();
    }

    const auto scanning = audioProcessor.isScanning();
    const auto loading = audioProcessor.isLoading();
    openButton.setEnabled (! scanning && ! loading);
    addButton.setEnabled (! loading);
    scanButton.setEnabled (! loading || scanning);
    cancelLoadButton.setVisible (loading);
    scanButton.setButtonText (scanning ? "Cancel" : "Scan");
    skipButton.setVisible (scanning);
    skipButton.setEnabled (audioProcessor.canSkipPlugin());
    const auto skippedCount = audioProcessor.getSkippedPluginCount();
    skippedButton.setButtonText ("Skipped (" + juce::String (skippedCount) + ")");
    skippedButton.setEnabled (! scanning && skippedCount > 0);
    libraryHint.setText (scanning ? "Scanning plugins..." : "No plugins yet - press Scan",
                         juce::dontSendNotification);

    meterLeft.setLevel (audioProcessor.getOutputLevel (0) * 1.6f);
    meterRight.setLevel (audioProcessor.getOutputLevel (1) * 1.6f);

    const auto text = audioProcessor.getStatus()
        + (audioProcessor.getNumSlots() > 0 ? " - " + juce::String (audioProcessor.getLatencySamples())
             + " sample delay - Host dropouts: " + juce::String (audioProcessor.getUnderruns()) : juce::String());
    if (status.getText() != text)
        status.setText (text, juce::dontSendNotification);
    status.setTooltip (text);
}

void CadenceAudioProcessorEditor::refreshBrowser()
{
    browserList.updateContent();
    libraryHint.setVisible (audioProcessor.getPlugins().empty());
}

void CadenceAudioProcessorEditor::refreshChain (int select)
{
    lastChainVersion = audioProcessor.getChainVersion();
    chainList.updateContent();

    if (select >= 0 && select < audioProcessor.getNumSlots())
    {
        chainList.selectRow (select);
        showSlot (select);
    }
    else if (audioProcessor.getNumSlots() == 0)
    {
        currentEditor.reset();
        inspectorContent.removeAllChildren();
        inspectorContent.setSize (0, 0);
        inspectorLabel.setText ("INSPECTOR", juce::dontSendNotification);
    }

    chainHint.setVisible (audioProcessor.getNumSlots() == 0);

    const auto hasSelection = selectedChain() >= 0;
    bypassButton.setEnabled (hasSelection);
    removeButton.setEnabled (hasSelection);
    upButton.setEnabled (hasSelection);
    downButton.setEnabled (hasSelection);
    refreshAutomation();
}

void CadenceAudioProcessorEditor::refreshAutomation()
{
    for (int control = 0; control < cadence::Automation::numControls; ++control)
    {
        auto& box = automationBoxes[static_cast<size_t> (control)];
        auto& entries = automationEntries[static_cast<size_t> (control)];
        box.clear (juce::dontSendNotification);
        entries.clear();
        box.addItem ("Unassigned", 1);

        const auto assigned = audioProcessor.automationAssignment (control);
        int selected = 1;
        int itemId = 2;
        for (int slot = 0; slot < audioProcessor.getNumSlots(); ++slot)
        {
            const auto uuid = audioProcessor.getSlotUuid (slot);
            for (const auto& target : audioProcessor.getAutomationTargets (slot))
            {
                entries.push_back ({ slot, target.key });
                box.addItem (juce::String (slot + 1) + ". " + audioProcessor.getSlotName (slot) + " / " + target.name,
                             itemId);
                if (uuid == assigned.slot && target.key == assigned.key)
                    selected = itemId;
                ++itemId;
            }
        }
        if (assigned.slot.isNotEmpty() && selected == 1)
        {
            box.addItem ("Unresolved: " + assigned.name, 900000 + control);
            selected = 900000 + control;
        }
        box.setSelectedId (selected, juce::dontSendNotification);
        box.setEnabled (itemId > 2);
        automationClearButtons[static_cast<size_t> (control)].setEnabled (assigned.slot.isNotEmpty());
    }
}

void CadenceAudioProcessorEditor::assignAutomationFromBox (int control)
{
    auto& box = automationBoxes[static_cast<size_t> (control)];
    const auto itemId = box.getSelectedId();
    if (itemId >= 900000)
        return;
    if (itemId <= 1)
    {
        audioProcessor.clearAutomation (control);
        refreshAutomation();
        return;
    }

    const auto index = itemId - 2;
    const auto& entries = automationEntries[static_cast<size_t> (control)];
    if (! juce::isPositiveAndBelow (index, static_cast<int> (entries.size())))
        return;

    const auto entry = entries[static_cast<size_t> (index)];
    juce::String error;
    if (! audioProcessor.assignAutomation (control, entry.slot, entry.key, error))
        audioProcessor.setStatus ("Automation: " + error);
    refreshAutomation();
}

void CadenceAudioProcessorEditor::clearSlotEditor()
{
    currentEditor.reset();
    inspectorContent.removeAllChildren();
    inspectorContent.setSize (0, 0);
    inspectorLabel.setText ("INSPECTOR", juce::dontSendNotification);
    inspectorHint.setText ("Select a plugin to edit", juce::dontSendNotification);
    inspectorHint.setVisible (true);
}

void CadenceAudioProcessorEditor::showSlot (int index)
{
    if (! juce::isPositiveAndBelow (index, audioProcessor.getNumSlots()))
        return;

    currentEditor.reset();
    inspectorContent.removeAllChildren();

    auto* editor = audioProcessor.createSlotEditor (index);
    if (editor == nullptr)
    {
        inspectorContent.setSize (0, 0);
        inspectorHint.setText (audioProcessor.getSlotLoadStatus (index).isNotEmpty()
                              ? audioProcessor.getSlotLoadStatus (index) : "Instrument unavailable", juce::dontSendNotification);
        inspectorHint.setVisible (true);
        inspectorLabel.setText (juce::String::fromUTF8 ("INSPECTOR  ·  ") + audioProcessor.getSlotName (index), juce::dontSendNotification);
        return;
    }

    currentEditor.reset (editor);
    inspectorHint.setVisible (false);
    editor->setLookAndFeel (nullptr);

    const int width = juce::jmax (editor->getWidth(), 320);
    const int height = juce::jmax (editor->getHeight(), 220);
    editor->setTopLeftPosition (0, 0);
    inspectorContent.setSize (width, height);
    inspectorContent.addAndMakeVisible (editor);
    inspectorViewport.setViewPosition (0, 0);
    inspectorLabel.setText (juce::String::fromUTF8 ("INSPECTOR  ·  ") + audioProcessor.getSlotName (index),
                            juce::dontSendNotification);
}

void CadenceAudioProcessorEditor::addSelected()
{
    if (audioProcessor.isLoading()) return;
    const auto row = selectedBrowser();
    const auto& plugins = audioProcessor.getPlugins();
    if (! juce::isPositiveAndBelow (row, static_cast<int> (plugins.size())))
        return;

    audioProcessor.loadPluginAsync (plugins[static_cast<size_t> (row)]);
    refreshChain (audioProcessor.getNumSlots() - 1);
    timerCallback();
}

void CadenceAudioProcessorEditor::scan()
{
    if (audioProcessor.isScanning())
        audioProcessor.cancelScan();
    else
        audioProcessor.scanPlugins();
    timerCallback();
}

void CadenceAudioProcessorEditor::showSkipped()
{
    const auto skipped = audioProcessor.getSkippedPlugins();
    juce::PopupMenu menu;
    menu.addSectionHeader ("Retry one plugin");
    int id = 1;
    for (const auto& item : skipped)
        menu.addItem (id++, item.candidate.name() + " (" + item.candidate.format + ") - " + item.reason);
    menu.showMenuAsync (juce::PopupMenu::Options().withTargetComponent (&skippedButton),
        [safe = juce::Component::SafePointer<CadenceAudioProcessorEditor> (this), skipped] (int choice)
        {
            if (safe != nullptr && juce::isPositiveAndBelow (choice - 1, static_cast<int> (skipped.size())))
                safe->audioProcessor.retrySkippedPlugin (skipped[static_cast<size_t> (choice - 1)].candidate);
        });
}

void CadenceAudioProcessorEditor::openSource()
{
    sourceChooser = std::make_unique<juce::FileChooser> (
        "Open an instrument or plugin", juce::File(), "*.nki;*.nkm;*.clap;*.dylib;manifest.json");
    sourceChooser->launchAsync (juce::FileBrowserComponent::openMode | juce::FileBrowserComponent::canSelectFiles,
        [safe = juce::Component::SafePointer<CadenceAudioProcessorEditor> (this)] (const juce::FileChooser& chooser)
        {
            if (safe == nullptr) return;
            const auto file = chooser.getResult();
            if (! file.exists()) return;
            const auto format = file.hasFileExtension ("nki;nkm") ? "Kontakt" : file.hasFileExtension ("clap") ? "CLAP" : "Protocol";
            safe->audioProcessor.retrySkippedPlugin ({ format, file.getFullPathName() });
        });
}

int CadenceAudioProcessorEditor::selectedChain() const
{
    return chainList.getSelectedRow();
}

int CadenceAudioProcessorEditor::selectedBrowser() const
{
    return browserList.getSelectedRow();
}
