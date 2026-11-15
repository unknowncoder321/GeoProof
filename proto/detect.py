"""GeoProof reference click-onset detector (prototype for the Rust port).

Pipeline: mono f32 audio -> 2x biquad high-pass -> log-spectral flux (>= F_LO) ->
adaptive median/MAD threshold -> peak pick with refractory gap -> sample-domain
onset refinement. Every constant here is mirrored in src/detect.rs.
"""
import numpy as np
from scipy.signal import lfilter

SR = 48000
N_FFT = 512
HOP = 128
F_LO = 1500.0
F_HI = 6000.0
HP_FC = 1500.0
LOG_GAIN = 100.0
THRESH_WIN_S = 1.0      # local statistics window
THRESH_K = 10.0         # MAD multiples above median
FLUX_FLOOR = 0.0        # absolute floor, relative to median-based scale, set by K
MIN_GAP_S = 0.020       # refractory gap between events
PEAK_NEIGH = 3          # local-max neighbourhood (frames)
REFINE_BEFORE_S = 0.012
REFINE_AFTER_S = 0.006
ENV_SMOOTH_S = 0.0004
ONSET_FRAC = 0.20


def biquad_hp(fc, sr, q=0.7071):
    w0 = 2 * np.pi * fc / sr
    c, a = np.cos(w0), np.sin(w0) / (2 * q)
    b = np.array([(1 + c) / 2, -(1 + c), (1 + c) / 2])
    den = np.array([1 + a, -2 * c, 1 - a])
    return b / den[0], den / den[0]


def highpass(x, sr=SR, fc=HP_FC):
    b, a = biquad_hp(fc, sr)
    return lfilter(b, a, lfilter(b, a, x))


def log_spectral_flux(x, sr=SR):
    win = np.hanning(N_FFT).astype(np.float32)
    n_frames = 1 + (len(x) - N_FFT) // HOP
    k_lo = int(np.ceil(F_LO * N_FFT / sr))
    k_hi = int(np.floor(F_HI * N_FFT / sr))
    prev = None
    flux = np.zeros(n_frames, dtype=np.float64)
    for t in range(n_frames):
        seg = x[t * HOP: t * HOP + N_FFT] * win
        mag = np.abs(np.fft.rfft(seg))[k_lo:k_hi]
        lg = np.log1p(LOG_GAIN * mag)
        if prev is not None:
            flux[t] = np.maximum(lg - prev, 0).sum()
        prev = lg
    return flux


STRIDE = 8


def local_threshold(flux, sr=SR):
    """Median/MAD threshold, computed every STRIDE frames and held (keeps long videos fast)."""
    hop_s = HOP / sr
    half = max(1, int(THRESH_WIN_S / hop_s / 2))
    thr = np.zeros_like(flux)
    for i in range(0, len(flux), STRIDE):
        w = flux[max(0, i - half): i + half + 1]
        med = np.median(w)
        mad = np.median(np.abs(w - med)) * 1.4826 + 1e-9
        thr[i:i + STRIDE] = med + THRESH_K * mad
    return thr


def pick_peaks(flux, thr, sr=SR):
    hop_s = HOP / sr
    gap = max(1, int(MIN_GAP_S / hop_s))
    cand = []
    for i in range(PEAK_NEIGH, len(flux) - PEAK_NEIGH):
        w = flux[i - PEAK_NEIGH: i + PEAK_NEIGH + 1]
        if flux[i] >= w.max() and flux[i] > thr[i]:
            cand.append(i)
    # keep strongest within refractory gap
    cand.sort(key=lambda i: -flux[i])
    kept = []
    for i in cand:
        if all(abs(i - j) >= gap for j in kept):
            kept.append(i)
    return sorted(kept)


def refine(hp, frame_idx, sr=SR):
    """Sample-accurate onset near a flux peak, using the high-passed envelope."""
    centre = frame_idx * HOP + N_FFT // 2
    a = max(0, centre - int(REFINE_BEFORE_S * sr))
    b = min(len(hp), centre + int(REFINE_AFTER_S * sr))
    env = np.abs(hp[a:b])
    k = max(1, int(ENV_SMOOTH_S * sr))
    env = np.convolve(env, np.ones(k) / k, mode="same")
    peak_i = int(np.argmax(env))
    floor = np.median(env[: max(8, int(0.003 * sr))])
    level = max(floor * 4.0, ONSET_FRAC * env[peak_i])
    idx = np.nonzero(env[: peak_i + 1] > level)[0]
    onset = a + (int(idx[0]) if len(idx) else peak_i)
    strength = float(env[peak_i] / (floor + 1e-9))
    return onset / sr, strength


def detect(x, sr=SR):
    x = np.asarray(x, dtype=np.float64)
    hp = highpass(x, sr)
    flux = log_spectral_flux(hp.astype(np.float32), sr)
    thr = local_threshold(flux, sr)
    peaks = pick_peaks(flux, thr, sr)
    out = []
    for p in peaks:
        t, s = refine(hp, p, sr)
        out.append({"t": t, "flux": float(flux[p]), "thr": float(thr[p]), "strength": s})
    return out
