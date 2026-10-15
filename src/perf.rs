//! 개발용 성능 기록. 환경 변수 `TB_NOTEPAD_PERF=<파일 경로>` 로 켜면
//! 프레임 시간 · 문서 배치 시간 · 검색 시간 등을 모았다가 끝날 때 그 파일에 요약을 씁니다.
//! 켜지 않으면 시간을 재지 않으므로 평소 성능에는 영향이 없습니다.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Perf {
    out: PathBuf,
    start: Instant,
    samples: Mutex<BTreeMap<&'static str, Vec<Duration>>>,
    notes: Mutex<Vec<(String, String)>>,
}

fn perf() -> Option<&'static Perf> {
    static P: OnceLock<Option<Perf>> = OnceLock::new();
    P.get_or_init(|| {
        let out = std::env::var_os("TB_NOTEPAD_PERF")?;
        Some(Perf {
            out: out.into(),
            start: Instant::now(),
            samples: Mutex::new(BTreeMap::new()),
            notes: Mutex::new(Vec::new()),
        })
    })
    .as_ref()
}

/// 프로그램 시작 시각을 정합니다 (main 맨 앞에서)
pub fn init() {
    let _ = perf();
}

pub fn enabled() -> bool {
    perf().is_some()
}

pub fn record(name: &'static str, d: Duration) {
    if let Some(p) = perf()
        && let Ok(mut m) = p.samples.lock()
    {
        m.entry(name).or_default().push(d);
    }
}

/// 각본 단계별 프레임 시간 (`frame:typing` 등)
pub fn record_phase(d: Duration) {
    if Script::active() {
        let name: &'static str = match phase() {
            "load" => "frame:load",
            "jump_to_end" => "frame:jump_to_end",
            "typing" => "frame:typing",
            "enter" => "frame:enter",
            "jump_to_top" => "frame:jump_to_top",
            "scroll" => "frame:scroll",
            "search_typing" => "frame:search_typing",
            _ => "frame:idle",
        };
        record(name, d);
    }
}

/// `f` 가 걸린 시간을 `name` 으로 기록합니다 (꺼져 있으면 그냥 실행)
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let t = Instant::now();
    let r = f();
    record(name, t.elapsed());
    r
}

/// 한 번만 남기는 값 (예: 첫 프레임까지 걸린 시간)
pub fn note(key: &str, value: impl Into<String>) {
    if let Some(p) = perf()
        && let Ok(mut n) = p.notes.lock()
        && !n.iter().any(|(k, _)| k == key)
    {
        n.push((key.to_owned(), value.into()));
    }
}

/// 마지막 값으로 덮어쓰는 기록 (예: 마지막 검색 결과 수)
pub fn set_note(key: &str, value: impl Into<String>) {
    if let Some(p) = perf()
        && let Ok(mut n) = p.notes.lock()
    {
        n.retain(|(k, _)| k != key);
        n.push((key.to_owned(), value.into()));
    }
}

/// 프로그램 시작부터 지금까지
pub fn since_start() -> Option<Duration> {
    perf().map(|p| p.start.elapsed())
}

/// (지금, 최대) 작업 메모리 MB
fn memory_mb() -> Option<(f64, f64)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: 지금 프로세스의 핸들과 크기를 맞춘 구조체
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (ok != 0).then(|| (c.WorkingSetSize as f64 / 1048576.0, c.PeakWorkingSetSize as f64 / 1048576.0))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 모은 기록을 파일로 씁니다 (프로그램이 끝날 때)
pub fn write_report() {
    let Some(p) = perf() else { return };
    let mut out = String::new();
    out.push_str(&format!("TB's Notepad {} perf report\n", env!("CARGO_PKG_VERSION")));
    if let Ok(n) = p.notes.lock() {
        for (k, v) in n.iter() {
            out.push_str(&format!("{k}: {v}\n"));
        }
    }
    if let Some((now, peak)) = memory_mb() {
        out.push_str(&format!("memory_mb: now {now:.1} / peak {peak:.1}\n"));
    }
    out.push_str("\nname                 count     mean      p50      p95      p99      max     total (ms)\n");
    if let Ok(m) = p.samples.lock() {
        for (name, v) in m.iter() {
            let mut v = v.clone();
            v.sort();
            let pick = |q: f64| ms(v[((v.len() - 1) as f64 * q).round() as usize]);
            let total: Duration = v.iter().sum();
            out.push_str(&format!(
                "{name:<18} {:>7} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>9.1}\n",
                v.len(),
                ms(total) / v.len() as f64,
                pick(0.5),
                pick(0.95),
                pick(0.99),
                ms(*v.last().unwrap_or(&Duration::ZERO)),
                ms(total),
            ));
        }
    }
    let _ = std::fs::write(&p.out, out);
}

