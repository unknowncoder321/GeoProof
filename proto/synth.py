import numpy as np
from scipy.signal import lfilter, butter, sosfilt

SR = 48000

def music(dur, rng):
    n = int(dur * SR); t = np.arange(n) / SR
    x = np.zeros(n)
    # bass + saw lead with plenty of HF harmonics
    for f, amp in [(55, .35), (110, .2)]:
        x += amp * np.sin(2 * np.pi * f * t)
    lead = np.zeros(n)
    notes = [220, 261.6, 329.6, 392, 440, 523.3]
    seg = int(0.25 * SR)
    for i in range(0, n, seg):
        f = notes[rng.integers(len(notes))]
        tt = np.arange(min(seg, n - i)) / SR
        lead[i:i + len(tt)] = ((2 * ((f * tt) % 1) - 1) * np.exp(-tt * 6))
    x += 0.25 * lead
    # kicks
    for tk in np.arange(0, dur, 0.5):
        i = int(tk * SR); m = min(int(0.2 * SR), n - i)
        tt = np.arange(m) / SR
        x[i:i + m] += 0.6 * np.sin(2 * np.pi * (45 + 100 * np.exp(-tt * 30)) * tt) * np.exp(-tt * 18)
    # loud hi-hats (the hard false-positive source), 16ths with noise bursts
    sos = butter(4, 6000, 'hp', fs=SR, output='sos')
    for th in np.arange(0.125, dur, 0.25):
        i = int(th * SR); m = min(int(0.05 * SR), n - i)
        tt = np.arange(m) / SR
        burst = sosfilt(sos, rng.standard_normal(m)) * np.exp(-tt * 90)
        x[i:i + m] += 0.18 * burst
    x += 0.003 * rng.standard_normal(n)
    return x

def click(rng, amp):
    m = int(0.012 * SR); t = np.arange(m) / SR
    n = rng.standard_normal(m)
    sos = butter(2, [1800, 9000], 'bp', fs=SR, output='sos')
    nb = sosfilt(sos, n)
    ring = np.sin(2 * np.pi * rng.uniform(2800, 4200) * t)
    env = np.exp(-t / 0.0018)
    c = (nb * 0.7 + ring * 0.5) * env
    return amp * c / np.max(np.abs(c))

def make(dur, n_clicks, snr_db, seed):
    rng = np.random.default_rng(seed)
    bg = music(dur, rng)
    rms = np.sqrt(np.mean(bg ** 2))
    amp = rms * 10 ** (snr_db / 20) * 2.0
    times = []
    t = 1.0
    while len(times) < n_clicks and t < dur - 1:
        times.append(t)
        t += rng.uniform(0.08, 0.7)
    x = bg.copy()
    for tc in times:
        i = int(round(tc * SR)); c = click(rng, amp)
        m = min(len(c), len(x) - i)
        x[i:i + m] += c[:m]
    x = x / (np.max(np.abs(x)) * 1.05)
    return x.astype(np.float32), np.array(times)
