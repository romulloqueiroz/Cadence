//! Zero-latency FIR head plus uniformly partitioned FFT tail. All kernels,
//! spectra and histories are allocated during load/prepare, never processing.
use nk_format::{audio::Sample, fx::ConvolutionParams};

pub(crate) const MAX_IR_SAMPLES: usize = 2_097_152;
const P: usize = 256;
const N: usize = P * 2;

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

struct Fft {
    reverse: Vec<usize>,
    roots: Vec<Complex>,
}
impl Fft {
    fn new() -> Self {
        Self {
            reverse: (0..N)
                .map(|i| i.reverse_bits() >> (usize::BITS - N.ilog2()))
                .collect(),
            roots: (0..N / 2)
                .map(|i| {
                    let angle = -std::f64::consts::TAU * i as f64 / N as f64;
                    Complex {
                        re: angle.cos() as f32,
                        im: angle.sin() as f32,
                    }
                })
                .collect(),
        }
    }
    fn run(&self, data: &mut [Complex], inverse: bool) {
        for i in 0..N {
            if i < self.reverse[i] {
                data.swap(i, self.reverse[i]);
            }
        }
        let mut size = 2;
        while size <= N {
            let half = size / 2;
            for base in (0..N).step_by(size) {
                for j in 0..half {
                    let mut w = self.roots[j * (N / size)];
                    if inverse {
                        w.im = -w.im;
                    }
                    let a = data[base + j];
                    let b = data[base + j + half];
                    let re = b.re * w.re - b.im * w.im;
                    let im = b.re * w.im + b.im * w.re;
                    data[base + j] = Complex {
                        re: a.re + re,
                        im: a.im + im,
                    };
                    data[base + j + half] = Complex {
                        re: a.re - re,
                        im: a.im - im,
                    };
                }
            }
            size *= 2;
        }
        if inverse {
            for x in data {
                x.re /= N as f32;
                x.im /= N as f32;
            }
        }
    }
}

struct Channel {
    head: Vec<f32>,
    kernels: Vec<Complex>,
    history: Vec<Complex>,
    live: Vec<bool>,
    work: Vec<Complex>,
    input: [f32; P],
    fir_history: [f32; P],
    ready: [f32; P],
    overlap: [f32; P],
    position: usize,
    next: usize,
    length: usize,
    remaining: usize,
}
impl Channel {
    fn new(ir: &[f32], fft: &Fft) -> Self {
        let parts = ir.len().saturating_sub(P).div_ceil(P);
        let mut kernels = vec![Complex::default(); parts * N];
        for part in 0..parts {
            let spectrum = &mut kernels[part * N..(part + 1) * N];
            for (bin, tap) in spectrum
                .iter_mut()
                .zip(ir.iter().skip(P * (part + 1)).take(P))
            {
                bin.re = *tap;
            }
            fft.run(spectrum, false);
        }
        Self {
            head: ir[..ir.len().min(P)].to_vec(),
            kernels,
            history: vec![Complex::default(); parts * N],
            live: vec![false; parts],
            work: vec![Complex::default(); N],
            input: [0.0; P],
            fir_history: [0.0; P],
            ready: [0.0; P],
            overlap: [0.0; P],
            position: 0,
            next: 0,
            length: ir.len(),
            remaining: 0,
        }
    }
    fn reset(&mut self) {
        self.live.fill(false);
        self.input.fill(0.0);
        self.fir_history.fill(0.0);
        self.ready.fill(0.0);
        self.overlap.fill(0.0);
        self.position = 0;
        self.next = 0;
        self.remaining = 0;
    }
    fn step(&mut self, input: f32, fft: &Fft) -> f32 {
        let pos = self.position;
        self.fir_history[pos] = input;
        self.input[pos] = input;
        let mut output = self.ready[pos];
        for (tap, coefficient) in self.head.iter().enumerate() {
            output += coefficient * self.fir_history[(pos + P - tap) & (P - 1)];
        }
        if input != 0.0 {
            self.remaining = self.length;
        }
        if self.remaining == 0 {
            output = 0.0;
        } else {
            self.remaining -= 1;
        }
        self.position += 1;
        if self.position == P {
            self.position = 0;
            self.finish_block(fft);
        }
        output
    }
    fn finish_block(&mut self, fft: &Fft) {
        let parts = self.live.len();
        if parts == 0 {
            return;
        }
        self.live[self.next] = self.input.iter().any(|v| *v != 0.0);
        if self.live[self.next] {
            let spectrum = &mut self.history[self.next * N..(self.next + 1) * N];
            spectrum.fill(Complex::default());
            for (bin, sample) in spectrum.iter_mut().zip(self.input) {
                bin.re = sample;
            }
            fft.run(spectrum, false);
        }
        self.work.fill(Complex::default());
        let mut any = false;
        for part in 0..parts {
            let index = (self.next + parts - part) % parts;
            if !self.live[index] {
                continue;
            }
            any = true;
            let x = &self.history[index * N..(index + 1) * N];
            let h = &self.kernels[part * N..(part + 1) * N];
            for ((sum, x), h) in self.work.iter_mut().zip(x).zip(h) {
                sum.re += x.re * h.re - x.im * h.im;
                sum.im += x.re * h.im + x.im * h.re;
            }
        }
        if any {
            fft.run(&mut self.work, true);
        }
        for i in 0..P {
            self.ready[i] = self.work[i].re + self.overlap[i];
            self.overlap[i] = self.work[P + i].re;
        }
        self.next = (self.next + 1) % parts;
    }
}

