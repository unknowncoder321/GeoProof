import numpy as np, subprocess
from scipy.io import wavfile
import synth, detect, test, sig
SR=48000
def auc(pos, neg):
    pos=np.array(pos); neg=np.array(neg)
    return float(np.mean([(p>n)+0.5*(p==n) for p in pos for n in neg]))
for snr in [0, 6, -6]:
    for seed in [1, 2]:
        x, truth = synth.make(40, 150, snr, seed)
        wavfile.write("/tmp/s.wav", SR, x); test.encode("/tmp/s.wav", "/tmp/s.mp4", 60, 40)
        y = test.decode("/tmp/s.mp4")
        detect.THRESH_K = 6.0                        # Sensitive: lots of candidates
        det = detect.detect(y)
        t = np.array([d["t"] for d in det])
        is_click = np.array([np.min(np.abs(truth - tt)) <= 0.012 for tt in t])
        sigs = [sig.signature(y, tt) for tt in t]
        ok = [s is not None for s in sigs]
        idx_pos = [i for i in range(len(t)) if ok[i] and is_click[i]]
        idx_neg = [i for i in range(len(t)) if ok[i] and not is_click[i]]
        # "user confirms" 5 clicks -> template
        learn = idx_pos[:5]; rest_pos = idx_pos[5:]
        tmpl = np.mean([sigs[i] for i in learn], axis=0); tmpl /= np.linalg.norm(tmpl)
        sp = [sig.cosine(tmpl, sigs[i]) for i in rest_pos]; sn = [sig.cosine(tmpl, sigs[i]) for i in idx_neg]
        print(f"snr{snr:+3d} seed{seed}: cand={len(t)} true={len(idx_pos)} false={len(idx_neg)} | cos true med={np.median(sp):.3f} false med={np.median(sn):.3f} AUC={auc(sp,sn):.3f}", end=" | ")
        for th in [0.90, 0.95]:
            keep_t = sum(s >= th for s in sp); keep_f = sum(s >= th for s in sn)
            print(f"th{th}: kept {keep_t}/{len(sp)} true, {keep_f}/{len(sn)} false", end="  ")
        print()
