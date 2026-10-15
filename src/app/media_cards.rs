//! 문서 안의 소리·영상 카드: 소리는 앱 안에서 재생(재생/멈춤, 재생 막대), 영상은 첫 장면을 보여 주고
//! 누르면 기본 플레이어로 엽니다. 파일 정보(길이·첫 장면)는 뒤에서 읽어 와 기억해 둡니다.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use super::decorations::{DecoOutput, DecoView};
use super::*;
use crate::media::{self, AudioPlayer, AudioState, MediaKind, Probe};

/// 카드에서 사용자가 한 일
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MediaAction {
    /// 소리 재생 / 멈춤
    Toggle(PathBuf),
    /// 소리를 이 위치(0~1)로
    Seek(PathBuf, f32),
    /// 기본 프로그램으로 열기 (영상)
    Open(PathBuf),
    /// 소리 크기 (0~1)
    Volume(f32),
}

enum MediaMeta {
    Pending,
    Audio(Option<Duration>),
    Video(Option<egui::TextureHandle>),
    /// 파일이 없음 (언제 확인했는지: 나중에 생겼을 수도 있어 잠시 뒤 다시 확인)
    Missing(f64),
}

/// 소리 재생기와 파일 정보 모음
pub(super) struct MediaHub {
    pub player: AudioPlayer,
    /// 이번 프레임의 재생 상태
    audio: Option<AudioState>,
    meta: HashMap<PathBuf, MediaMeta>,
    tx: Sender<(PathBuf, Probe)>,
    rx: Receiver<(PathBuf, Probe)>,
    now: f64,
    /// 소리 크기 (설정에서 매 프레임 받아 옴)
    volume: f32,
    /// 소리를 끄기 전 크기 (스피커를 다시 누르면 돌아감)
    unmute_to: f32,
}

impl Default for MediaHub {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            player: AudioPlayer::default(),
            audio: None,
            meta: HashMap::new(),
            tx,
            rx,
            now: 0.0,
            volume: 1.0,
            unmute_to: 1.0,
        }
    }
}

impl MediaHub {
    /// 프레임마다: 뒤에서 읽어 온 정보를 받고, 재생 중이면 막대가 움직이게 다시 그립니다.
    pub fn poll(&mut self, ctx: &egui::Context, volume: f32) {
        self.now = ctx.input(|i| i.time);
        self.volume = volume;
        if volume > 0.0 {
            self.unmute_to = volume;
        }
        self.player.set_volume(volume);
        while let Ok((path, probe)) = self.rx.try_recv() {
            let meta = match probe {
                Probe::Audio(d) => MediaMeta::Audio(d),
                Probe::Video(img) => MediaMeta::Video(img.map(|img| {
                    ctx.load_texture(format!("video-thumb:{}", path.display()), img, egui::TextureOptions::LINEAR)
                })),
                Probe::Missing => MediaMeta::Missing(self.now),
            };
            self.meta.insert(path, meta);
        }
        self.audio = self.player.state();
        if self.audio.as_ref().is_some_and(|a| a.playing) {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// 아직 모르는(또는 없던 지 오래된) 파일이면 `true` → 읽어 오기 요청
    fn needs_probe(&self, path: &Path) -> bool {
        match self.meta.get(path) {
            None => true,
            Some(MediaMeta::Missing(at)) => self.now - at > 3.0,
            Some(_) => false,
        }
    }

    pub fn request(&mut self, ctx: &egui::Context, path: PathBuf) {
        if !self.needs_probe(&path) {
            return;
        }
        self.meta.insert(path.clone(), MediaMeta::Pending);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let probe = media::probe(&path);
            let _ = tx.send((path, probe));
            ctx.request_repaint();
        });
    }

    fn missing(&self, path: &Path) -> bool {
        matches!(self.meta.get(path), Some(MediaMeta::Missing(_)))
    }

    fn audio_of(&self, path: &Path) -> Option<&AudioState> {
        self.audio.as_ref().filter(|a| a.path == path)
    }
}

