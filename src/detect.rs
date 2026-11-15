//! Click onset detection. Direct port of proto/detect.py, which is the
//! validated reference; keep the two in sync.
//!
//! Pipeline: 2x biquad high-pass -> log-spectral flux in [f_lo, f_hi] ->
//! adaptive median/MAD threshold -> peak picking with a refractory gap ->
//! sample-domain onset refinement -> click-sound signature (see proto/sig.py).

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

const N_FFT: usize = 512;
const HOP: usize = 128;
const HP_FC: f64 = 1500.0;
const LOG_GAIN: f32 = 100.0;
const THRESH_WIN_S: f64 = 1.0;
const STRIDE: usize = 8;
const PEAK_NEIGH: usize = 3;
const REFINE_BEFORE_S: f64 = 0.012;
const REFINE_AFTER_S: f64 = 0.006;
const ENV_SMOOTH_S: f64 = 0.0004;
const ONSET_FRAC: f32 = 0.20;
const PROGRESS_EVERY: usize = 2048;

pub const SIG_BANDS: usize = 16;
const SIG_WIN: usize = 512;
const SIG_F_MIN: f64 = 500.0;
const SIG_F_MAX: f64 = 12000.0;

/// What a click adds on top of the music: a 16-band spectral shape, unit length.
pub type Signature = [f32; SIG_BANDS];

#[derive(Debug, Clone)]
pub struct Params {
    /// Lower edge of the analysis band (Hz).
    pub f_lo: f32,
    /// Upper edge of the analysis band (Hz).
    pub f_hi: f32,
    /// Threshold in robust standard deviations above the local median.
    pub k: f32,
    /// Minimum time between two detected events (seconds).
    pub min_gap_s: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params { f_lo: 1500.0, f_hi: 6000.0, k: 10.0, min_gap_s: 0.020 }
    }
}

