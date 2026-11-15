import subprocess, sys, json, numpy as np
from scipy.io import wavfile
import synth, detect

SR = 48000

def decode(path):
    r = subprocess.run(["ffmpeg","-v","error","-i",path,"-vn","-ac","1","-ar",str(SR),"-f","f32le","-"],
                       capture_output=True, check=True)
    return np.frombuffer(r.stdout, dtype=np.float32)

def encode(wav, mp4, fps=60, dur=40):
    subprocess.run(["ffmpeg","-y","-v","error","-f","lavfi","-i",f"color=c=black:s=320x180:r={fps}:d={dur}",
                    "-i",wav,"-c:v","libx264","-preset","ultrafast","-c:a","aac","-b:a","128k","-shortest",mp4],check=True)

def match(truth, det, tol=0.012):
    det_t = np.array([d["t"] for d in det]); used = set(); errs = []
    for tt in truth:
        if len(det_t)==0: break
        j = int(np.argmin(np.abs(det_t - tt)))
        if abs(det_t[j]-tt) <= tol and j not in used:
            used.add(j); errs.append(det_t[j]-tt)
    tp = len(errs); fn = len(truth)-tp; fp = len(det_t)-tp
    return tp, fn, fp, np.array(errs)

def run(snr, seed, via, dur=40, n=150):
    x, truth = synth.make(dur, n, snr, seed)
    wavfile.write("/tmp/t.wav", SR, x)
    if via == "wav":
        y = decode("/tmp/t.wav")
    else:
        encode("/tmp/t.wav", "/tmp/t.mp4", 60, dur); y = decode("/tmp/t.mp4")
    det = detect.detect(y)
    tp, fn, fp, e = match(truth, det)
    ms = e*1000
    return dict(snr=snr, via=via, truth=len(truth), tp=tp, fn=fn, fp=fp,
                recall=round(tp/len(truth),3), precision=round(tp/max(1,tp+fp),3),
                err_med_ms=round(float(np.median(ms)),2), err_p95_abs_ms=round(float(np.percentile(np.abs(ms),95)),2),
                err_std_ms=round(float(np.std(ms)),2))

if __name__ == "__main__":
    for via in ["wav","aac"]:
        for snr in [-6, 0, 6]:
            print(json.dumps(run(snr, 1, via)))