// ---------------------------------------------------------------------------
// 자동 입력 각본: `TB_NOTEPAD_PERF_SCRIPT=1` 이면 문서를 연 뒤 egui 에 입력 이벤트를 직접 넣어
// (OS 키보드·마우스를 쓰지 않으므로 다른 프로그램에 입력이 가지 않음)
// 끝으로 이동 → 글자 40개 입력 → Enter 10번 → 맨 위로 → 휠 스크롤 40번 → 잠시 쉼 → 창 닫기.
// 단계마다 프레임 시간을 따로 기록합니다 (`frame:typing` 등).

static PHASE: Mutex<&'static str> = Mutex::new("load");

pub fn phase() -> &'static str {
    PHASE.lock().map(|p| *p).unwrap_or("load")
}

fn set_phase(p: &'static str) {
    if let Ok(mut cur) = PHASE.lock() {
        *cur = p;
    }
}

#[derive(Default)]
pub struct Script {
    frame: u32,
    /// 검색 각본: 검색어를 다 친 때
    typed_at: Option<Instant>,
}

/// `TB_NOTEPAD_PERF_SCRIPT=search` 면 사이드바 검색 각본 (편집 각본 대신)
fn search_mode() -> bool {
    std::env::var("TB_NOTEPAD_PERF_SCRIPT").is_ok_and(|v| v == "search")
}

const QUERY: &str = "zebra42";

const TYPED: &str = "perf typing test 0123456789 abcdefghijk";

impl Script {
    pub fn active() -> bool {
        enabled() && std::env::var_os("TB_NOTEPAD_PERF_SCRIPT").is_some()
    }

    /// 프레임마다 (raw_input_hook 에서) 부릅니다. 검색 각본이면 이번 프레임의 검색어를 돌려줍니다.
    pub fn step(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) -> Option<String> {
        use egui::{Event, Key, Modifiers};
        const LOAD: u32 = 90;
        if search_mode() {
            let f = self.frame;
            self.frame += 1;
            ctx.request_repaint();
            let n = QUERY.chars().count() as u32;
            return match f {
                f if f < LOAD => {
                    set_phase("load");
                    None
                }
                f if f < LOAD + n => {
                    set_phase("search_typing");
                    self.typed_at = Some(Instant::now());
                    Some(QUERY.chars().take((f - LOAD + 1) as usize).collect())
                }
                // 뒤의 검색이 끝날 때까지 넉넉히 기다림 (프레임 수가 아니라 시간으로 8초)
                _ if self.typed_at.is_some_and(|t| t.elapsed() < Duration::from_secs(8)) => {
                    set_phase("idle");
                    std::thread::sleep(Duration::from_millis(15));
                    None
                }
                _ => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    None
                }
            };
        }
        let typed = TYPED.chars().count() as u32;
        let f = self.frame;
        self.frame += 1;
        ctx.request_repaint();
        let center = raw.screen_rect.map_or(egui::pos2(600.0, 400.0), |r| r.center());
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        let key = |key: Key, modifiers: Modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let typing_end = LOAD + 1 + typed;
        let enter_end = typing_end + 10;
        let scroll_end = enter_end + 1 + 40;
        match f {
            f if f < LOAD => set_phase("load"),
            f if f == LOAD => {
                set_phase("jump_to_end");
                raw.events.push(Event::PointerMoved(center));
                raw.events.push(key(Key::End, ctrl));
            }
            f if f <= LOAD + typed => {
                set_phase("typing");
                let c = TYPED.chars().nth((f - LOAD - 1) as usize).unwrap_or(' ');
                raw.events.push(Event::Text(c.to_string()));
            }
            f if f <= enter_end => {
                set_phase("enter");
                raw.events.push(key(Key::Enter, Modifiers::NONE));
            }
            f if f == enter_end + 1 => {
                set_phase("jump_to_top");
                raw.events.push(key(Key::Home, ctrl));
            }
            f if f <= scroll_end => {
                set_phase("scroll");
                raw.events.push(Event::PointerMoved(center));
                raw.events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                });
            }
            f if f <= scroll_end + 30 => set_phase("idle"),
            _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
        None
    }
}
