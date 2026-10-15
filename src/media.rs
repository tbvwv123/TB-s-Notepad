//! 소리·영상: 파일 종류 판별, 앱 안 소리 재생기, 영상 첫 장면 미리보기, 로컬 파일 링크 해석.
//!
//! 문서에는 그림과 같은 문법으로 넣습니다: `![설명](media/노래.mp3)`, `![설명](media/영상.mp4)`.
//! 소리는 앱 안에서 바로 재생하고(rodio), 영상은 카드로 보여 주다가 누르면 기본 플레이어로 엽니다.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
}

pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac"];
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "webm", "mkv", "mov", "avi", "wmv"];

fn extension_of(s: &str) -> String {
    let s = s.trim().trim_start_matches('<').trim_end_matches('>');
    let s = s.split(['?', '#']).next().unwrap_or(s);
    Path::new(s).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

/// 경로(또는 링크 주소)의 확장자로 그림 / 소리 / 영상을 가립니다. 모르는 확장자는 그림으로 봅니다.
pub fn kind_of(dest: &str) -> MediaKind {
    let ext = extension_of(dest);
    if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        MediaKind::Audio
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        MediaKind::Video
    } else {
        MediaKind::Image
    }
}

pub fn is_audio_or_video(path: &Path) -> bool {
    kind_of(&path.to_string_lossy()) != MediaKind::Image
}

/// 링크 주소가 이 PC의 파일·폴더를 가리키면 그 경로.
/// `https://` 같은 웹 주소는 `None`, `file:///C:/…` · `C:\…` · 문서 기준 상대 경로는 경로로 풉니다.
pub fn local_link_target(url: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
    let url = url.trim().trim_start_matches('<').trim_end_matches('>');
    if url.is_empty() || url.starts_with('#') {
        return None;
    }
    let path = if let Some(rest) = url.strip_prefix("file:///").or_else(|| url.strip_prefix("file://")) {
        crate::markdown::percent_decode(rest)
    } else if url.contains("://") || url.starts_with("mailto:") {
        return None;
    } else {
        crate::markdown::percent_decode(url)
    };
    let p = PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if p.is_absolute() { Some(p) } else { base_dir.map(|dir| dir.join(p)) }
}

/// 카드에 보여 줄 파일 정보 (뒤에서 읽어 옴)
pub enum Probe {
    Audio(Option<Duration>),
    /// 영상 첫 장면 (셸이 형식을 모르면 `None`)
    Video(Option<egui::ColorImage>),
    Missing,
}

pub fn probe(path: &Path) -> Probe {
    if !path.is_file() {
        return Probe::Missing;
    }
    match kind_of(&path.to_string_lossy()) {
        MediaKind::Audio => Probe::Audio(audio_duration(path)),
        MediaKind::Video => Probe::Video(shell_thumbnail(path, 640)),
        MediaKind::Image => Probe::Missing,
    }
}

/// 소리 길이 (머리 정보만 읽음)
pub fn audio_duration(path: &Path) -> Option<Duration> {
    let decoder = rodio::Decoder::try_from(std::fs::File::open(path).ok()?).ok()?;
    rodio::Source::total_duration(&decoder)
}

/// `m:ss` (한 시간이 넘으면 `h:mm:ss`)
pub fn fmt_time(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

// ---------------------------------------------------------------------------
// 소리 재생기

/// 한 번에 한 곡만 재생하는 간단한 재생기. 소리 장치는 처음 재생할 때 엽니다.
pub struct AudioPlayer {
    device: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    current: Option<PathBuf>,
    duration: Option<Duration>,
    /// 소리 크기 (0~1, 막대 위치)
    volume: f32,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self { device: None, player: None, current: None, duration: None, volume: 1.0 }
    }
}

/// 막대 위치(0~1) → 실제 증폭. 귀는 작은 소리 차이에 민감해서 제곱으로 (막대 절반 ≈ 1/4 크기)
fn gain(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0).powi(2)
}