#[derive(Debug, Clone)]
pub struct Onset {
    /// Seconds from the start of the audio stream.
    pub t: f64,
    #[allow(dead_code)]
    pub flux: f32,
    #[allow(dead_code)]
    pub thr: f32,
    /// Peak envelope over the local noise floor; a rough confidence signal.
    pub strength: f32,
    /// Spectral shape of this click, for matching against the user's click sound.
    pub sig: Option<Signature>,
}

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn highpass(fc: f64, sr: f64) -> Self {
        let q = 0.7071;
        let w0 = 2.0 * std::f64::consts::PI * fc / sr;
        let (c, a) = (w0.cos(), w0.sin() / (2.0 * q));
        let a0 = 1.0 + a;
        Biquad {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    // Transposed direct form II, same as scipy.signal.lfilter.
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

fn highpass(x: &[f32], sr: f64) -> Vec<f32> {
    let mut s1 = Biquad::highpass(HP_FC, sr);
    let mut s2 = Biquad::highpass(HP_FC, sr);
    x.iter().map(|&v| s2.process(s1.process(v as f64)) as f32).collect()
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos())
        .collect()
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn log_spectral_flux(
    hp: &[f32],
    sr: f64,
    f_lo: f32,
    f_hi: f32,
    progress: &mut dyn FnMut(f32),
) -> Vec<f32> {
    if hp.len() < N_FFT {
        return Vec::new();
    }
    let n_frames = 1 + (hp.len() - N_FFT) / HOP;
    let k_lo = ((f_lo as f64 * N_FFT as f64 / sr).ceil() as usize).max(1);
    let k_hi = ((f_hi as f64 * N_FFT as f64 / sr).floor() as usize).min(N_FFT / 2);
    if k_hi <= k_lo {
        return vec![0.0; n_frames];
    }

    let window = hann(N_FFT);
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut buf = vec![Complex::new(0.0f32, 0.0f32); N_FFT];
    let mut prev: Vec<f32> = vec![0.0; k_hi - k_lo];
    let mut flux = vec![0.0f32; n_frames];

    for t in 0..n_frames {
        if t % PROGRESS_EVERY == 0 {
            progress(t as f32 / n_frames as f32);
        }
        let seg = &hp[t * HOP..t * HOP + N_FFT];
        for i in 0..N_FFT {
            buf[i] = Complex::new(seg[i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        let mut sum = 0.0f32;
        for (j, k) in (k_lo..k_hi).enumerate() {
            let lg = (LOG_GAIN * buf[k].norm()).ln_1p();
            if t > 0 {
                let d = lg - prev[j];
                if d > 0.0 {
                    sum += d;
                }
            }
            prev[j] = lg;
        }
        flux[t] = sum;
    }
    flux
}

fn local_threshold(flux: &[f32], hop_s: f64, k: f32) -> Vec<f32> {
    let n = flux.len();
    let half = ((THRESH_WIN_S / hop_s / 2.0) as usize).max(1);
    let mut thr = vec![0.0f32; n];
    let mut i = 0;
    while i < n {
        let a = i.saturating_sub(half);
        let b = (i + half + 1).min(n);
        let mut w: Vec<f32> = flux[a..b].to_vec();
        let med = median(&mut w);
        let mut dev: Vec<f32> = w.iter().map(|v| (v - med).abs()).collect();
        let mad = median(&mut dev) * 1.4826 + 1e-9;
        let v = med + k * mad;
        for j in i..(i + STRIDE).min(n) {
            thr[j] = v;
        }
        i += STRIDE;
    }
    thr
}

fn pick_peaks(flux: &[f32], thr: &[f32], hop_s: f64, min_gap_s: f64) -> Vec<usize> {
    let n = flux.len();
    if n <= 2 * PEAK_NEIGH {
        return Vec::new();
    }
    let gap = ((min_gap_s / hop_s) as usize).max(1);
    let mut cand: Vec<usize> = Vec::new();
    for i in PEAK_NEIGH..n - PEAK_NEIGH {
        let local_max = flux[i - PEAK_NEIGH..=i + PEAK_NEIGH]
            .iter()
            .fold(f32::MIN, |m, &v| m.max(v));
        if flux[i] >= local_max && flux[i] > thr[i] {
            cand.push(i);
        }
    }
    cand.sort_by(|&a, &b| flux[b].partial_cmp(&flux[a]).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<usize> = Vec::new();
    for i in cand {
        if kept.iter().all(|&j| i.abs_diff(j) >= gap) {
            kept.push(i);
        }
    }
    kept.sort_unstable();
    kept
}

/// Sample-accurate onset near a flux peak, from the smoothed high-passed envelope.
fn refine(hp: &[f32], frame_idx: usize, sr: f64) -> (f64, f32) {
    let centre = frame_idx * HOP + N_FFT / 2;
    let a = centre.saturating_sub((REFINE_BEFORE_S * sr) as usize);
    let b = (centre + (REFINE_AFTER_S * sr) as usize).min(hp.len());
    if b < a + 32 {
        return (centre as f64 / sr, 0.0);
    }
    let abs: Vec<f32> = hp[a..b].iter().map(|v| v.abs()).collect();
    let k = ((ENV_SMOOTH_S * sr) as usize).max(1);
    let (before, after) = (k / 2, (k - 1) / 2);
    let mut prefix = vec![0.0f32; abs.len() + 1];
    for i in 0..abs.len() {
        prefix[i + 1] = prefix[i] + abs[i];
    }
    let env: Vec<f32> = (0..abs.len())
        .map(|i| {
            let lo = i.saturating_sub(before);
            let hi = (i + after + 1).min(abs.len());
            (prefix[hi] - prefix[lo]) / k as f32
        })
        .collect();

    let mut peak_i = 0;
    for i in 1..env.len() {
        if env[i] > env[peak_i] {
            peak_i = i;
        }
    }
    let floor_len = ((0.003 * sr) as usize).max(8).min(env.len());
    let mut head: Vec<f32> = env[..floor_len].to_vec();
    let floor = median(&mut head);
    let level = (floor * 4.0).max(ONSET_FRAC * env[peak_i]);
    let idx = env[..=peak_i].iter().position(|&v| v > level).unwrap_or(peak_i);
    ((a + idx) as f64 / sr, env[peak_i] / (floor + 1e-9))
}

struct SigCtx {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    edges: [usize; SIG_BANDS + 1],
}

impl SigCtx {
    fn new(sr: f64) -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(SIG_WIN);
        let mut edges = [0usize; SIG_BANDS + 1];
        for k in 0..=SIG_BANDS {
            let f = SIG_F_MIN * (SIG_F_MAX / SIG_F_MIN).powf(k as f64 / SIG_BANDS as f64);
            edges[k] = ((f * SIG_WIN as f64 / sr).round() as usize).clamp(1, SIG_WIN / 2);
        }
        SigCtx { fft, window: hann(SIG_WIN), edges }
    }

    fn magnitudes(&self, seg: &[f32]) -> Vec<f32> {
        let mut buf: Vec<Complex<f32>> = seg
            .iter()
            .zip(self.window.iter())
            .map(|(&s, &w)| Complex::new(s * w, 0.0))
            .collect();
        self.fft.process(&mut buf);
        buf.iter().take(SIG_WIN / 2 + 1).map(|c| c.norm()).collect()
    }

    /// Post-onset spectrum minus pre-onset spectrum (half-wave), pooled into log bands.
    fn at(&self, audio: &[f32], onset_s: f64, sr: f64) -> Option<Signature> {
        let i = (onset_s * sr).round() as usize;
        if i < SIG_WIN + 96 || i + SIG_WIN - 48 > audio.len() {
            return None;
        }
        let post = self.magnitudes(&audio[i - 48..i - 48 + SIG_WIN]);
        let pre = self.magnitudes(&audio[i - SIG_WIN - 96..i - 96]);
        let mut v = [0.0f32; SIG_BANDS];
        for k in 0..SIG_BANDS {
            let lo = self.edges[k];
            let hi = self.edges[k + 1].max(lo + 1).min(post.len());
            let mut e = 0.0f32;
            for b in lo..hi {
                let d = (post[b] - pre[b]).max(0.0);
                e += d * d;
            }
            v[k] = (30.0 * e.sqrt()).ln_1p();
        }
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n <= 1e-9 {
            return None;
        }
        for x in v.iter_mut() {
            *x /= n;
        }
        Some(v)
    }
}

/// Cosine similarity of two unit-length signatures.
pub fn similarity(a: &Signature, b: &Signature) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Average several signatures into one unit-length template.
pub fn mean_signature(sigs: &[Signature]) -> Option<Signature> {
    if sigs.is_empty() {
        return None;
    }
    let mut m = [0.0f32; SIG_BANDS];
    for s in sigs {
        for k in 0..SIG_BANDS {
            m[k] += s[k];
        }
    }
    let n = m.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n <= 1e-9 {
        return None;
    }
    for x in m.iter_mut() {
        *x /= n;
    }
    Some(m)
}

pub fn detect_with_progress(
    audio: &[f32],
    sr: f64,
    p: &Params,
    progress: &mut dyn FnMut(f32),
) -> Vec<Onset> {
    progress(0.0);
    let hp = highpass(audio, sr);
    let flux = {
        let mut flux_progress = |f: f32| progress(0.05 + 0.80 * f);
        log_spectral_flux(&hp, sr, p.f_lo, p.f_hi, &mut flux_progress)
    };
    if flux.is_empty() {
        return Vec::new();
    }
    let hop_s = HOP as f64 / sr;
    let thr = local_threshold(&flux, hop_s, p.k);
    progress(0.88);
    let peaks = pick_peaks(&flux, &thr, hop_s, p.min_gap_s);
    let sig_ctx = SigCtx::new(sr);
    let total = peaks.len().max(1);
    let mut out = Vec::with_capacity(peaks.len());
    for (n, i) in peaks.into_iter().enumerate() {
        let (t, strength) = refine(&hp, i, sr);
        out.push(Onset { t, flux: flux[i], thr: thr[i], strength, sig: sig_ctx.at(audio, t, sr) });
        if n % 64 == 0 {
            progress(0.88 + 0.12 * n as f32 / total as f32);
        }
    }
    progress(1.0);
    out
}

#[allow(dead_code)]
pub fn detect(audio: &[f32], sr: f64, p: &Params) -> Vec<Onset> {
    detect_with_progress(audio, sr, p, &mut |_f: f32| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic noise so the test needs no extra crates.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
    }

    #[test]
    fn finds_a_click_in_quiet_noise() {
        let sr = 48_000.0;
        let mut rng = Lcg(7);
        let mut x: Vec<f32> = (0..(sr as usize * 3)).map(|_| 0.002 * rng.next()).collect();
        let click_at = 1.5_f64;
        let i0 = (click_at * sr) as usize;
        for n in 0..400 {
            let env = (-(n as f32) / 80.0).exp();
            x[i0 + n] += 0.4 * env * rng.next();
        }
        let found = detect(&x, sr, &Params::default());
        assert_eq!(found.len(), 1, "expected exactly one click, got {}", found.len());
        assert!((found[0].t - click_at).abs() < 0.003, "onset off by {} s", found[0].t - click_at);
        assert!(found[0].sig.is_some(), "click should carry a signature");
    }

    #[test]
    fn silence_has_no_clicks() {
        let x = vec![0.0f32; 48_000 * 2];
        assert!(detect(&x, 48_000.0, &Params::default()).is_empty());
    }

    #[test]
    fn signature_matches_itself() {
        let mut a = [0.0f32; SIG_BANDS];
        a[3] = 3.0;
        a[4] = 4.0;
        let t = mean_signature(&[a]).unwrap();
        assert!((similarity(&t, &t) - 1.0).abs() < 1e-5);
    }
}
