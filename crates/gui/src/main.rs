//! Securidge Trace 화면 프로그램.
//!
//! 설명서 없이 "언어 선택 → 점검 시작 → 보고서 받기"가 되도록 만든다.
//! 화면 흐름: 첫 실행(언어·안내) → [왼쪽 메뉴: 점검 / 지난 내역 / 설정 / 도움말]
//!           점검: 시작 → 점검 정보 → 진행 → 결과
//! 디자인: 어두운 그래파이트 바탕 + 시안 강조색 한 가지 (밝은 테마도 고를 수 있다).
//! 모든 문구는 st-i18n의 ko.json·en.json에서 온다.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::f32::consts::{PI, TAU};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, RichText, Sense, Stroke, Vec2};
use st_core::settings::{AllowedDevice, Customer, Settings};
use st_engine::store::{self, HistoryEntry};
use st_engine::{CheckParams, CheckResult, Mode, Stage};
use st_i18n::{t, Lang};
use st_rules::{Grade, Level};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Securidge Trace")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([920.0, 620.0])
            .with_icon(Arc::new(app_icon())),
        ..Default::default()
    };
    eframe::run_native("Securidge Trace", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

// ---------------------------------------------------------------- 색과 글꼴

#[derive(Clone, Copy)]
struct Pal {
    dark: bool,
    bg: Color32,
    side: Color32,
    card: Color32,
    card2: Color32,
    line: Color32,
    text: Color32,
    muted: Color32,
    faint: Color32,
    accent: Color32,
    accent_soft: Color32,
    on_accent: Color32,
    ok: Color32,
    warn: Color32,
    bad: Color32,
    na: Color32,
}

impl Pal {
    fn dark() -> Self {
        Pal {
            dark: true,
            bg: Color32::from_rgb(11, 14, 20),
            side: Color32::from_rgb(14, 18, 26),
            card: Color32::from_rgb(19, 24, 33),
            card2: Color32::from_rgb(25, 31, 42),
            line: Color32::from_rgb(36, 44, 58),
            text: Color32::from_rgb(230, 237, 243),
            muted: Color32::from_rgb(145, 156, 172),
            faint: Color32::from_rgb(92, 104, 120),
            accent: Color32::from_rgb(34, 211, 238),
            accent_soft: Color32::from_rgba_unmultiplied(34, 211, 238, 28),
            on_accent: Color32::from_rgb(6, 24, 30),
            ok: Color32::from_rgb(52, 211, 153),
            warn: Color32::from_rgb(251, 191, 36),
            bad: Color32::from_rgb(248, 113, 113),
            na: Color32::from_rgb(148, 163, 184),
        }
    }

    fn light() -> Self {
        Pal {
            dark: false,
            bg: Color32::from_rgb(244, 246, 249),
            side: Color32::from_rgb(236, 240, 245),
            card: Color32::from_rgb(255, 255, 255),
            card2: Color32::from_rgb(246, 248, 251),
            line: Color32::from_rgb(222, 228, 236),
            text: Color32::from_rgb(15, 23, 42),
            muted: Color32::from_rgb(84, 96, 114),
            faint: Color32::from_rgb(140, 150, 165),
            accent: Color32::from_rgb(8, 145, 178),
            accent_soft: Color32::from_rgba_unmultiplied(8, 145, 178, 26),
            on_accent: Color32::from_rgb(255, 255, 255),
            ok: Color32::from_rgb(22, 150, 90),
            warn: Color32::from_rgb(196, 130, 0),
            bad: Color32::from_rgb(210, 50, 50),
            na: Color32::from_rgb(110, 118, 130),
        }
    }

    fn grade(&self, g: Grade) -> Color32 {
        match g {
            Grade::NothingFound => self.ok,
            Grade::Caution => self.warn,
            Grade::Review => self.bad,
            Grade::Undetermined => self.na,
        }
    }

    fn level(&self, l: Level) -> Color32 {
        match l {
            Level::High => self.bad,
            Level::Medium => self.warn,
            Level::Low => self.na,
        }
    }
}

fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

fn bfont(size: f32) -> FontId {
    FontId::new(size, bold())
}

/// 한글 글꼴을 운영체제에서 찾아 넣는다 (Windows: 맑은 고딕). 굵은 글꼴은 "bold" 글꼴 묶음으로.
fn install_fonts(ctx: &egui::Context) {
    let regular = [
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\gulim.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];
    let bolds = [
        "C:\\Windows\\Fonts\\malgunbd.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut have_regular = false;
    for path in regular {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("korean".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().insert(0, "korean".to_owned());
            }
            have_regular = true;
            break;
        }
    }
    let mut bold_list: Vec<String> = Vec::new();
    for path in bolds {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("korean-bold".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            bold_list.push("korean-bold".to_owned());
            break;
        }
    }
    if have_regular {
        bold_list.push("korean".to_owned());
    }
    // 기본 글꼴도 뒤에 둔다 (기호 등)
    bold_list.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(bold(), bold_list);
    ctx.set_fonts(fonts);
}

fn install_style(ctx: &egui::Context, p: &Pal) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(14);
    v.extreme_bg_color = p.card2;
    v.text_edit_bg_color = Some(p.card2);
    v.faint_bg_color = p.card2;
    v.code_bg_color = p.card2;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.5, p.accent);
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.bad;
    v.striped = false;
    let r = CornerRadius::same(10);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.bg_fill = p.card2;
    v.widgets.inactive.weak_bg_fill = p.card2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, if p.dark { Color32::from_rgb(58, 70, 88) } else { Color32::from_rgb(196, 204, 216) });
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = p.card2;
    v.widgets.hovered.weak_bg_fill = p.card2;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.bg_fill = p.accent_soft;
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.5, p.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.active.corner_radius = r;
    v.widgets.open = v.widgets.active;
    ctx.set_visuals(v);
    ctx.all_styles_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Heading, bfont(24.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Small, FontId::proportional(12.5)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.interact_size.y = 30.0;
    });
}

/// 창 아이콘: 동심원 고리와 점 (그림 파일 없이 그린다).
fn app_icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let d = (dx * dx + dy * dy).sqrt();
            let ang = dy.atan2(dx);
            let (r, g, b, a) = if d < 30.0 {
                let ring1 = (d - 24.0).abs() < 3.2 && !(ang > 0.35 && ang < 1.35);
                let ring2 = (d - 15.0).abs() < 2.6 && !(ang > -2.6 && ang < -1.7);
                let dot = d < 5.5;
                if ring1 || ring2 || dot {
                    (34u8, 211u8, 238u8, 255u8)
                } else {
                    (14, 18, 26, 255)
                }
            } else if d < 31.5 {
                (14, 18, 26, 200)
            } else {
                (0, 0, 0, 0)
            };
            let i = (y * n + x) * 4;
            if let Some(px) = rgba.get_mut(i..i + 4) {
                px.copy_from_slice(&[r, g, b, a]);
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

// ---------------------------------------------------------------- 그리기 도구

fn arc(painter: &egui::Painter, center: Pos2, radius: f32, start: f32, sweep: f32, stroke: Stroke) {
    let steps = ((sweep.abs() / TAU) * 96.0).ceil().max(2.0) as usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f32 / steps as f32);
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(pts, stroke));
}

fn card<R>(ui: &mut egui::Ui, p: &Pal, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, add)
        .inner
}

fn dot(ui: &mut egui::Ui, color: Color32, r: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(r * 2.0 + 2.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), r, color);
}

fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(20))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).font(bfont(12.5)));
        });
}

