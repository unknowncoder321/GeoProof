"""Click-sound signature: what a click adds on top of the music, as a 16-band spectral shape."""
import numpy as np
SR = 48000
N_BANDS = 16
F_MIN, F_MAX = 500.0, 12000.0
WIN = 512

def band_edges(n_fft=WIN, sr=SR):
    f = np.geomspace(F_MIN, F_MAX, N_BANDS + 1)
    return np.clip(np.round(f * n_fft / sr).astype(int), 1, n_fft // 2)

def signature(x, onset_s, sr=SR):
    """Post-onset spectrum minus pre-onset spectrum (half-wave), pooled into log bands, L2-normalised."""
    i = int(round(onset_s * sr))
    post = x[i - 48: i - 48 + WIN]
    pre = x[i - WIN - 96: i - 96]
    if len(post) < WIN or len(pre) < WIN or i - WIN - 96 < 0:
        return None
    w = np.hanning(WIN)
    a = np.abs(np.fft.rfft(post * w)); b = np.abs(np.fft.rfft(pre * w))
    d = np.maximum(a - b, 0.0)
    e = band_edges()
    v = np.array([np.sqrt(np.sum(d[e[k]:max(e[k + 1], e[k] + 1)] ** 2)) for k in range(N_BANDS)])
    v = np.log1p(30.0 * v)
    n = np.linalg.norm(v)
    return v / n if n > 0 else None

def cosine(a, b):
    return float(np.dot(a, b))
