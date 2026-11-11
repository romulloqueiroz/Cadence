//! Conformance harness: the tests every plugin must pass before publishing.

use crate::{
    AudioBuffer, Capabilities, Event, ParameterInfo, Plugin, PluginKind, ProcessContext,
    StateError, UiNode, API_VERSION, MAX_BLOCK_SIZE, MAX_CHANNELS, MAX_PARAMETERS,
};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub struct ConformanceConfig {
    pub sample_rate: f64,
    pub block_size: usize,
    pub frames: usize,
    /// Require audible output after a note-on (instruments).
    pub expect_sound: bool,
}

impl Default for ConformanceConfig {
    fn default() -> Self {
        ConformanceConfig {
            sample_rate: 48_000.0,
            block_size: 256,
            frames: 4096,
            expect_sound: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ConformanceReport {
    pub failures: Vec<String>,
}

impl ConformanceReport {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.failures.push(message.into());
    }
}

/// A pass certificate produced by [`gate`]. The publish flow will bind this to
/// a signed package manifest; this type deliberately contains no fake crypto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    pub plugin_id: String,
    pub version: String,
    pub api_version: u32,
    pub format_version: u32,
}

/// The publish gate: run conformance and only issue a certificate on success.
pub fn gate<P: Plugin>(
    plugin: &mut P,
    config: &ConformanceConfig,
) -> Result<Certificate, ConformanceReport> {
    let report = run(plugin, config);
    if !report.passed() {
        return Err(report);
    }
    let descriptor = plugin.descriptor();
    Ok(Certificate {
        plugin_id: descriptor.id.to_string(),
        version: descriptor.version.to_string(),
        api_version: API_VERSION,
        format_version: 1,
    })
}

/// Run the standard conformance suite.
pub fn run<P: Plugin>(plugin: &mut P, config: &ConformanceConfig) -> ConformanceReport {
    let mut report = ConformanceReport::default();

    let descriptor = plugin.descriptor();
    if !descriptor.is_valid() {
        report.fail(format!("invalid descriptor: {descriptor:?}"));
    }

    let capabilities = plugin.capabilities();
    if !capabilities.is_valid() {
        report.fail(format!("invalid capabilities: {capabilities:?}"));
        return report;
    }

    let parameters = plugin.parameters();
    if parameters.len() > MAX_PARAMETERS {
        report.fail(format!(
            "too many parameters: {} > {MAX_PARAMETERS}",
            parameters.len()
        ));
    }
    check_parameters(&parameters, &mut report);
    check_ui(&plugin.ui(), &parameters, &mut report);

    let context = ProcessContext {
        sample_rate: config.sample_rate,
        block_size: config.block_size.clamp(1, MAX_BLOCK_SIZE),
        tempo: 120.0,
        playing: true,
        sample_pos: 0,
        ppq_position: 0.0,
    };
    if !context.is_valid() {
        report.fail(format!("invalid process context: {context:?}"));
        return report;
    }

    plugin.prepare(&context);
    plugin.reset();

    // 1. No input and no events must be finite; instruments must be silent.
    let output = render(
        plugin,
        &context,
        capabilities,
        config.frames,
        |_| Vec::new(),
        |_, _, _| {},
    );
    check_outputs(&output, "silence", &mut report);
    if descriptor.kind == PluginKind::Instrument && rms(&output) > 1.0e-4 {
        report.fail(format!(
            "instrument produced output without a note (rms {:e})",
            rms(&output)
        ));
    }

    // 2. Note-on/off for instruments.
    if descriptor.kind == PluginKind::Instrument {
        plugin.reset();
        let output = render(
            plugin,
            &context,
            capabilities,
            config.frames,
            |block| {
                if block == 0 {
                    vec![Event::note_on(0, 60, 100)]
                } else {
                    Vec::new()
                }
            },
            |_, _, _| {},
        );
        check_outputs(&output, "note-on", &mut report);
        check_bounded(&output, "note-on", &mut report);
        if config.expect_sound && rms(&output) < 1.0e-6 {
            report.fail(format!(
                "note-on produced no audible output (rms {:e})",
                rms(&output)
            ));
        }

        plugin.reset();
        let output = render(
            plugin,
            &context,
            capabilities,
            config.frames,
            |block| match block {
                0 => vec![Event::note_on(0, 60, 100)],
                8 => vec![Event::note_off(0, 60)],
                _ => Vec::new(),
            },
            |_, _, _| {},
        );
        check_outputs(&output, "note-off", &mut report);
        check_bounded(&output, "note-off", &mut report);
    }

    // 3. Effects must return finite audio for a finite input.
    if descriptor.kind == PluginKind::Effect && capabilities.inputs > 0 {
        plugin.reset();
        let mut phase = 0.0f32;
        let output = render(
            plugin,
            &context,
            capabilities,
            config.frames,
            |_| Vec::new(),
            |_, _, slice| {
                let increment = std::f32::consts::TAU * 220.0 / config.sample_rate as f32;
                for value in slice.iter_mut() {
                    *value = phase.sin() * 0.25;
                    phase = (phase + increment) % std::f32::consts::TAU;
                }
            },
        );
        check_outputs(&output, "effect-input", &mut report);
    }

    // 4. State round-trip.
    let state = plugin.save_state();
    plugin.reset();
    match plugin.load_state(&state) {
        Ok(()) => {
            let restored = plugin.save_state();
            if restored != state {
                report.fail("state did not round-trip");
            }
        }
        Err(StateError::SchemaTooNew { found, supported }) => {
            report.fail(format!(
                "plugin rejected its own state schema {found} (supported {supported})"
            ));
        }
        Err(error) => report.fail(format!("load_state failed: {error}")),
    }

    report
}

fn check_parameters(parameters: &[ParameterInfo], report: &mut ConformanceReport) {
    let mut ids = HashSet::new();
    for parameter in parameters {
        if !parameter.is_valid() {
            report.fail(format!("invalid parameter: {parameter:?}"));
        }
        if !ids.insert(parameter.id) {
            report.fail(format!("duplicate parameter id {}", parameter.id));
        }
    }
}

fn check_ui(ui: &[UiNode], parameters: &[ParameterInfo], report: &mut ConformanceReport) {
    let ids: HashSet<u32> = parameters.iter().map(|p| p.id).collect();
    for node in ui {
        check_ui_node(node, &ids, report);
    }
}

fn check_ui_node(node: &UiNode, ids: &HashSet<u32>, report: &mut ConformanceReport) {
    match node {
        UiNode::Group { children, .. } => {
            for child in children {
                check_ui_node(child, ids, report);
            }
        }
        UiNode::Knob { param, .. }
        | UiNode::Toggle { param, .. }
        | UiNode::Selector { param, .. } => {
            if !ids.contains(param) {
                report.fail(format!("UI references unknown parameter {param}"));
            }
        }
        UiNode::Label { .. } | UiNode::Meter { .. } => {}
    }
}

fn check_outputs(output: &[Vec<f32>], label: &str, report: &mut ConformanceReport) {
    if output.is_empty() {
        report.fail(format!("{label}: no output channels"));
        return;
    }
    for (channel, samples) in output.iter().enumerate() {
        for (index, value) in samples.iter().enumerate() {
            if !value.is_finite() {
                report.fail(format!(
                    "{label}: channel {channel} non-finite sample at {index}: {value}"
                ));
                return;
            }
        }
    }
}

fn check_bounded(output: &[Vec<f32>], label: &str, report: &mut ConformanceReport) {
    for (channel, samples) in output.iter().enumerate() {
        for (index, value) in samples.iter().enumerate() {
            if value.abs() > 1.0 {
                report.fail(format!(
                    "{label}: channel {channel} sample {value} out of [-1, 1] at {index}"
                ));
                return;
            }
        }
    }
}

fn rms(output: &[Vec<f32>]) -> f64 {
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for channel in output {
        for value in channel {
            sum += (*value as f64) * (*value as f64);
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64).sqrt()
    }
}

#[allow(clippy::too_many_arguments)]
fn render<P, E, I>(
    plugin: &mut P,
    context: &ProcessContext,
    capabilities: Capabilities,
    frames: usize,
    mut events_for_block: E,
    mut fill_input: I,
) -> Vec<Vec<f32>>
where
    P: Plugin,
    E: FnMut(usize) -> Vec<Event>,
    I: FnMut(usize, usize, &mut [f32]),
{
    let block = context.block_size.clamp(1, MAX_BLOCK_SIZE);
    let outputs = capabilities.outputs.min(MAX_CHANNELS);
    let inputs = capabilities.inputs.min(MAX_CHANNELS);
    let sidechain = capabilities.sidechain_inputs.min(MAX_CHANNELS);

    let mut output = vec![Vec::with_capacity(frames); outputs];
    let mut input_data = vec![0.0f32; inputs * block];
    let mut output_data = vec![0.0f32; outputs * block];
    let mut sidechain_data = vec![0.0f32; sidechain * block];

    let mut position = 0usize;
    let mut block_index = 0usize;
    while position < frames {
        let n = (frames - position).min(block);

        input_data.fill(0.0);
        output_data.fill(0.0);
        sidechain_data.fill(0.0);

        for channel in 0..inputs {
            let start = channel * n;
            fill_input(block_index, channel, &mut input_data[start..start + n]);
        }

        let events: Vec<Event> = events_for_block(block_index)
            .into_iter()
            .filter(|event| event.frame < n)
            .collect();

        let mut block_context = *context;
        block_context.block_size = n;
        block_context.sample_pos = position as i64;

        let mut audio = AudioBuffer::new(
            &mut input_data,
            inputs,
            &mut output_data,
            outputs,
            &mut sidechain_data,
            sidechain,
            n,
        );
        plugin.process(&block_context, &mut audio, &events);

        for (channel, channel_output) in output.iter_mut().enumerate() {
            let start = channel * n;
            channel_output.extend_from_slice(&output_data[start..start + n]);
        }

        position += n;
        block_index += 1;
    }

    output
}