fn primary(ui: &mut egui::Ui, p: &Pal, text: &str, size: Vec2, enabled: bool) -> egui::Response {
    let b = egui::Button::new(RichText::new(text).font(bfont(15.5)).color(p.on_accent))
        .fill(p.accent)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(10))
        .min_size(size);
    ui.add_enabled(enabled, b)
}

fn ghost(ui: &mut egui::Ui, text: &str, size: Vec2) -> egui::Response {
    ui.add(egui::Button::new(text).corner_radius(CornerRadius::same(10)).min_size(size))
}

fn title(ui: &mut egui::Ui, p: &Pal, text: &str, sub: Option<&str>) {
    ui.label(RichText::new(text).font(bfont(26.0)).color(p.text));
    if let Some(s) = sub {
        ui.label(RichText::new(s).color(p.muted).size(14.5));
    }
    ui.add_space(14.0);
}

fn section(ui: &mut egui::Ui, p: &Pal, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).font(bfont(13.0)).color(p.muted));
    ui.add_space(2.0);
}

// ---------------------------------------------------------------- 상태

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Language,
    Welcome,
    Home,
    Form { offline: bool },
    Progress,
    Result,
    History,
    Settings,
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Basic,
    Customers,
    Advanced,
}

#[derive(Default)]
struct Shared {
    stage: Option<Stage>,
    message: String,
    done: Option<Result<CheckResult, String>>,
}

struct Form {
    customer_idx: usize,
    new_customer: String,
    pc_name: String,
    analyst: String,
    period_days: u32,
    report_lang: Lang,
    consent: bool,
    input: String,
}

struct App {
    data: PathBuf,
    settings: Settings,
    customers: Vec<Customer>,
    lang: Lang,
    pal: Pal,
    screen: Screen,
    admin: bool,
    form: Form,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    started_at: f64,
    result: Option<CheckResult>,
    error: Option<String>,
    flash: Option<String>,
    history: Vec<HistoryEntry>,
    history_filter: String,
    confirm_delete: Option<PathBuf>,
    tab: SettingsTab,
    edit_customer: usize,
    new_device: (String, String),
    lists: [String; 5],
}

fn lang_of(code: &str) -> Lang {
    Lang::parse(code).unwrap_or(Lang::Ko)
}

fn pal_of(theme: &str) -> Pal {
    if theme == "light" {
        Pal::light()
    } else {
        Pal::dark()
    }
}

