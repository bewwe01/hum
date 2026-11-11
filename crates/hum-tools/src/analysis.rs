//! Measurements for rendered audio: peak, RMS, spectral centroid, tail.

/// In-place radix-2 FFT. `re.len()` must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let (w_im, w_re) = angle.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut c_re, mut c_im) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let t_re = re[b] as f64 * c_re - im[b] as f64 * c_im;
                let t_im = re[b] as f64 * c_im + im[b] as f64 * c_re;
                re[b] = (re[a] as f64 - t_re) as f32;
                im[b] = (im[a] as f64 - t_im) as f32;
                re[a] = (re[a] as f64 + t_re) as f32;
                im[a] = (im[a] as f64 + t_im) as f32;
                let next_re = c_re * w_re - c_im * w_im;
                c_im = c_re * w_im + c_im * w_re;
                c_re = next_re;
            }
        }
        len <<= 1;
    }
}

/// Magnitude spectrum (bins 0..n/2) of a mono signal, Hann-windowed and
/// zero-padded or truncated to `n` (a power of two).
pub fn spectrum(mono: &[f32], n: usize) -> Vec<f32> {
    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];
    let len = mono.len().min(n);
    for i in 0..len {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / len.max(2) as f32).cos();
        re[i] = mono[i] * w;
    }
    fft(&mut re, &mut im);
    (0..n / 2)
        .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
        .collect()
}

/// Magnitude-weighted mean frequency in Hz ("brightness").
pub fn spectral_centroid(mono: &[f32], sample_rate: f32) -> f32 {
    let n = 32_768usize;
    // Analyse the loudest stretch so decaying sounds are measured fairly.
    let window = mono.len().min(n);
    let mag = spectrum(&mono[..window], n);
    let mut num = 0.0f64;
    let mut den = 0.0f64;
    for (i, m) in mag.iter().enumerate().skip(1) {
        let f = i as f64 * sample_rate as f64 / n as f64;
        num += f * *m as f64;
        den += *m as f64;
    }
    if den <= 1.0e-12 {
        0.0
    } else {
        (num / den) as f32
    }
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples
        .iter()
        .map(|s| (*s as f64) * (*s as f64))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt() as f32
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-10).log10()
}

/// Mix interleaved stereo down to mono.
pub fn to_mono(stereo: &[f32]) -> Vec<f32> {
    stereo
        .chunks_exact(2)
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect()
}

/// Index of the last frame whose magnitude reaches `threshold`, plus one.
pub fn last_above(stereo: &[f32], threshold: f32) -> usize {
    stereo
        .chunks_exact(2)
        .rposition(|f| f[0].abs() >= threshold || f[1].abs() >= threshold)
        .map_or(0, |i| i + 1)
}

/// Largest sample-to-sample jump (a click detector).
pub fn max_step(stereo: &[f32]) -> f32 {
    let mut worst = 0.0f32;
    for w in stereo.chunks_exact(2).collect::<Vec<_>>().windows(2) {
        worst = worst
            .max((w[1][0] - w[0][0]).abs())
            .max((w[1][1] - w[0][1]).abs());
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_sine() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 37.0 * i as f32 / n as f32).sin())
            .collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let mags: Vec<f32> = (0..n / 2)
            .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
            .collect();
        let (best, _) = mags
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert_eq!(best, 37);
        assert!((mags[37] - n as f32 / 2.0).abs() < 1.0);
    }

    #[test]
    fn centroid_of_a_sine_is_its_frequency() {
        let sr = 48_000.0;
        let mono: Vec<f32> = (0..48_000)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / sr).sin())
            .collect();
        let c = spectral_centroid(&mono, sr);
        assert!((c - 1000.0).abs() < 40.0, "{c}");
    }

    #[test]
    fn level_helpers() {
        assert_eq!(peak(&[0.1, -0.7, 0.3]), 0.7);
        assert!((rms(&[1.0, -1.0, 1.0, -1.0]) - 1.0).abs() < 1e-6);
        assert!((db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(last_above(&[0.0, 0.0, 0.5, 0.0, 0.0, 0.0], 0.1), 2);
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5]), vec![0.5, 0.5]);
    }
}