/// 카드에 보여 줄 지금 상태
#[derive(Clone, Debug, PartialEq)]
pub struct AudioState {
    pub path: PathBuf,
    pub playing: bool,
    pub position: Duration,
    pub duration: Option<Duration>,
}

impl AudioPlayer {
    /// 재생 중인 곡이면 멈춤/계속, 다른 곡이면 처음부터 재생
    pub fn toggle(&mut self, path: &Path) -> Result<(), String> {
        if self.current.as_deref() == Some(path)
            && let Some(p) = &self.player
            && !p.empty()
        {
            if p.is_paused() {
                p.play()
            } else {
                p.pause()
            }
            return Ok(());
        }
        self.start(path, Duration::ZERO)
    }

    fn start(&mut self, path: &Path, from: Duration) -> Result<(), String> {
        if self.device.is_none() {
            let mut device = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|e| format!("소리 장치를 열 수 없습니다: {e}"))?;
            device.log_on_drop(false);
            self.device = Some(device);
        }
        let file = std::fs::File::open(path).map_err(|e| format!("소리 파일을 열 수 없습니다: {e}"))?;
        let decoder = rodio::Decoder::try_from(file).map_err(|e| format!("이 소리 형식은 재생할 수 없습니다: {e}"))?;
        self.duration = rodio::Source::total_duration(&decoder);
        let player = rodio::Player::connect_new(self.device.as_ref().map(|d| d.mixer()).ok_or("소리 장치 없음")?);
        player.set_volume(gain(self.volume));
        player.append(decoder);
        if from > Duration::ZERO {
            let _ = player.try_seek(from);
        }
        self.player = Some(player);
        self.current = Some(path.to_path_buf());
        Ok(())
    }

    /// `fraction` (0~1) 위치로 이동. 다른 곡이면 그 곡을 그 위치부터 재생합니다.
    pub fn seek(&mut self, path: &Path, fraction: f32) -> Result<(), String> {
        let Some(total) = self.duration.filter(|_| self.current.as_deref() == Some(path)) else {
            self.start(path, Duration::ZERO)?;
            return Ok(());
        };
        let target = total.mul_f32(fraction.clamp(0.0, 1.0));
        match &self.player {
            // 끝까지 재생해서 비었으면 다시 넣고 이동
            Some(p) if !p.empty() => p.try_seek(target).map_err(|e| format!("이동할 수 없습니다: {e}")),
            _ => self.start(path, target),
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        if (volume - self.volume).abs() < f32::EPSILON {
            return;
        }
        self.volume = volume;
        if let Some(p) = &self.player {
            p.set_volume(gain(volume));
        }
    }

    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.current = None;
    }

    pub fn state(&self) -> Option<AudioState> {
        let path = self.current.clone()?;
        let p = self.player.as_ref()?;
        let finished = p.empty();
        Some(AudioState {
            path,
            playing: !finished && !p.is_paused(),
            position: if finished { Duration::ZERO } else { p.get_pos() },
            duration: self.duration,
        })
    }
}

// ---------------------------------------------------------------------------
// 영상 첫 장면 (Windows 셸 썸네일)