fn lists_of(s: &Settings) -> [String; 5] {
    [
        s.rules.cloud_domains.join("\n"),
        s.rules.webmail_domains.join("\n"),
        s.rules.file_share_domains.join("\n"),
        s.rules.cleanup_tool_names.join("\n"),
        s.rules.remote_tool_names.join("\n"),
    ]
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let data = store::data_dir();
        let settings = store::load_settings(&data);
        let pal = pal_of(&settings.theme);
        install_style(&cc.egui_ctx, &pal);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale.clamp(0.8, 2.0));
        let customers = store::load_customers(&data);
        let first_run = settings.ui_lang.is_empty();
        let lang = if first_run { system_lang() } else { lang_of(&settings.ui_lang) };
        let screen = if first_run {
            Screen::Language
        } else if !settings.consent_accepted {
            Screen::Welcome
        } else {
            Screen::Home
        };
        let customer_idx = customers.iter().position(|c| c.name == settings.last_customer).unwrap_or(0);
        let history = store::list_history(&store::history_dir(&data));
        App {
            form: Form {
                customer_idx,
                new_customer: String::new(),
                pc_name: std::env::var("COMPUTERNAME").unwrap_or_default(),
                analyst: settings.analyst.clone(),
                period_days: settings.default_period_days,
                report_lang: lang_of(&settings.report_lang),
                consent: false,
                input: String::new(),
            },
            admin: st_engine::live::is_admin(),
            lists: lists_of(&settings),
            data,
            settings,
            customers,
            lang,
            pal,
            screen,
            shared: Arc::new(Mutex::new(Shared::default())),
            cancel: Arc::new(AtomicBool::new(false)),
            started_at: 0.0,
            result: None,
            error: None,
            flash: None,
            history,
            history_filter: String::new(),
            confirm_delete: None,
            tab: SettingsTab::Basic,
            edit_customer: 0,
            new_device: (String::new(), String::new()),
        }
    }

    fn t(&self, k: &str) -> String {
        t(self.lang, k)
    }

    fn save_settings(&mut self) {
        self.settings.ui_lang = self.lang.code().to_string();
        if let Err(e) = store::save_settings(&self.data, &self.settings) {
            self.flash = Some(e);
        }
    }

    fn save_customers(&mut self) {
        if let Err(e) = store::save_customers(&self.data, &self.customers) {
            self.flash = Some(e);
        }
    }

    fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        if !self.settings.ui_lang.is_empty() {
            self.save_settings();
        }
    }

    fn go(&mut self, screen: Screen) {
        if screen == Screen::History {
            self.history = store::list_history(&store::history_dir(&self.data));
        }
        if matches!(screen, Screen::Form { .. }) {
            self.form.consent = false;
            self.error = None;
        }
        self.flash = None;
        self.screen = screen;
    }

    // ------------------------------------------------------------ 왼쪽 메뉴
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        let busy = self.screen == Screen::Progress;
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            let c = rect.center();
            arc(ui.painter(), c, 13.0, -PI * 0.8, PI * 1.6, Stroke::new(2.6, p.accent));
            arc(ui.painter(), c, 7.5, PI * 0.25, PI * 1.5, Stroke::new(2.2, p.accent));
            ui.painter().circle_filled(c, 2.6, p.accent);
            ui.vertical(|ui| {
                ui.label(RichText::new("Securidge").font(bfont(16.0)).color(p.text));
                ui.label(RichText::new("TRACE").font(bfont(11.0)).color(p.accent));
            });
        });
        ui.add_space(26.0);
        let items = [
            (Screen::Home, "ui.nav.check"),
            (Screen::History, "ui.home.history"),
            (Screen::Settings, "ui.home.settings"),
            (Screen::Help, "ui.home.help"),
        ];
        for (target, key) in items {
            let selected = match (target, self.screen) {
                (Screen::Home, Screen::Home | Screen::Form { .. } | Screen::Progress | Screen::Result) => true,
                (a, b) => a == b,
            };
            let w = ui.available_width() - 4.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 42.0), if busy { Sense::hover() } else { Sense::click() });
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, CornerRadius::same(10), p.accent_soft);
                let bar = egui::Rect::from_min_size(rect.left_top() + Vec2::new(0.0, 10.0), Vec2::new(3.0, 22.0));
                painter.rect_filled(bar, CornerRadius::same(2), p.accent);
            } else if resp.hovered() && !busy {
                painter.rect_filled(rect, CornerRadius::same(10), p.card2);
            }
            let color = if selected { p.text } else if busy { p.faint } else { p.muted };
            painter.text(rect.left_center() + Vec2::new(18.0, 0.0), Align2::LEFT_CENTER, self.t(key), bfont(15.0), color);
            if resp.clicked() && !busy {
                self.go(target);
            }
            ui.add_space(4.0);
        }

        // 아래쪽: 권한, 언어, 버전
        let bottom_h = 150.0;
        let space = ui.available_height() - bottom_h;
        if space > 0.0 {
            ui.add_space(space);
        }
        ui.separator();
        ui.add_space(6.0);
        if cfg!(windows) {
            ui.horizontal(|ui| {
                dot(ui, if self.admin { p.ok } else { p.warn }, 4.0);
                let key = if self.admin { "ui.home.admin_ok" } else { "ui.home.admin_no" };
                ui.add(egui::Label::new(RichText::new(self.t(key)).size(12.0).color(p.muted)).wrap());
            });
            if !self.admin && !busy && ui.small_button(self.t("ui.home.restart_admin")).clicked() {
                restart_as_admin(ui.ctx());
            }
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            for (l, name) in [(Lang::Ko, "한국어"), (Lang::En, "English")] {
                let on = self.lang == l;
                let txt = RichText::new(name).size(13.0).color(if on { p.accent } else { p.muted });
                if ui.add(egui::Button::new(txt).frame(false)).clicked() && !busy {
                    self.set_lang(l);
                }
            }
        });
        ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(11.5).color(p.faint));
    }

    // ------------------------------------------------------------ 첫 실행
    fn ui_language(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        let h = ui.available_height();
        ui.add_space((h / 2.0 - 190.0).max(20.0));
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
            let c = rect.center();
            let time = ui.input(|i| i.time) as f32;
            arc(ui.painter(), c, 42.0, time * 0.6, PI * 1.5, Stroke::new(3.0, p.accent));
            arc(ui.painter(), c, 26.0, -time * 0.9, PI * 1.3, Stroke::new(2.5, p.accent.gamma_multiply(0.7)));
            ui.painter().circle_filled(c, 6.0, p.accent);
            ui.ctx().request_repaint();
            ui.add_space(14.0);
            ui.label(RichText::new("Securidge Trace").font(bfont(30.0)).color(p.text));
            ui.add_space(22.0);
            for line in self.t("ui.choose_lang").lines() {
                ui.label(RichText::new(line).size(16.0).color(p.muted));
            }
            ui.add_space(26.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 2.0 * 180.0 - 16.0) / 2.0);
                for (l, name) in [(Lang::Ko, "한국어"), (Lang::En, "English")] {
                    if primary(ui, &p, name, Vec2::new(180.0, 54.0), true).clicked() {
                        self.lang = l;
                        self.settings.report_lang = l.code().into();
                        self.form.report_lang = l;
                        self.save_settings();
                        self.screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);
                }
            });
        });
    }

    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        let h = ui.available_height();
        ui.add_space((h / 2.0 - 220.0).max(20.0));
        ui.vertical_centered(|ui| {
            ui.set_max_width(640.0);
            card(ui, &p, |ui| {
                ui.set_width(600.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| self.welcome_body(ui));
            });
        });
    }

    fn welcome_body(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        {
            {
                ui.label(RichText::new(self.t("ui.welcome.title")).font(bfont(24.0)).color(p.text));
                ui.add_space(14.0);
                for k in ["ui.welcome.1", "ui.welcome.2", "ui.welcome.3"] {
                    ui.horizontal(|ui| {
                        dot(ui, p.accent, 3.5);
                        ui.add(egui::Label::new(RichText::new(self.t(k)).size(15.5).color(p.text)).wrap());
                    });
                    ui.add_space(6.0);
                }
                ui.add_space(12.0);
                let mut agree = self.settings.consent_accepted;
                ui.checkbox(&mut agree, RichText::new(self.t("ui.welcome.agree")).size(15.5));
                self.settings.consent_accepted = agree;
                ui.add_space(14.0);
                if primary(ui, &p, &self.t("ui.start"), Vec2::new(180.0, 46.0), agree).clicked() {
                    self.save_settings();
                    self.screen = Screen::Home;
                }
            }
        }
    }

    // ------------------------------------------------------------ 점검 시작
    fn ui_home(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        title(ui, &p, &self.t("ui.home.title"), Some(&self.t("ui.home.subtitle")));
        ui.horizontal_top(|ui| {
            // 왼쪽: 시작 고리
            ui.vertical(|ui| {
                ui.set_width(300.0);
                card(ui, &p, |ui| {
                    ui.set_width(264.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        if self.scan_button(ui).clicked() {
                            self.go(Screen::Form { offline: false });
                        }
                        ui.add_space(10.0);
                        if ui.link(RichText::new(self.t("ui.home.offline")).size(13.0)).clicked() {
                            self.go(Screen::Form { offline: true });
                        }
                    });
                });
            });
            ui.add_space(8.0);
            // 오른쪽: 확인하는 기록 + 마지막 점검
            let right_w = (ui.available_width() - 4.0).max(320.0);
            ui.allocate_ui_with_layout(Vec2::new(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(right_w);
                section(ui, &p, &self.t("ui.home.covers"));
                let tiles = ["ui.tile.devices", "ui.tile.files", "ui.tile.programs", "ui.tile.deleted", "ui.tile.network", "ui.tile.web"];
                let tile_w = (right_w - 10.0) / 2.0;
                for row in tiles.chunks(2) {
                    ui.horizontal(|ui| {
                        for key in row {
                            egui::Frame::new()
                                .fill(p.card)
                                .stroke(Stroke::new(1.0, p.line))
                                .corner_radius(CornerRadius::same(12))
                                .inner_margin(Margin::symmetric(14, 11))
                                .show(ui, |ui| {
                                    ui.set_width(tile_w - 30.0);
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(Vec2::new(6.0, 16.0), Sense::hover());
                                            ui.painter().rect_filled(rect, CornerRadius::same(2), p.accent);
                                            ui.label(RichText::new(self.t(key)).font(bfont(14.5)).color(p.text));
                                        });
                                        ui.label(RichText::new(self.t(&format!("{key}.d"))).size(12.5).color(p.muted));
                                    });
                                });
                        }
                    });
                    ui.add_space(2.0);
                }
                ui.add_space(10.0);
                section(ui, &p, &self.t("ui.home.last"));
                let last = self.history.first().cloned();
                card(ui, &p, |ui| {
                    ui.set_width(ui.available_width());
                    match last {
                        None => {
                            ui.label(RichText::new(self.t("ui.home.no_last")).color(p.muted));
                        }
                        Some(e) => {
                            let tz: chrono_tz::Tz = self.settings.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
                            ui.horizontal(|ui| {
                                dot(ui, p.grade(e.grade), 6.0);
                                ui.label(RichText::new(t(self.lang, e.grade.key())).font(bfont(15.0)).color(p.text));
                                ui.label(RichText::new(st_i18n::format_stamp(e.started, tz, self.lang)).color(p.muted));
                            });
                            let who = [e.customer.as_str(), e.pc_name.as_str(), e.analyst.as_str()]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join("  ·  ");
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if who.is_empty() { "-".to_string() } else { who }).size(13.0).color(p.muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ghost(ui, &self.t("ui.result.open"), Vec2::new(120.0, 32.0)).clicked() {
                                            let lang = if e.reports.iter().any(|r| r == &format!("report_{}.html", self.lang.code())) {
                                                self.lang
                                            } else if e.reports.iter().any(|r| r == "report_ko.html") {
                                                Lang::Ko
                                            } else {
                                                Lang::En
                                            };
                                            self.flash = st_engine::open_path(&e.dir.join(format!("report_{}.html", lang.code()))).err();
                                        }
                                    });
                            });
                        }
                    }
                });
                if let Some(f) = &self.flash {
                    ui.colored_label(p.bad, f);
                }
            });
        });
    }

    /// 가운데 큰 원형 "점검 시작" 버튼. 마우스를 올리면 고리가 돈다.
    fn scan_button(&self, ui: &mut egui::Ui) -> egui::Response {
        let p = self.pal;
        let size = 220.0;
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        let painter = ui.painter_at(rect.expand(4.0));
        let c = rect.center();
        let hovered = resp.hovered();
        let time = ui.input(|i| i.time) as f32;
        let speed = if hovered { 1.6 } else { 0.35 };
        painter.circle_stroke(c, 104.0, Stroke::new(1.0, p.line));
        for k in 0..3 {
            let start = time * speed + k as f32 * TAU / 3.0;
            arc(&painter, c, 104.0, start, PI * 0.42, Stroke::new(3.0, p.accent));
        }
        arc(&painter, c, 90.0, -time * speed * 0.7, PI * 1.25, Stroke::new(1.5, p.accent.gamma_multiply(0.45)));
        let inner = if hovered { p.accent_soft } else { p.card2 };
        painter.circle_filled(c, 78.0, inner);
        painter.circle_stroke(c, 78.0, Stroke::new(1.0, if hovered { p.accent } else { p.line }));
        painter.text(c - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, self.t("ui.home.scan"), bfont(22.0), p.text);
        painter.text(c + Vec2::new(0.0, 18.0), Align2::CENTER_CENTER, self.t("ui.home.duration"), FontId::proportional(12.5), p.muted);
        ui.ctx().request_repaint();
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    // ------------------------------------------------------------ 점검 정보
    fn ui_form(&mut self, ui: &mut egui::Ui, offline: bool) {
        let p = self.pal;
        let sub = if offline { self.t("ui.home.offline") } else { self.t("ui.home.subtitle") };
        title(ui, &p, &self.t("ui.form.title"), Some(&sub));
        card(ui, &p, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            egui::Grid::new("form").num_columns(2).spacing([22.0, 14.0]).min_col_width(120.0).show(ui, |ui| {
                let label = |ui: &mut egui::Ui, s: String| {
                    ui.label(RichText::new(s).color(p.muted));
                };
                if offline {
                    label(ui, self.t("ui.form.input"));
                    ui.vertical(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.form.input).desired_width(460.0).hint_text("E:\\"));
                        ui.label(RichText::new(self.t("ui.form.input_hint")).size(12.0).color(p.faint));
                    });
                    ui.end_row();
                }
                label(ui, self.t("ui.form.customer"));
                ui.horizontal(|ui| {
                    let names: Vec<String> = self.customers.iter().map(|c| c.name.clone()).collect();
                    let current = names.get(self.form.customer_idx).cloned().unwrap_or_else(|| "-".to_string());
                    egui::ComboBox::from_id_salt("cust").selected_text(current).width(220.0).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.form.customer_idx, i, n);
                        }
                    });
                    let hint = self.t("ui.form.new_customer");
                    ui.add(egui::TextEdit::singleline(&mut self.form.new_customer).hint_text(hint).desired_width(150.0));
                    if ui.button(self.t("ui.settings.add")).clicked() && !self.form.new_customer.trim().is_empty() {
                        self.customers.push(Customer { name: self.form.new_customer.trim().to_string(), ..Default::default() });
                        self.form.customer_idx = self.customers.len() - 1;
                        self.form.new_customer.clear();
                        self.save_customers();
                    }
                });
                ui.end_row();
                label(ui, self.t("ui.form.pc"));
                ui.add(egui::TextEdit::singleline(&mut self.form.pc_name).desired_width(280.0));
                ui.end_row();
                label(ui, self.t("ui.form.analyst"));
                ui.add(egui::TextEdit::singleline(&mut self.form.analyst).desired_width(280.0));
                ui.end_row();
                label(ui, self.t("ui.form.period"));
                ui.horizontal(|ui| {
                    for (days, key) in [(30, "ui.form.days30"), (90, "ui.form.days90"), (365, "ui.form.days365"), (0, "ui.form.all")] {
                        let on = self.form.period_days == days;
                        let txt = RichText::new(self.t(key)).color(if on { p.accent } else { p.text });
                        if ui.add(egui::Button::selectable(on, txt).corner_radius(CornerRadius::same(8))).clicked() {
                            self.form.period_days = days;
                        }
                    }
                });
                ui.end_row();
                label(ui, self.t("ui.form.report_lang"));
                ui.horizontal(|ui| {
                    for (l, name) in [(Lang::Ko, "한국어"), (Lang::En, "English")] {
                        let on = self.form.report_lang == l;
                        let txt = RichText::new(name).color(if on { p.accent } else { p.text });
                        if ui.add(egui::Button::selectable(on, txt).corner_radius(CornerRadius::same(8))).clicked() {
                            self.form.report_lang = l;
                        }
                    }
                });
                ui.end_row();
            });
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            let consent_label = RichText::new(self.t("ui.form.consent")).font(bfont(15.0));
            ui.checkbox(&mut self.form.consent, consent_label);
            if let Some(e) = &self.error {
                ui.colored_label(p.bad, e);
            }
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ghost(ui, &self.t("ui.back"), Vec2::new(110.0, 44.0)).clicked() {
                self.go(Screen::Home);
            }
            if primary(ui, &p, &self.t("ui.run"), Vec2::new(220.0, 44.0), self.form.consent).clicked() {
                self.start(ui.ctx(), offline);
            }
            if !self.form.consent {
                ui.label(RichText::new(self.t("ui.err.no_consent")).size(13.0).color(p.faint));
            }
        });
    }

    fn start(&mut self, ctx: &egui::Context, offline: bool) {
        let mode = if offline {
            let p = PathBuf::from(self.form.input.trim());
            if !p.is_dir() {
                self.error = Some(self.t("ui.err.no_input"));
                return;
            }
            Mode::Offline(p)
        } else {
            if !cfg!(windows) {
                self.error = Some(self.t("ui.err.windows_only"));
                return;
            }
            Mode::Live
        };
        let customer = self.customers.get(self.form.customer_idx).cloned().unwrap_or_default();
        self.settings.last_customer = customer.name.clone();
        self.settings.analyst = self.form.analyst.clone();
        self.save_settings();
        let params = CheckParams {
            mode,
            customer,
            pc_name: self.form.pc_name.clone(),
            analyst: self.form.analyst.clone(),
            company: self.settings.company_name.clone(),
            period_days: self.form.period_days,
            report_lang: self.form.report_lang,
            time_zone: self.settings.time_zone.clone(),
            consent: self.form.consent,
            mask_names: self.settings.mask_names,
            keep_copies: self.settings.keep_collected_copies,
            rules: self.settings.rules.clone(),
            history_dir: store::history_dir(&self.data),
            fallback_codepage: if self.lang == Lang::Ko { 949 } else { 1252 },
        };
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = Shared::default();
        self.cancel.store(false, Ordering::Relaxed);
        let shared = self.shared.clone();
        let cancel = self.cancel.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let progress = |stage: Stage, msg: &str| {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.stage = Some(stage);
                s.message = msg.to_string();
                ctx2.request_repaint();
            };
            let r = st_engine::run_check(&params, &progress, &cancel);
            shared.lock().unwrap_or_else(|e| e.into_inner()).done = Some(r);
            ctx2.request_repaint();
        });
        self.started_at = ctx.input(|i| i.time);
        self.result = None;
        self.error = None;
        self.screen = Screen::Progress;
    }

    // ------------------------------------------------------------ 진행
    fn ui_progress(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        let (stage, message, done) = {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (s.stage, s.message.clone(), s.done.take())
        };
        if let Some(r) = done {
            match r {
                Ok(res) => {
                    self.result = Some(res);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.history = store::list_history(&store::history_dir(&self.data));
            self.screen = Screen::Result;
            return;
        }
        let order = [Stage::Collect, Stage::Analyze, Stage::Report];
        let pos = stage.and_then(|s| order.iter().position(|x| *x == s)).unwrap_or(0);
        let now = ui.input(|i| i.time);
        let elapsed = (now - self.started_at).max(0.0);
        ui.ctx().request_repaint();

        let h = ui.available_height();
        ui.add_space((h / 2.0 - 230.0).max(10.0));
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(200.0), Sense::hover());
            let painter = ui.painter_at(rect.expand(4.0));
            let c = rect.center();
            let frac = (pos as f32 + 0.5) / 3.0;
            painter.circle_stroke(c, 88.0, Stroke::new(6.0, p.card2));
            arc(&painter, c, 88.0, -PI / 2.0, TAU * frac, Stroke::new(6.0, p.accent));
            let spin = elapsed as f32 * 2.2;
            arc(&painter, c, 72.0, spin, PI * 0.5, Stroke::new(2.0, p.accent.gamma_multiply(0.6)));
            painter.text(c - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, format!("{}/3", pos + 1), bfont(30.0), p.text);
            let secs = elapsed as u64;
            painter.text(c + Vec2::new(0.0, 24.0), Align2::CENTER_CENTER, format!("{:02}:{:02}", secs / 60, secs % 60), FontId::monospace(13.0), p.muted);
            ui.add_space(20.0);
            ui.label(RichText::new(self.t("ui.progress.title")).font(bfont(22.0)).color(p.text));
            ui.add_space(16.0);
            // 단계 표시
            let keys = ["ui.progress.collect", "ui.progress.analyze", "ui.progress.report"];
            let w = 560.0f32.min(ui.available_width());
            let (srect, _) = ui.allocate_exact_size(Vec2::new(w, 64.0), Sense::hover());
            let painter = ui.painter_at(srect);
            let step = w / 3.0;
            for (i, key) in keys.iter().enumerate() {
                let cx = srect.left() + step * (i as f32 + 0.5);
                let cy = srect.top() + 14.0;
                if i < 2 {
                    let color = if i < pos { p.accent } else { p.line };
                    painter.line_segment([Pos2::new(cx + 14.0, cy), Pos2::new(cx + step - 14.0, cy)], Stroke::new(2.0, color));
                }
                let (fill, ring) = if i < pos {
                    (p.accent, p.accent)
                } else if i == pos {
                    (p.accent_soft, p.accent)
                } else {
                    (p.card2, p.line)
                };
                painter.circle_filled(Pos2::new(cx, cy), 10.0, fill);
                painter.circle_stroke(Pos2::new(cx, cy), 10.0, Stroke::new(1.5, ring));
                let color = if i <= pos { p.text } else { p.faint };
                painter.text(Pos2::new(cx, cy + 30.0), Align2::CENTER_CENTER, self.t(key), FontId::proportional(13.5), color);
            }
            ui.add_space(8.0);
            let msg = if message.is_empty() { " ".to_string() } else { message.clone() };
            ui.label(RichText::new(msg).monospace().color(p.faint));
            ui.add_space(18.0);
            if self.cancel.load(Ordering::Relaxed) {
                ui.label(RichText::new(self.t("ui.progress.cancelling")).color(p.warn));
            } else if ghost(ui, &self.t("ui.cancel"), Vec2::new(120.0, 38.0)).clicked() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        });
    }

    // ------------------------------------------------------------ 결과
    fn ui_result(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        if let Some(e) = self.error.clone() {
            title(ui, &p, &self.t("ui.result.error"), None);
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(p.bad, e);
            });
            ui.add_space(12.0);
            if primary(ui, &p, &self.t("ui.result.home"), Vec2::new(140.0, 40.0), true).clicked() {
                self.go(Screen::Home);
            }
            return;
        }
        let Some(res) = self.result.clone() else {
            self.go(Screen::Home);
            return;
        };
        let a = &res.case.assessment;
        let tz: chrono_tz::Tz = res.case.info.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            title(ui, &p, &self.t("ui.result.title"), None);
            // 등급 카드
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(84.0), Sense::hover());
                    let c = rect.center();
                    let col = p.grade(a.grade);
                    ui.painter().circle_filled(c, 34.0, col.gamma_multiply(0.18));
                    ui.painter().circle_stroke(c, 34.0, Stroke::new(3.0, col));
                    ui.painter().circle_filled(c, 12.0, col);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(self.t(a.grade.key())).font(bfont(26.0)).color(col));
                        let desc = a.grade.key().replace("grade.", "grade.desc.");
                        ui.add(egui::Label::new(RichText::new(self.t(&desc)).color(p.muted)).wrap());
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                self.t("report.created"),
                                st_i18n::format_stamp_with_zone(res.case.info.finished_utc, tz, self.lang)
                            ))
                            .size(12.5)
                            .color(p.faint),
                        );
                    });
                });
            });
            for w in &res.warnings {
                ui.add_space(6.0);
                ui.colored_label(p.warn, format!("!  {w}"));
            }
            ui.add_space(12.0);
            // 숫자 카드
            let high = a.findings.iter().filter(|f| f.level == Level::High).count();
            let med = a.findings.iter().filter(|f| f.level == Level::Medium).count();
            let stats = [
                (res.case.analysis.events.len(), "ui.result.records", p.text),
                (high, "ui.result.high", if high > 0 { p.bad } else { p.text }),
                (med, "ui.result.medium", if med > 0 { p.warn } else { p.text }),
                (res.case.analysis.devices.len(), "ui.result.devices_n", p.text),
            ];
            let w = ((ui.available_width() - 30.0) / 4.0).max(120.0);
            ui.horizontal(|ui| {
                for (n, key, color) in stats {
                    egui::Frame::new()
                        .fill(p.card)
                        .stroke(Stroke::new(1.0, p.line))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width(w - 30.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(n.to_string()).font(bfont(28.0)).color(color));
                                ui.label(RichText::new(self.t(key)).size(13.0).color(p.muted));
                            });
                        });
                }
            });
            ui.add_space(12.0);
            // 동작 버튼
            ui.horizontal(|ui| {
                if primary(ui, &p, &self.t("ui.result.open"), Vec2::new(170.0, 42.0), true).clicked() {
                    self.flash = st_engine::open_path(&res.report_path).err();
                }
                if ghost(ui, &self.t("ui.result.print"), Vec2::new(90.0, 42.0)).clicked() {
                    self.flash = st_engine::open_path(&res.report_path).err();
                }
                if ghost(ui, &self.t("ui.result.folder"), Vec2::new(140.0, 42.0)).clicked() {
                    self.flash = st_engine::open_path(&res.dir).err();
                }
                if ghost(ui, &self.t("ui.result.regen"), Vec2::new(200.0, 42.0)).clicked() {
                    let other = if res.report_path.to_string_lossy().ends_with("_ko.html") { Lang::En } else { Lang::Ko };
                    match st_report::regenerate(&res.dir, other) {
                        Ok(path) => self.flash = st_engine::open_path(&path).err(),
                        Err(e) => self.flash = Some(e),
                    }
                }
            });
            if let Some(f) = &self.flash {
                ui.colored_label(p.bad, f);
            }
            ui.add_space(10.0);
            // 주요 내용
            section(ui, &p, &self.t("report.key_events"));
            let key: Vec<_> = a.findings.iter().take(8).collect();
            if key.is_empty() {
                card(ui, &p, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(self.t("report.no_key_events")).color(p.muted));
                });
            }
            for f in key {
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.line))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            chip(ui, &t(self.lang, &format!("level.{:?}", f.level)), p.level(f.level));
                            ui.label(RichText::new(t(self.lang, &format!("finding.{}.title", f.rule_id))).font(bfont(14.5)).color(p.text));
                        });
                        ui.add(egui::Label::new(RichText::new(st_report::html::finding_sentence(f, self.lang, tz)).color(p.text)).wrap());
                        ui.add(egui::Label::new(RichText::new(t(self.lang, &format!("finding.{}.fp", f.rule_id))).size(12.5).color(p.faint)).wrap());
                    });
                ui.add_space(6.0);
            }
            // 장치
            ui.add_space(6.0);
            section(ui, &p, &self.t("ui.result.devices"));
            let cust_idx = self.form.customer_idx;
            let mut register: Option<(String, String)> = None;
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                if res.case.analysis.devices.is_empty() {
                    ui.label(RichText::new(self.t("report.none")).color(p.muted));
                    return;
                }
                egui::Grid::new("devs").striped(true).num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
                    for d in &res.case.analysis.devices {
                        let name = [d.vendor.clone(), d.product.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ");
                        ui.label(RichText::new(t(self.lang, &format!("device_kind.{:?}", d.kind))).color(p.muted));
                        ui.label(if name.is_empty() { d.friendly_name.clone().unwrap_or_default() } else { name.clone() });
                        ui.label(RichText::new(d.serial.clone().unwrap_or_default()).monospace().color(p.muted));
                        let serial = d.serial.clone().unwrap_or_default();
                        let already = self.customers.get(cust_idx).is_some_and(|c| c.is_allowed(&serial));
                        if already {
                            chip(ui, &self.t("ui.result.registered"), p.ok);
                        } else if !serial.is_empty() && ui.small_button(self.t("ui.result.register")).clicked() {
                            register = Some((serial, name));
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some((id, memo)) = register {
                if self.customers.is_empty() {
                    self.customers.push(Customer { name: res.case.info.customer.clone(), ..Default::default() });
                }
                if let Some(c) = self.customers.get_mut(cust_idx) {
                    c.allowed_devices.push(AllowedDevice { id, memo });
                }
                self.save_customers();
            }
            ui.add_space(14.0);
            if ghost(ui, &self.t("ui.result.home"), Vec2::new(120.0, 38.0)).clicked() {
                self.go(Screen::Home);
            }
        });
    }

    // ------------------------------------------------------------ 지난 내역
    fn ui_history(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.t("ui.history.title")).font(bfont(26.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = self.t("ui.history.search");
                ui.add(egui::TextEdit::singleline(&mut self.history_filter).hint_text(hint).desired_width(220.0));
            });
        });
        ui.label(RichText::new(self.t("ui.history.privacy")).size(13.0).color(p.muted));
        if let Some(f) = &self.flash {
            ui.colored_label(p.bad, f);
        }
        ui.add_space(10.0);
        if self.history.is_empty() {
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(self.t("ui.history.empty")).color(p.muted));
            });
            return;
        }
        let tz: chrono_tz::Tz = self.settings.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
        let filter = self.history_filter.to_lowercase();
        let mut action: Option<(u8, PathBuf, Option<Lang>)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for e in &self.history {
                let hay = format!("{} {} {}", e.customer, e.pc_name, e.analyst).to_lowercase();
                if !filter.is_empty() && !hay.contains(&filter) {
                    continue;
                }
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.line))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(4.0, 44.0), Sense::hover());
                            ui.painter().rect_filled(rect, CornerRadius::same(2), p.grade(e.grade));
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(t(self.lang, e.grade.key())).font(bfont(15.0)).color(p.grade(e.grade)));
                                    ui.label(RichText::new(st_i18n::format_stamp(e.started, tz, self.lang)).color(p.muted));
                                    if !e.changed.is_empty() {
                                        chip(ui, &t(self.lang, "ui.history.changed"), p.bad);
                                    }
                                    if e.old {
                                        chip(ui, &t(self.lang, "ui.history.old"), p.warn);
                                    }
                                });
                                ui.horizontal(|ui| {
                                    let who = [e.customer.as_str(), e.pc_name.as_str(), e.analyst.as_str()]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect::<Vec<_>>()
                                        .join("  ·  ");
                                    ui.label(RichText::new(if who.is_empty() { "-".into() } else { who }).size(13.5).color(p.text));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new(t(self.lang, "ui.delete")).color(p.bad)).clicked() {
                                            action = Some((3, e.dir.clone(), None));
                                        }
                                        if ui.button(t(self.lang, "ui.result.regen")).clicked() {
                                            let other = if e.reports.iter().any(|r| r == "report_ko.html") && !e.reports.iter().any(|r| r == "report_en.html") {
                                                Lang::En
                                            } else {
                                                Lang::Ko
                                            };
                                            action = Some((1, e.dir.clone(), Some(other)));
                                        }
                                        if ui.button(t(self.lang, "ui.result.folder")).clicked() {
                                            action = Some((2, e.dir.clone(), None));
                                        }
                                        let open = egui::Button::new(RichText::new(t(self.lang, "ui.result.open")).color(p.on_accent).font(bfont(14.0))).fill(p.accent);
                                        if ui.add(open).clicked() {
                                            let lang = if e.reports.iter().any(|r| r == &format!("report_{}.html", self.lang.code())) {
                                                self.lang
                                            } else if e.reports.iter().any(|r| r == "report_ko.html") {
                                                Lang::Ko
                                            } else {
                                                Lang::En
                                            };
                                            action = Some((0, e.dir.clone(), Some(lang)));
                                        }
                                    });
                                });
                            });
                        });
                    });
                ui.add_space(8.0);
            }
        });
        match action {
            Some((0, dir, Some(l))) => self.flash = st_engine::open_path(&dir.join(format!("report_{}.html", l.code()))).err(),
            Some((1, dir, Some(l))) => {
                match st_report::regenerate(&dir, l) {
                    Ok(path) => self.flash = st_engine::open_path(&path).err(),
                    Err(e) => self.flash = Some(e),
                }
                self.history = store::list_history(&store::history_dir(&self.data));
            }
            Some((2, dir, _)) => self.flash = st_engine::open_path(&dir).err(),
            Some((3, dir, _)) => self.confirm_delete = Some(dir),
            _ => {}
        }
        if let Some(dir) = self.confirm_delete.clone() {
            egui::Window::new(self.t("ui.delete"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(self.t("ui.history.confirm_delete"));
                    ui.label(RichText::new(dir.display().to_string()).size(12.0).color(p.faint));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let del = egui::Button::new(RichText::new(self.t("ui.delete")).color(Color32::WHITE).font(bfont(14.0))).fill(p.bad);
                        if ui.add(del).clicked() {
                            self.flash = store::delete_history(&store::history_dir(&self.data), &dir).err();
                            self.history = store::list_history(&store::history_dir(&self.data));
                            self.confirm_delete = None;
                        }
                        if ui.button(self.t("ui.cancel")).clicked() {
                            self.confirm_delete = None;
                        }
                    });
                });
        }
    }

    // ------------------------------------------------------------ 설정
    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.t("ui.settings.title")).font(bfont(26.0)).color(p.text));
            ui.add_space(20.0);
            for (tab, key) in [
                (SettingsTab::Basic, "ui.settings.basic"),
                (SettingsTab::Customers, "ui.settings.customers"),
                (SettingsTab::Advanced, "ui.settings.advanced"),
            ] {
                let on = self.tab == tab;
                let txt = RichText::new(self.t(key)).color(if on { p.accent } else { p.muted }).font(bfont(14.5));
                if ui.add(egui::Button::selectable(on, txt).corner_radius(CornerRadius::same(8))).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 64.0).show(ui, |ui| {
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                match self.tab {
                    SettingsTab::Basic => self.settings_basic(ui),
                    SettingsTab::Customers => self.settings_customers(ui),
                    SettingsTab::Advanced => self.settings_advanced(ui),
                }
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if primary(ui, &p, &self.t("ui.save"), Vec2::new(110.0, 40.0), true).clicked() {
                let split = |s: &str| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
                let [a, b, c, d, e] = &self.lists;
                self.settings.rules.cloud_domains = split(a);
                self.settings.rules.webmail_domains = split(b);
                self.settings.rules.file_share_domains = split(c);
                self.settings.rules.cleanup_tool_names = split(d);
                self.settings.rules.remote_tool_names = split(e);
                self.save_settings();
                self.save_customers();
                ui.ctx().set_zoom_factor(self.settings.ui_scale.clamp(0.8, 2.0));
                self.pal = pal_of(&self.settings.theme);
                install_style(ui.ctx(), &self.pal);
                self.flash = Some(self.t("ui.settings.saved"));
            }
            if ghost(ui, &self.t("ui.settings.reset"), Vec2::new(0.0, 40.0)).clicked() {
                let keep_lang = self.settings.ui_lang.clone();
                let keep_theme = self.settings.theme.clone();
                self.settings = Settings { ui_lang: keep_lang, consent_accepted: true, theme: keep_theme, ..Settings::default() };
                self.lists = lists_of(&self.settings);
            }
            if ghost(ui, &self.t("ui.settings.export"), Vec2::new(0.0, 40.0)).clicked() {
                let to = self.data.join("securidge-trace-settings.json");
                self.flash = Some(match store::export_bundle(&self.data, &to) {
                    Ok(()) => to.display().to_string(),
                    Err(e) => e,
                });
            }
            if ghost(ui, &self.t("ui.settings.import"), Vec2::new(0.0, 40.0)).clicked() {
                let from = self.data.join("securidge-trace-settings.json");
                self.flash = Some(match store::import_bundle(&self.data, &from) {
                    Ok(()) => {
                        self.settings = store::load_settings(&self.data);
                        self.customers = store::load_customers(&self.data);
                        self.lists = lists_of(&self.settings);
                        self.t("ui.settings.saved")
                    }
                    Err(e) => format!("{e} ({})", from.display()),
                });
            }
            if let Some(f) = &self.flash {
                ui.label(RichText::new(f).color(p.ok));
            }
        });
    }

    fn settings_basic(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        egui::Grid::new("basic").num_columns(2).spacing([22.0, 12.0]).min_col_width(160.0).show(ui, |ui| {
            let label = |ui: &mut egui::Ui, s: String| {
                ui.label(RichText::new(s).color(p.muted));
            };
            label(ui, self.t("ui.settings.ui_lang"));
            ui.horizontal(|ui| {
                if ui.radio(self.lang == Lang::Ko, "한국어").clicked() {
                    self.lang = Lang::Ko;
                }
                if ui.radio(self.lang == Lang::En, "English").clicked() {
                    self.lang = Lang::En;
                }
            });
            ui.end_row();
            label(ui, self.t("ui.settings.theme"));
            ui.horizontal(|ui| {
                for (v, key) in [("dark", "ui.theme.dark"), ("light", "ui.theme.light")] {
                    let label = self.t(key);
                    ui.radio_value(&mut self.settings.theme, v.to_string(), label);
                }
            });
            ui.end_row();
            label(ui, self.t("ui.settings.report_lang"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.settings.report_lang, "ko".to_string(), "한국어");
                ui.radio_value(&mut self.settings.report_lang, "en".to_string(), "English");
            });
            ui.end_row();
            label(ui, self.t("ui.settings.tz"));
            ui.add(egui::TextEdit::singleline(&mut self.settings.time_zone).desired_width(240.0));
            ui.end_row();
            label(ui, self.t("ui.settings.company"));
            ui.add(egui::TextEdit::singleline(&mut self.settings.company_name).desired_width(240.0));
            ui.end_row();
            label(ui, self.t("ui.settings.analyst"));
            ui.add(egui::TextEdit::singleline(&mut self.settings.analyst).desired_width(240.0));
            ui.end_row();
            label(ui, self.t("ui.settings.period"));
            ui.add(egui::DragValue::new(&mut self.settings.default_period_days).range(0..=3650));
            ui.end_row();
            label(ui, self.t("ui.settings.scale"));
            ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.8..=1.6));
            ui.end_row();
        });
        if self.settings.time_zone.parse::<chrono_tz::Tz>().is_err() {
            ui.colored_label(p.bad, "Asia/Seoul, UTC, America/New_York …");
        }
    }

    fn settings_customers(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        if self.customers.is_empty() {
            self.customers.push(Customer { name: "-".into(), ..Default::default() });
        }
        ui.horizontal(|ui| {
            let names: Vec<String> = self.customers.iter().map(|c| c.name.clone()).collect();
            egui::ComboBox::from_id_salt("edit_cust")
                .selected_text(names.get(self.edit_customer).cloned().unwrap_or_default())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.edit_customer, i, n);
                    }
                });
            if ui.button(self.t("ui.form.new_customer")).clicked() {
                self.customers.push(Customer { name: format!("#{}", self.customers.len() + 1), ..Default::default() });
                self.edit_customer = self.customers.len() - 1;
            }
            if self.customers.len() > 1 && ui.button(RichText::new(self.t("ui.delete")).color(p.bad)).clicked() {
                self.customers.remove(self.edit_customer.min(self.customers.len() - 1));
                self.edit_customer = 0;
            }
        });
        ui.add_space(10.0);
        let lang = self.lang;
        let Some(c) = self.customers.get_mut(self.edit_customer) else { return };
        egui::Grid::new("cust").num_columns(2).spacing([22.0, 12.0]).min_col_width(160.0).show(ui, |ui| {
            ui.label(RichText::new(t(lang, "report.customer")).color(p.muted));
            ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(240.0));
            ui.end_row();
            ui.label(RichText::new(t(lang, "ui.settings.work_hours")).color(p.muted));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut c.work_start_hour).range(0..=23));
                ui.label("~");
                ui.add(egui::DragValue::new(&mut c.work_end_hour).range(1..=24));
            });
            ui.end_row();
            ui.label(RichText::new(t(lang, "ui.settings.work_days")).color(p.muted));
            ui.horizontal(|ui| {
                for (i, k) in ["weekday.mon", "weekday.tue", "weekday.wed", "weekday.thu", "weekday.fri", "weekday.sat", "weekday.sun"]
                    .iter()
                    .enumerate()
                {
                    let bit = 1u8 << i;
                    let on = c.work_days_mask & bit != 0;
                    let txt = RichText::new(t(lang, k)).color(if on { p.accent } else { p.muted });
                    if ui.add(egui::Button::selectable(on, txt).corner_radius(CornerRadius::same(8))).clicked() {
                        c.work_days_mask ^= bit;
                    }
                }
            });
            ui.end_row();
        });
        ui.add_space(12.0);
        section(ui, &p, &t(lang, "ui.settings.devices"));
        let mut remove = None;
        egui::Grid::new("devlist").striped(true).num_columns(3).show(ui, |ui| {
            for (i, d) in c.allowed_devices.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut d.id).desired_width(240.0).font(egui::TextStyle::Monospace));
                ui.add(egui::TextEdit::singleline(&mut d.memo).desired_width(200.0));
                if ui.button("✕").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            c.allowed_devices.remove(i);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_device.0).hint_text(t(lang, "ui.settings.device_id")).desired_width(240.0));
            ui.add(egui::TextEdit::singleline(&mut self.new_device.1).hint_text(t(lang, "ui.settings.memo")).desired_width(200.0));
            if ui.button(t(lang, "ui.settings.add")).clicked() && !self.new_device.0.trim().is_empty() {
                c.allowed_devices.push(AllowedDevice { id: self.new_device.0.trim().to_string(), memo: self.new_device.1.trim().to_string() });
                self.new_device = (String::new(), String::new());
            }
        });
    }

    fn settings_advanced(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.colored_label(p.warn, self.t("ui.settings.advanced_hint"));
        ui.add_space(6.0);
        section(ui, &p, &self.t("ui.settings.rules"));
        let lang = self.lang;
        let r = &mut self.settings.rules;
        egui::Grid::new("rules").num_columns(2).spacing([30.0, 6.0]).show(ui, |ui| {
            let toggles: [(&mut bool, &str); 13] = [
                (&mut r.new_storage_device, "R1"),
                (&mut r.after_hours_storage_use, "R2"),
                (&mut r.files_opened_from_removable, "R3"),
                (&mut r.mass_delete, "R4"),
                (&mut r.log_cleared, "R5"),
                (&mut r.time_changed, "R6"),
                (&mut r.portable_device, "R7"),
                (&mut r.cloud_or_webmail, "R8"),
                (&mut r.program_from_removable, "R9"),
                (&mut r.cleanup_tools, "R10"),
                (&mut r.remote_logon, "R11"),
                (&mut r.large_upload, "R12"),
                (&mut r.remote_tools, "R13"),
            ];
            for (i, (on, id)) in toggles.into_iter().enumerate() {
                ui.checkbox(on, format!("{id}  {}", t(lang, &format!("finding.{id}.title"))));
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(8.0);
        egui::Grid::new("thresholds").num_columns(2).spacing([22.0, 10.0]).show(ui, |ui| {
            ui.label(st_i18n::tf(
                lang,
                "ui.settings.mass_delete",
                &[("minutes", &r.mass_delete_window_minutes.to_string()), ("count", &r.mass_delete_count.to_string())],
            ));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut r.mass_delete_window_minutes).range(1..=1440));
                ui.add(egui::DragValue::new(&mut r.mass_delete_count).range(1..=10000));
            });
            ui.end_row();
            ui.label(t(lang, "ui.settings.time_change"));
            ui.add(egui::DragValue::new(&mut r.time_change_minutes).range(1..=100000));
            ui.end_row();
            ui.label(t(lang, "ui.settings.cloud_count"));
            ui.add(egui::DragValue::new(&mut r.cloud_visit_count).range(1..=100000));
            ui.end_row();
            ui.label(t(lang, "ui.settings.upload_mb"));
            ui.add(egui::DragValue::new(&mut r.large_upload_mb).range(1..=1_000_000));
            ui.end_row();
        });
        ui.add_space(10.0);
        section(ui, &p, &t(lang, "ui.settings.domains"));
        ui.horizontal_top(|ui| {
            for (i, k) in ["ui.settings.cloud", "ui.settings.webmail", "ui.settings.share"].iter().enumerate() {
                ui.vertical(|ui| {
                    ui.label(RichText::new(t(lang, k)).color(p.muted));
                    if let Some(text) = self.lists.get_mut(i) {
                        ui.add(egui::TextEdit::multiline(text).desired_width(210.0).desired_rows(7));
                    }
                });
            }
        });
        ui.add_space(10.0);
        section(ui, &p, &t(lang, "ui.settings.tools"));
        ui.horizontal_top(|ui| {
            for (i, k) in [(3usize, "ui.settings.cleanup_list"), (4, "ui.settings.remote_list")] {
                ui.vertical(|ui| {
                    ui.label(RichText::new(t(lang, k)).color(p.muted));
                    if let Some(text) = self.lists.get_mut(i) {
                        ui.add(egui::TextEdit::multiline(text).desired_width(320.0).desired_rows(7));
                    }
                });
            }
        });
        ui.add_space(10.0);
        ui.checkbox(&mut self.settings.mask_names, t(lang, "ui.settings.mask"));
        ui.checkbox(&mut self.settings.keep_collected_copies, t(lang, "ui.settings.keep_copies"));
        ui.checkbox(&mut self.settings.export_full_browser_history, t(lang, "ui.settings.full_browser"));
    }

    // ------------------------------------------------------------ 도움말
    fn ui_help(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        title(ui, &p, &self.t("ui.help.title"), None);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                for (i, line) in self.t("ui.help.steps").lines().enumerate() {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 12.0, p.accent_soft);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, (i + 1).to_string(), bfont(13.0), p.accent);
                        let text = line.split_once(". ").map(|x| x.1).unwrap_or(line);
                        ui.add(egui::Label::new(RichText::new(text).size(15.0)).wrap());
                    });
                    ui.add_space(4.0);
                }
            });
            ui.add_space(10.0);
            section(ui, &p, &self.t("ui.help.grades"));
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                for g in [Grade::NothingFound, Grade::Caution, Grade::Review, Grade::Undetermined] {
                    ui.horizontal(|ui| {
                        dot(ui, p.grade(g), 6.0);
                        ui.label(RichText::new(self.t(g.key())).font(bfont(15.0)).color(p.grade(g)));
                    });
                    let desc = g.key().replace("grade.", "grade.desc.");
                    ui.add(egui::Label::new(RichText::new(self.t(&desc)).color(p.muted)).wrap());
                    ui.add_space(6.0);
                }
            });
            ui.add_space(10.0);
            section(ui, &p, &self.t("report.cannot_check"));
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                for k in ["cannot.1", "cannot.2", "cannot.3", "cannot.4", "cannot.5"] {
                    ui.horizontal(|ui| {
                        dot(ui, p.faint, 3.0);
                        ui.add(egui::Label::new(RichText::new(self.t(k))).wrap());
                    });
                }
            });
            ui.add_space(10.0);
            card(ui, &p, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(self.t("ui.help.smartscreen")).wrap());
                ui.add_space(6.0);
                ui.label(self.t("ui.help.data"));
                ui.label(RichText::new(self.data.display().to_string()).monospace().color(p.muted));
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{} {}", self.t("ui.help.version"), env!("CARGO_PKG_VERSION"))).color(p.muted));
                ui.label(RichText::new("https://github.com/securidge/securidge-trace").color(p.accent));
                ui.add(egui::Label::new(RichText::new(self.t("report.reference_only")).size(12.5).color(p.faint)).wrap());
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let p = self.pal;
        if matches!(self.screen, Screen::Language | Screen::Welcome) {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::same(24))).show(ui, |ui| {
                match self.screen {
                    Screen::Language => self.ui_language(ui),
                    _ => self.ui_welcome(ui),
                }
            });
            return;
        }
        egui::Panel::left("nav")
            .exact_size(224.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.side).inner_margin(Margin::symmetric(14, 0)).stroke(Stroke::new(1.0, p.line)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::symmetric(32, 26))).show(ui, |ui| {
            match self.screen {
                Screen::Home => self.ui_home(ui),
                Screen::Form { offline } => self.ui_form(ui, offline),
                Screen::Progress => self.ui_progress(ui),
                Screen::Result => self.ui_result(ui),
                Screen::History => self.ui_history(ui),
                Screen::Settings => self.ui_settings(ui),
                Screen::Help => self.ui_help(ui),
                Screen::Language | Screen::Welcome => {}
            }
        });
    }
}

/// Windows 표시 언어가 한국어면 한국어, 아니면 영어.
fn system_lang() -> Lang {
    let candidates = [std::env::var("LANG").ok(), std::env::var("LC_ALL").ok()];
    if candidates.iter().flatten().any(|v| v.to_ascii_lowercase().starts_with("ko")) {
        return Lang::Ko;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(o) = std::process::Command::new("reg")
            .args(["query", "HKCU\\Control Panel\\International", "/v", "LocaleName"])
            .creation_flags(0x0800_0000)
            .output()
        {
            if String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("ko-kr") {
                return Lang::Ko;
            }
        }
        Lang::En
    }
    #[cfg(not(windows))]
    {
        Lang::Ko
    }
}

/// 관리자 권한으로 다시 실행하고 지금 창을 닫는다.
fn restart_as_admin(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(exe) = std::env::current_exe() {
            let cmd = format!("Start-Process -FilePath '{}' -Verb RunAs", exe.display().to_string().replace('\'', "''"));
            let ok = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd])
                .creation_flags(0x0800_0000)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ctx;
    }
}