impl NotepadApp {
    pub(super) fn apply_media_action(&mut self, ctx: &egui::Context, action: MediaAction) {
        let result = match &action {
            MediaAction::Toggle(p) => self.media.player.toggle(p),
            MediaAction::Seek(p, f) => self.media.player.seek(p, *f),
            MediaAction::Open(p) => media::open_with_default_app(p).map_err(|e| e.to_string()),
            MediaAction::Volume(v) => {
                self.settings.audio_volume = v.clamp(0.0, 1.0);
                self.media.player.set_volume(self.settings.audio_volume);
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                if let MediaAction::Open(p) = &action {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.set_status(ctx, tf!("기본 플레이어로 여는 중: {}", name));
                }
                ctx.request_repaint();
            }
            Err(e) => self.set_status(ctx, e),
        }
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// 한 줄로, 넘치면 … 으로 줄인 글자
fn one_line(ui: &Ui, text: &str, size: f32, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(10.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// 스피커 모양. 크기가 0이면 ×, 아니면 크기만큼 소리 물결
fn speaker_icon(center: egui::Pos2, volume: f32, color: Color32, out: &mut Vec<Shape>) {
    let c = center + egui::vec2(-3.0, 0.0);
    let body = vec![
        c + egui::vec2(-5.0, -2.5),
        c + egui::vec2(-2.0, -2.5),
        c + egui::vec2(2.0, -6.0),
        c + egui::vec2(2.0, 6.0),
        c + egui::vec2(-2.0, 2.5),
        c + egui::vec2(-5.0, 2.5),
    ];
    out.push(Shape::convex_polygon(body, color, Stroke::NONE));
    let stroke = Stroke::new(1.4, color);
    if volume <= 0.0 {
        let x = c + egui::vec2(7.5, 0.0);
        out.push(Shape::line_segment([x + egui::vec2(-2.5, -2.5), x + egui::vec2(2.5, 2.5)], stroke));
        out.push(Shape::line_segment([x + egui::vec2(-2.5, 2.5), x + egui::vec2(2.5, -2.5)], stroke));
        return;
    }
    let waves = if volume > 0.66 {
        3
    } else if volume > 0.33 {
        2
    } else {
        1
    };
    for i in 0..waves {
        let r = 4.5 + i as f32 * 3.0;
        let pts: Vec<egui::Pos2> = (0..=8)
            .map(|k| {
                let a = (-0.8 + 1.6 * k as f32 / 8.0) as f64;
                c + egui::vec2(1.5 + r * a.cos() as f32, r * a.sin() as f32)
            })
            .collect();
        out.push(Shape::line(pts, stroke));
    }
}

fn play_icon(center: egui::Pos2, r: f32, playing: bool, color: Color32, out: &mut Vec<Shape>) {
    if playing {
        let (w, h) = (r * 0.28, r * 0.9);
        for dx in [-r * 0.3, r * 0.3] {
            let bar = egui::Rect::from_center_size(center + egui::vec2(dx, 0.0), egui::vec2(w, h));
            out.push(Shape::rect_filled(bar, 1.5, color));
        }
    } else {
        // 눈으로 보기에 가운데로 보이게 살짝 오른쪽으로
        let c = center + egui::vec2(r * 0.12, 0.0);
        let pts =
            vec![c + egui::vec2(-r * 0.4, -r * 0.5), c + egui::vec2(-r * 0.4, r * 0.5), c + egui::vec2(r * 0.5, 0.0)];
        out.push(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
}

/// 소리 카드: [▶] 파일 이름 / 재생 막대 / 시간
pub(super) fn audio_card(ui: &mut Ui, v: &DecoView<'_>, r: egui::Rect, path: &Path, id: Id, out: &mut DecoOutput) {
    if v.media.needs_probe(path) {
        out.media_requests.push(path.to_path_buf());
    }
    let missing = v.media.missing(path);
    let state = v.media.audio_of(path);
    let playing = state.is_some_and(|s| s.playing);
    let duration = state.and_then(|s| s.duration).or(match v.media.meta.get(path) {
        Some(MediaMeta::Audio(d)) => *d,
        _ => None,
    });
    let position = state.map_or(Duration::ZERO, |s| s.position);

    out.fg.push(Shape::rect_filled(r, 10.0, mix(v.bg, v.text, 0.04)));

    // 재생 버튼
    let center = egui::pos2(r.left() + 29.0, r.center().y);
    let btn = egui::Rect::from_center_size(center, egui::vec2(36.0, 36.0));
    let b = ui.interact(btn, id.with("play"), egui::Sense::click());
    let fill = if missing {
        mix(v.bg, v.text, 0.2)
    } else if b.hovered() {
        mix(v.accent, v.text, 0.15)
    } else {
        v.accent
    };
    out.fg.push(Shape::circle_filled(center, 17.0, fill));
    play_icon(center, 13.0, playing, Color32::WHITE, &mut out.fg);
    if !missing && b.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if b.clicked() && !missing {
        out.media_action = Some(MediaAction::Toggle(path.to_path_buf()));
    }

    // 이름과 시간
    let x0 = r.left() + 56.0;
    let x1 = r.right() - 14.0;
    let time = if missing {
        String::new()
    } else {
        match duration {
            Some(d) => format!("{} / {}", media::fmt_time(position), media::fmt_time(d)),
            None if state.is_some() => media::fmt_time(position),
            None => String::new(),
        }
    };
    let time_g = one_line(ui, &time, 12.0, v.muted, 120.0);
    let name_g = one_line(ui, &file_label(path), 13.5, v.text, x1 - x0 - time_g.size().x - 10.0);
    out.fg.push(Shape::galley(egui::pos2(x0, r.top() + 9.0), name_g, v.text));
    out.fg.push(Shape::galley(egui::pos2(x1 - time_g.size().x, r.top() + 10.5), time_g, v.muted));

    if missing {
        let g = one_line(ui, t!("파일을 찾을 수 없습니다"), 12.0, v.muted, x1 - x0);
        out.fg.push(Shape::galley(egui::pos2(x0, r.top() + 32.0), g, v.muted));
        return;
    }

    // 오른쪽 아래: 스피커(누르면 소리 끄기/켜기)와 소리 크기 막대
    let y = r.top() + 40.0;
    let vol_x1 = x1;
    let vol_x0 = x1 - 56.0;
    let spk = egui::pos2(vol_x0 - 16.0, y);
    let sp =
        ui.interact(egui::Rect::from_center_size(spk, egui::vec2(22.0, 20.0)), id.with("mute"), egui::Sense::click());
    let volume = v.media.volume;
    speaker_icon(spk, volume, if sp.hovered() { v.text } else { v.muted }, &mut out.fg);
    if sp.clicked() {
        out.media_action = Some(MediaAction::Volume(if volume > 0.0 { 0.0 } else { v.media.unmute_to.max(0.1) }));
    }
    let vr = egui::Rect::from_x_y_ranges(vol_x0 - 4.0..=vol_x1 + 4.0, y - 8.0..=y + 8.0);
    let vs = ui.interact(vr, id.with("volume"), egui::Sense::click_and_drag());
    let mut shown_volume = volume;
    if (vs.clicked() || vs.dragged_by(egui::PointerButton::Primary))
        && let Some(p) = vs.interact_pointer_pos()
    {
        shown_volume = ((p.x - vol_x0) / (vol_x1 - vol_x0)).clamp(0.0, 1.0);
        out.media_action = Some(MediaAction::Volume(shown_volume));
    }
    if vs.hovered() || sp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let vtrack = egui::Rect::from_x_y_ranges(vol_x0..=vol_x1, y - 1.5..=y + 1.5);
    out.fg.push(Shape::rect_filled(vtrack, 1.5, mix(v.bg, v.text, 0.13)));
    let vdone = egui::Rect::from_x_y_ranges(vol_x0..=vol_x0 + (vol_x1 - vol_x0) * shown_volume, y - 1.5..=y + 1.5);
    out.fg.push(Shape::rect_filled(vdone, 1.5, v.muted));
    if vs.hovered() || vs.dragged() {
        out.fg.push(Shape::circle_filled(egui::pos2(vdone.right(), y), 4.5, v.text));
        vs.on_hover_text(tf!("소리 크기 {}%", (shown_volume * 100.0).round()));
    }

    // 재생 막대 (누르거나 끌어서 이동)
    let x1 = spk.x - 16.0;
    let bar = egui::Rect::from_x_y_ranges(x0..=x1, y - 8.0..=y + 8.0);
    let s = ui.interact(bar, id.with("seek"), egui::Sense::click_and_drag());
    let pointer_frac = s.interact_pointer_pos().map(|p| ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0));
    let mut frac = match duration {
        Some(d) if d > Duration::ZERO => (position.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0),
        _ => 0.0,
    };
    if s.dragged_by(egui::PointerButton::Primary)
        && let Some(f) = pointer_frac
    {
        frac = f;
    }
    if (s.clicked() || s.drag_stopped_by(egui::PointerButton::Primary))
        && let Some(f) = pointer_frac
    {
        out.media_action = Some(MediaAction::Seek(path.to_path_buf(), f));
    }
    if s.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let track = egui::Rect::from_x_y_ranges(x0..=x1, y - 2.0..=y + 2.0);
    out.fg.push(Shape::rect_filled(track, 2.0, mix(v.bg, v.text, 0.13)));
    let done = egui::Rect::from_x_y_ranges(x0..=x0 + (x1 - x0) * frac, y - 2.0..=y + 2.0);
    out.fg.push(Shape::rect_filled(done, 2.0, v.accent));
    if state.is_some() || s.hovered() || s.dragged() {
        out.fg.push(Shape::circle_filled(egui::pos2(done.right(), y), 5.5, v.accent));
    }
}

/// 영상 카드: 첫 장면 + 가운데 ▶ (누르면 기본 플레이어로) + 아래쪽 파일 이름
pub(super) fn video_card(
    ui: &mut Ui,
    v: &DecoView<'_>,
    r: egui::Rect,
    path: &Path,
    id: Id,
    row: &egui::Response,
    out: &mut DecoOutput,
) {
    if v.media.needs_probe(path) {
        out.media_requests.push(path.to_path_buf());
    }
    let missing = v.media.missing(path);
    out.fg.push(Shape::rect_filled(r, 8.0, Color32::from_gray(24)));
    if let Some(MediaMeta::Video(Some(tex))) = v.media.meta.get(path) {
        // 비율을 지키며 카드 안에 맞춤 (남는 곳은 검게)
        let ts = tex.size_vec2();
        let scale = (r.width() / ts.x).min(r.height() / ts.y);
        let fit = egui::Rect::from_center_size(r.center(), ts * scale);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        out.fg.push(Shape::image(tex.id(), fit, uv, Color32::WHITE));
    }

    let center = r.center();
    let radius = (r.height() * 0.16).clamp(16.0, 30.0);
    let btn = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
    let b = ui.interact(btn, id.with("play"), egui::Sense::click());
    if !missing {
        let fill = if b.hovered() { v.accent } else { Color32::from_black_alpha(150) };
        out.fg.push(Shape::circle_filled(center, radius, fill));
        out.fg.push(Shape::circle_stroke(center, radius, Stroke::new(1.5, Color32::from_white_alpha(200))));
        play_icon(center, radius * 0.75, false, Color32::WHITE, &mut out.fg);
        if b.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if b.clicked() || row.double_clicked() {
            out.media_action = Some(MediaAction::Open(path.to_path_buf()));
        }
    }

    // 아래쪽 띠: 파일 이름 (없으면 안내)
    let strip = egui::Rect::from_x_y_ranges(r.x_range(), r.bottom() - 28.0..=r.bottom());
    out.fg.push(Shape::rect_filled(
        strip,
        egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 },
        Color32::from_black_alpha(140),
    ));
    let label =
        if missing { tf!("{}  ·  파일을 찾을 수 없습니다", file_label(path)) } else { file_label(path) };
    let g = one_line(ui, &label, 12.5, Color32::WHITE, r.width() - 24.0);
    out.fg.push(Shape::galley(egui::pos2(r.left() + 12.0, strip.center().y - g.size().y / 2.0), g, Color32::WHITE));
}

/// 그림 줄이 소리·영상이면 그 파일 경로
pub(super) fn media_path(kind: MediaKind, uri: &str) -> Option<PathBuf> {
    if kind == MediaKind::Image {
        return None;
    }
    media::local_link_target(uri, None)
}