pub(crate) struct Convolution {
    source: Sample,
    params: ConvolutionParams,
    fft: Fft,
    channels: Vec<Channel>,
    delay: Vec<[f32; 2]>,
    delay_position: usize,
    rate: f32,
    tail: usize,
}
impl Convolution {
    pub(crate) fn new(source: Sample, params: ConvolutionParams) -> Result<Self, &'static str> {
        if !(1..=2).contains(&source.channels)
            || source.num_frames == 0
            || source.sample_rate == 0
            || source.num_frames > MAX_IR_SAMPLES / source.channels
            || source.samples.len() != source.channels
            || source.samples.iter().any(|c| {
                c.len() != source.num_frames || c.iter().any(|v| !v.is_finite() || v.abs() > 64.0)
            })
        {
            return Err("IR must be finite mono/stereo audio within the resident IR budget");
        }
        if params.rate_factor != -1.0 && params.rate_factor != 1.0 {
            return Err("IRC sample-rate reduction factors other than auto/1 are unsupported");
        }
        if params.early_length != 1.0
            || params.late_length != 1.0
            || params.envelope_active
            || params.early_highpass != 20.0
            || params.late_highpass != 20.0
            || params.early_lowpass != 20_000.0
            || params.late_lowpass != 20_000.0
        {
            return Err("IRC stretching, nondefault ER/LR EQ and active envelopes are unsupported");
        }
        if !(0.0..=2000.0).contains(&params.predelay_ms) || params.block_size < 0 {
            return Err("IRC predelay/block size is outside supported bounds");
        }
        Ok(Self {
            source,
            params,
            fft: Fft::new(),
            channels: Vec::new(),
            delay: Vec::new(),
            delay_position: 0,
            rate: 0.0,
            tail: 0,
        })
    }
    pub(crate) fn prepare(&mut self, rate: f32, budget: &mut usize) -> Result<(), &'static str> {
        if !rate.is_finite() || !(8000.0..=192000.0).contains(&rate) {
            return Err("unsupported convolution sample rate");
        }
        let length =
            ((self.source.num_frames as f64 * rate as f64 / self.source.sample_rate as f64).ceil()
                as usize)
                .max(1);
        let samples = length.checked_mul(2).ok_or("IR size overflow")?;
        *budget = budget.checked_add(samples).ok_or("IR budget overflow")?;
        if *budget > MAX_IR_SAMPLES {
            return Err("prepared IRs exceed resident IR budget");
        }
        if rate == self.rate {
            return Ok(());
        }
        let mut ir = Vec::with_capacity(self.source.channels);
        for channel in &self.source.samples {
            let mut taps = Vec::with_capacity(length);
            for i in 0..length {
                let p = i as f64 * self.source.sample_rate as f64 / rate as f64;
                let index = (p as usize).min(channel.len() - 1);
                let next = channel.get(index + 1).copied().unwrap_or(0.0);
                taps.push(channel[index] + (next - channel[index]) * (p - index as f64) as f32);
            }
            if self.params.reverse {
                taps.reverse();
            }
            ir.push(taps);
        }
        if self.params.auto_gain {
            // BEffectIRC::RebuildIRC: max channel energy, then sqrt(1/(2E)),
            // capped at 2; near-silent kernels retain unity.
            let energy = ir
                .iter()
                .map(|c| c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>())
                .fold(0f64, f64::max);
            let gain = if energy < 0.001 {
                1.0
            } else {
                (1.0 / (2.0 * energy)).sqrt().min(2.0) as f32
            };
            for channel in &mut ir {
                for tap in channel {
                    *tap *= gain;
                }
            }
        }
        self.channels = (0..2)
            .map(|channel| Channel::new(&ir[channel % ir.len()], &self.fft))
            .collect();
        let predelay = (self.params.predelay_ms as f64 * rate as f64 / 1000.0).round() as usize;
        self.delay = vec![[0.0; 2]; predelay];
        self.delay_position = 0;
        self.tail = length - 1 + predelay;
        self.rate = rate;
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.delay.fill([0.0; 2]);
        self.delay_position = 0;
    }
    pub(crate) fn memory_bytes(&self) -> usize {
        use std::mem::size_of;
        size_of::<Self>()
            + self.source.samples.capacity() * size_of::<Vec<f32>>()
            + self
                .source
                .samples
                .iter()
                .map(|c| c.capacity() * size_of::<f32>())
                .sum::<usize>()
            + self.fft.reverse.capacity() * size_of::<usize>()
            + self.fft.roots.capacity() * size_of::<Complex>()
            + self.channels.capacity() * size_of::<Channel>()
            + self
                .channels
                .iter()
                .map(|c| {
                    c.head.capacity() * size_of::<f32>()
                        + (c.kernels.capacity() + c.history.capacity() + c.work.capacity())
                            * size_of::<Complex>()
                        + c.live.capacity() * size_of::<bool>()
                })
                .sum::<usize>()
            + self.delay.capacity() * size_of::<[f32; 2]>()
    }
    pub(crate) fn tail(&self) -> usize {
        self.tail
    }
    pub(crate) fn step(&mut self, input: [f32; 2]) -> [f32; 2] {
        if self.channels.is_empty() {
            return [0.0; 2];
        }
        // A mono kernel filters each stereo input independently: keep two
        // independent channel histories even for a single IR channel.
        let mut output = [
            self.channels[0].step(input[0], &self.fft),
            self.channels[1].step(input[1], &self.fft),
        ];
        if !self.delay.is_empty() {
            std::mem::swap(&mut output, &mut self.delay[self.delay_position]);
            self.delay_position = (self.delay_position + 1) % self.delay.len();
        }
        output
    }
}