/// 탐색기가 보여 주는 것과 같은 미리보기 그림. 셸이 해당 형식을 모르면 `None`.
#[cfg(windows)]
pub fn shell_thumbnail(path: &Path, size: u32) -> Option<egui::ColorImage> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
    };
    use windows::core::HSTRING;

    // SAFETY: COM 은 이 스레드에서 열고 닫으며, GDI 객체는 쓰고 나서 모두 돌려줍니다.
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| {
            let factory: IShellItemImageFactory =
                SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
            let side = size as i32;
            let hbm = factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK).ok()?;
            let mut bm = BITMAP::default();
            GetObjectW(HGDIOBJ(hbm.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
            let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // 위에서 아래로
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (w * h * 4).max(0) as usize];
            let dc = CreateCompatibleDC(None);
            let lines = GetDIBits(dc, hbm, 0, h as u32, Some(bgra.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
            let _ = DeleteDC(dc);
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            if lines == 0 || w <= 0 || h <= 0 {
                return None;
            }
            // BGRA → RGBA. 셸 그림은 알파가 0인 경우가 많아 불투명으로 맞춤
            let all_clear = bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 0);
            for px in bgra.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
                if all_clear {
                    px[3] = 255;
                }
            }
            Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &bgra))
        })();
        if com.is_ok() {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn shell_thumbnail(_path: &Path, _size: u32) -> Option<egui::ColorImage> {
    None
}

/// 파일을 기본 프로그램으로 엽니다 (영상 → 동영상 플레이어).
pub fn open_with_default_app(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, w};
        // SAFETY: 문자열들은 호출이 끝날 때까지 살아 있습니다.
        let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL) };
        // ShellExecute 는 성공하면 32보다 큰 값을 돌려줍니다.
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("열 수 없습니다 (코드 {})", r.0 as isize)))
        }
    }
    #[cfg(not(windows))]
    {
        let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(cmd).arg(path).spawn().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_of("media/노래.MP3"), MediaKind::Audio);
        assert_eq!(kind_of("<my clip.mp4>"), MediaKind::Video);
        assert_eq!(kind_of("https://x/a.webm?t=3"), MediaKind::Video);
        assert_eq!(kind_of("images/a.png"), MediaKind::Image);
        assert_eq!(kind_of("이름없음"), MediaKind::Image);
    }

    #[test]
    fn local_links() {
        let base = Path::new("C:/notes/일기");
        assert_eq!(local_link_target("https://example.com", Some(base)), None);
        assert_eq!(local_link_target("mailto:a@b.c", Some(base)), None);
        assert_eq!(local_link_target("#제목", Some(base)), None);
        let p = local_link_target("../자료/보고서%201.pdf", Some(base)).unwrap();
        assert!(p.ends_with(Path::new("자료").join("보고서 1.pdf")), "{p:?}");
        assert!(p.starts_with(base));
        let abs = local_link_target("file:///C:/Users/Public", Some(base)).unwrap();
        assert!(abs.is_absolute());
        assert_eq!(local_link_target("<C:\\Data\\사진>", None), Some(PathBuf::from("C:\\Data\\사진")));
        assert_eq!(local_link_target("상대/경로", None), None);
    }

    #[test]
    fn time_format() {
        assert_eq!(fmt_time(Duration::from_secs(65)), "1:05");
        assert_eq!(fmt_time(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn volume_curve() {
        assert_eq!(gain(0.0), 0.0);
        assert_eq!(gain(1.0), 1.0);
        assert_eq!(gain(0.5), 0.25);
        assert_eq!(gain(3.0), 1.0);
    }

    /// 소리 장치 없이도 디코더가 길이를 읽는지 (wav 를 직접 만들어 확인)
    #[test]
    fn decodes_wav_duration() {
        let rate = 8000u32;
        let samples = rate as usize; // 1초
        let mut wav = Vec::new();
        let data_len = (samples * 2) as u32;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, samples * 2));
        let dir = std::env::temp_dir().join(format!("tbn_wav_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("beep.wav");
        std::fs::write(&f, &wav).unwrap();
        let d = audio_duration(&f).unwrap();
        assert!(matches!(probe(&f), Probe::Audio(Some(_))));
        assert!(matches!(probe(&dir.join("없음.mp3")), Probe::Missing));
        assert!((d.as_secs_f32() - 1.0).abs() < 0.05, "{d:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 셸 미리보기 경로가 동작하는지 (그림 파일로 확인; 영상도 같은 경로)
    #[cfg(windows)]
    #[test]
    fn shell_thumbnail_of_png() {
        let png = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("logo.png");
        let img = shell_thumbnail(&png, 128).expect("셸 미리보기");
        assert!(img.size[0] > 0 && img.size[1] > 0);
    }
}
