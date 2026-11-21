//! Securidge Trace 화면 프로그램.
//!
//! 대상: IT 지식이 없는 유지보수 업체 직원. 설명서 없이 "언어 선택 → 점검 시작 → 보고서 받기".
//! 화면 흐름: 첫 실행(언어·안내) → 홈 → 점검 정보 → 진행 → 결과 / 지난 내역 / 설정 / 도움말
//! 모든 문구는 st-i18n의 ko.json·en.json에서 온다.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};
use st_core::settings::{AllowedDevice, Customer, Settings};
use st_engine::store::{self, HistoryEntry};
use st_engine::{CheckParams, CheckResult, Mode, Stage};
use st_i18n::{t, Lang};
use st_rules::Grade;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Securidge Trace")
            .with_inner_size([980.0, 720.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("Securidge Trace", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

// ---------------------------------------------------------------- 글꼴

/// 한글 글꼴을 운영체제에서 찾아 넣는다 (Windows: 맑은 고딕).
fn install_fonts(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\gulim.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];
    let mut fonts = egui::FontDefinitions::default();
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("korean".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().insert(0, "korean".to_owned());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

/// 밝은 테마와 큰 글자 (비전문가용 업무 화면, 인쇄물과 같은 느낌).
fn install_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(24.0)),
            (TextStyle::Body, FontId::proportional(16.0)),
            (TextStyle::Button, FontId::proportional(16.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
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
    screen: Screen,
    admin: bool,
    form: Form,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    result: Option<CheckResult>,
    error: Option<String>,
    flash: Option<String>,
    history: Vec<HistoryEntry>,
    history_filter: String,
    confirm_delete: Option<PathBuf>,
    tab: SettingsTab,
    edit_customer: usize,
    new_device: (String, String),
    domains: [String; 3],
}

fn lang_of(code: &str) -> Lang {
    Lang::parse(code).unwrap_or(Lang::Ko)
}

fn grade_color(g: Grade) -> Color32 {
    match g {
        Grade::NothingFound => Color32::from_rgb(31, 138, 76),
        Grade::Caution => Color32::from_rgb(201, 138, 0),
        Grade::Review => Color32::from_rgb(198, 47, 47),
        Grade::Undetermined => Color32::from_rgb(107, 114, 128),
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        install_style(&cc.egui_ctx);
        let data = store::data_dir();
        let settings = store::load_settings(&data);
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
        let domains = [
            settings.rules.cloud_domains.join("\n"),
            settings.rules.webmail_domains.join("\n"),
            settings.rules.file_share_domains.join("\n"),
        ];
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
            data,
            settings,
            customers,
            lang,
            screen,
            shared: Arc::new(Mutex::new(Shared::default())),
            cancel: Arc::new(AtomicBool::new(false)),
            result: None,
            error: None,
            flash: None,
            history: Vec::new(),
            history_filter: String::new(),
            confirm_delete: None,
            tab: SettingsTab::Basic,
            edit_customer: 0,
            new_device: (String::new(), String::new()),
            domains,
        }
    }

    fn t(&self, k: &str) -> String {
        t(self.lang, k)
    }

    fn save_settings(&mut self) {
        self.settings.ui_lang = self.lang.code().to_string();
        match store::save_settings(&self.data, &self.settings) {
            Ok(()) => {}
            Err(e) => self.flash = Some(e),
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

    // ------------------------------------------------------------ 공통 머리
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new(self.t("app.name")).strong());
            ui.label(RichText::new(self.t("app.tagline")).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(self.lang == Lang::En, "English").clicked() {
                    self.set_lang(Lang::En);
                }
                if ui.selectable_label(self.lang == Lang::Ko, "한국어").clicked() {
                    self.set_lang(Lang::Ko);
                }
            });
        });
        ui.separator();
    }

    // ------------------------------------------------------------ 화면들
    fn ui_language(&mut self, ui: &mut egui::Ui) {
        ui.add_space(120.0);
        ui.vertical_centered(|ui| {
            for line in self.t("ui.choose_lang").lines() {
                ui.heading(line);
            }
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 170.0);
                if ui.add_sized([160.0, 56.0], egui::Button::new(RichText::new("한국어").size(20.0))).clicked() {
                    self.lang = Lang::Ko;
                    self.settings.report_lang = "ko".into();
                    self.form.report_lang = Lang::Ko;
                    self.save_settings();
                    self.screen = Screen::Welcome;
                }
                ui.add_space(20.0);
                if ui.add_sized([160.0, 56.0], egui::Button::new(RichText::new("English").size(20.0))).clicked() {
                    self.lang = Lang::En;
                    self.settings.report_lang = "en".into();
                    self.form.report_lang = Lang::En;
                    self.save_settings();
                    self.screen = Screen::Welcome;
                }
            });
        });
    }

    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.add_space(20.0);
        ui.heading(self.t("ui.welcome.title"));
        ui.add_space(10.0);
        for k in ["ui.welcome.1", "ui.welcome.2", "ui.welcome.3"] {
            ui.label(RichText::new(format!("•  {}", self.t(k))).size(16.0));
            ui.add_space(4.0);
        }
        ui.add_space(16.0);
        let mut agree = self.settings.consent_accepted;
        ui.checkbox(&mut agree, RichText::new(self.t("ui.welcome.agree")).size(16.0));
        self.settings.consent_accepted = agree;
        ui.add_space(10.0);
        if ui.add_enabled(agree, egui::Button::new(RichText::new(self.t("ui.start")).size(18.0)).min_size([160.0, 44.0].into())).clicked() {
            self.save_settings();
            self.screen = Screen::Home;
        }
    }

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            let big = egui::Button::new(RichText::new(self.t("ui.home.check")).size(26.0).strong().color(Color32::WHITE))
                .min_size([420.0, 120.0].into())
                .fill(Color32::from_rgb(29, 36, 51));
            if ui.add(big).clicked() {
                self.form.consent = false;
                self.error = None;
                self.screen = Screen::Form { offline: false };
            }
            ui.add_space(12.0);
            if cfg!(windows) {
                if self.admin {
                    ui.label(RichText::new(self.t("ui.home.admin_ok")).color(Color32::from_rgb(31, 138, 76)));
                } else {
                    ui.label(RichText::new(self.t("ui.home.admin_no")).color(Color32::from_rgb(201, 138, 0)));
                    if ui.button(self.t("ui.home.restart_admin")).clicked() {
                        restart_as_admin(ui.ctx());
                    }
                }
            }
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 260.0);
                for (key, screen) in [
                    ("ui.home.history", Screen::History),
                    ("ui.home.settings", Screen::Settings),
                    ("ui.home.help", Screen::Help),
                ] {
                    if ui.add_sized([160.0, 48.0], egui::Button::new(RichText::new(self.t(key)).size(17.0))).clicked() {
                        if screen == Screen::History {
                            self.history = store::list_history(&store::history_dir(&self.data));
                        }
                        self.flash = None;
                        self.screen = screen;
                    }
                }
            });
            ui.add_space(30.0);
            if ui.link(self.t("ui.home.offline")).clicked() {
                self.form.consent = false;
                self.error = None;
                self.screen = Screen::Form { offline: true };
            }
        });
    }

    fn ui_form(&mut self, ui: &mut egui::Ui, offline: bool) {
        self.header(ui);
        ui.heading(self.t("ui.form.title"));
        ui.add_space(8.0);
        egui::Grid::new("form").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
            if offline {
                ui.label(self.t("ui.form.input"));
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.form.input).desired_width(420.0));
                    ui.label(RichText::new(self.t("ui.form.input_hint")).weak().small());
                });
                ui.end_row();
            }
            ui.label(self.t("ui.form.customer"));
            ui.horizontal(|ui| {
                let names: Vec<String> = self.customers.iter().map(|c| c.name.clone()).collect();
                let current = names.get(self.form.customer_idx).cloned().unwrap_or_else(|| "-".to_string());
                egui::ComboBox::from_id_salt("cust").selected_text(current).width(220.0).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.form.customer_idx, i, n);
                    }
                });
                let hint = self.t("ui.form.new_customer");
                ui.add(egui::TextEdit::singleline(&mut self.form.new_customer).hint_text(hint).desired_width(160.0));
                if ui.button(self.t("ui.settings.add")).clicked() && !self.form.new_customer.trim().is_empty() {
                    self.customers.push(Customer { name: self.form.new_customer.trim().to_string(), ..Default::default() });
                    self.form.customer_idx = self.customers.len() - 1;
                    self.form.new_customer.clear();
                    self.save_customers();
                }
            });
            ui.end_row();
            ui.label(self.t("ui.form.pc"));
            ui.add(egui::TextEdit::singleline(&mut self.form.pc_name).desired_width(260.0));
            ui.end_row();
            ui.label(self.t("ui.form.analyst"));
            ui.add(egui::TextEdit::singleline(&mut self.form.analyst).desired_width(260.0));
            ui.end_row();
            ui.label(self.t("ui.form.period"));
            ui.horizontal(|ui| {
                for (days, key) in [(30, "ui.form.days30"), (90, "ui.form.days90"), (365, "ui.form.days365"), (0, "ui.form.all")] {
                    let label = self.t(key);
                    ui.radio_value(&mut self.form.period_days, days, label);
                }
            });
            ui.end_row();
            ui.label(self.t("ui.form.report_lang"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.form.report_lang, Lang::Ko, "한국어");
                ui.radio_value(&mut self.form.report_lang, Lang::En, "English");
            });
            ui.end_row();
        });
        ui.add_space(12.0);
        let consent_label = RichText::new(self.t("ui.form.consent")).size(16.0).strong();
        ui.checkbox(&mut self.form.consent, consent_label);
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(198, 47, 47), e);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.add_sized([120.0, 40.0], egui::Button::new(self.t("ui.back"))).clicked() {
                self.screen = Screen::Home;
            }
            let run = egui::Button::new(RichText::new(self.t("ui.run")).size(18.0).strong()).min_size([200.0, 40.0].into());
            if ui.add_enabled(self.form.consent, run).clicked() {
                self.start(ui.ctx(), offline);
            }
        });
        if !self.form.consent {
            ui.label(RichText::new(self.t("ui.err.no_consent")).weak());
        }
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
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress = |stage: Stage, msg: &str| {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.stage = Some(stage);
                s.message = msg.to_string();
                ctx.request_repaint();
            };
            let r = st_engine::run_check(&params, &progress, &cancel);
            shared.lock().unwrap_or_else(|e| e.into_inner()).done = Some(r);
            ctx.request_repaint();
        });
        self.result = None;
        self.error = None;
        self.screen = Screen::Progress;
    }

    fn ui_progress(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
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
            self.screen = Screen::Result;
            return;
        }
        ui.add_space(40.0);
        ui.heading(self.t("ui.progress.title"));
        ui.add_space(16.0);
        let order = [Stage::Collect, Stage::Analyze, Stage::Report];
        let pos = stage.and_then(|s| order.iter().position(|x| *x == s)).unwrap_or(0);
        for (i, (st, key)) in order.iter().zip(["ui.progress.collect", "ui.progress.analyze", "ui.progress.report"]).enumerate() {
            let mark = if i < pos { "✔" } else if i == pos { "▶" } else { "·" };
            let text = format!("{mark}  {}. {}", i + 1, self.t(key));
            let rich = if Some(*st) == stage { RichText::new(text).size(18.0).strong() } else { RichText::new(text).size(18.0) };
            ui.label(rich);
        }
        ui.add_space(10.0);
        ui.add(egui::ProgressBar::new((pos as f32 + 0.5) / 3.0).animate(true).desired_width(500.0));
        if !message.is_empty() {
            ui.label(RichText::new(message).weak().monospace());
        }
        ui.add_space(20.0);
        if self.cancel.load(Ordering::Relaxed) {
            ui.label(self.t("ui.progress.cancelling"));
        } else if ui.add_sized([120.0, 36.0], egui::Button::new(self.t("ui.cancel"))).clicked() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn ui_result(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        if let Some(e) = self.error.clone() {
            ui.heading(RichText::new(self.t("ui.result.error")).color(Color32::from_rgb(198, 47, 47)));
            ui.label(e);
            ui.add_space(10.0);
            if ui.button(self.t("ui.result.home")).clicked() {
                self.screen = Screen::Home;
            }
            return;
        }
        let Some(res) = self.result.clone() else {
            self.screen = Screen::Home;
            return;
        };
        let a = &res.case.assessment;
        let tz: chrono_tz::Tz = res.case.info.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size([44.0, 44.0].into(), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 21.0, grade_color(a.grade));
            ui.vertical(|ui| {
                ui.label(RichText::new(self.t(a.grade.key())).size(24.0).strong());
                ui.label(format!(
                    "{}: {}",
                    self.t("report.created"),
                    st_i18n::format_stamp_with_zone(res.case.info.finished_utc, tz, self.lang)
                ));
            });
        });
        for w in &res.warnings {
            ui.colored_label(Color32::from_rgb(201, 138, 0), format!("! {w}"));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_sized([150.0, 40.0], egui::Button::new(RichText::new(self.t("ui.result.open")).strong())).clicked() {
                self.flash = st_engine::open_path(&res.report_path).err();
            }
            if ui.add_sized([110.0, 40.0], egui::Button::new(self.t("ui.result.print"))).clicked() {
                self.flash = st_engine::open_path(&res.report_path).err();
            }
            if ui.add_sized([150.0, 40.0], egui::Button::new(self.t("ui.result.folder"))).clicked() {
                self.flash = st_engine::open_path(&res.dir).err();
            }
            if ui.add_sized([210.0, 40.0], egui::Button::new(self.t("ui.result.regen"))).clicked() {
                let other = if res.report_path.to_string_lossy().ends_with("_ko.html") { Lang::En } else { Lang::Ko };
                match st_report::regenerate(&res.dir, other) {
                    Ok(p) => self.flash = st_engine::open_path(&p).err(),
                    Err(e) => self.flash = Some(e),
                }
            }
        });
        if let Some(f) = &self.flash {
            ui.colored_label(Color32::from_rgb(198, 47, 47), f);
        }
        ui.add_space(10.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(self.t("report.key_events"));
            let key: Vec<_> = a.findings.iter().filter(|f| f.level >= st_rules::Level::Medium).take(5).collect();
            if key.is_empty() {
                ui.label(self.t("report.no_key_events"));
            }
            for (i, f) in key.iter().enumerate() {
                ui.label(RichText::new(format!("{}. {}", i + 1, st_report::html::finding_sentence(f, self.lang, tz))).size(15.0));
            }
            ui.add_space(12.0);
            ui.heading(self.t("ui.result.devices"));
            let cust_idx = self.form.customer_idx;
            let mut register: Option<(String, String)> = None;
            egui::Grid::new("devs").striped(true).num_columns(4).show(ui, |ui| {
                for d in &res.case.analysis.devices {
                    let name = [d.vendor.clone(), d.product.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ");
                    ui.label(t(self.lang, &format!("device_kind.{:?}", d.kind)));
                    ui.label(if name.is_empty() { d.friendly_name.clone().unwrap_or_default() } else { name.clone() });
                    ui.label(RichText::new(d.serial.clone().unwrap_or_default()).monospace());
                    let serial = d.serial.clone().unwrap_or_default();
                    let already = self.customers.get(cust_idx).is_some_and(|c| c.is_allowed(&serial));
                    if already {
                        ui.label(self.t("ui.result.registered"));
                    } else if !serial.is_empty() && ui.button(self.t("ui.result.register")).clicked() {
                        register = Some((serial, name));
                    }
                    ui.end_row();
                }
            });
            if res.case.analysis.devices.is_empty() {
                ui.label(self.t("report.none"));
            }
            if let Some((id, memo)) = register {
                if self.customers.is_empty() {
                    self.customers.push(Customer { name: res.case.info.customer.clone(), ..Default::default() });
                }
                if let Some(c) = self.customers.get_mut(cust_idx).or(None) {
                    c.allowed_devices.push(AllowedDevice { id, memo });
                }
                self.save_customers();
            }
        });
        ui.add_space(8.0);
        if ui.button(self.t("ui.result.home")).clicked() {
            self.screen = Screen::Home;
        }
    }

    fn ui_history(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.horizontal(|ui| {
            ui.heading(self.t("ui.history.title"));
            ui.add_space(20.0);
            ui.label(self.t("ui.history.search"));
            ui.text_edit_singleline(&mut self.history_filter);
            if ui.button(self.t("ui.back")).clicked() {
                self.screen = Screen::Home;
            }
        });
        ui.label(RichText::new(self.t("ui.history.privacy")).weak());
        if let Some(f) = &self.flash {
            ui.colored_label(Color32::from_rgb(198, 47, 47), f);
        }
        ui.separator();
        if self.history.is_empty() {
            ui.label(self.t("ui.history.empty"));
            return;
        }
        let tz: chrono_tz::Tz = self.settings.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
        let filter = self.history_filter.to_lowercase();
        let mut action: Option<(u8, PathBuf, Option<Lang>)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("hist").striped(true).num_columns(6).spacing([14.0, 8.0]).show(ui, |ui| {
                for h in [self.t("ui.history.when"), self.t("report.customer"), self.t("report.pc"), self.t("report.analyst"), self.t("report.result"), String::new()] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for e in &self.history {
                    let hay = format!("{} {} {}", e.customer, e.pc_name, e.analyst).to_lowercase();
                    if !filter.is_empty() && !hay.contains(&filter) {
                        continue;
                    }
                    ui.label(st_i18n::format_stamp(e.started, tz, self.lang));
                    ui.label(&e.customer);
                    ui.label(&e.pc_name);
                    ui.label(&e.analyst);
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size([12.0, 12.0].into(), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 6.0, grade_color(e.grade));
                        ui.label(t(self.lang, e.grade.key()));
                        if !e.changed.is_empty() {
                            ui.colored_label(Color32::from_rgb(198, 47, 47), t(self.lang, "ui.history.changed"));
                        }
                        if e.old {
                            ui.colored_label(Color32::from_rgb(201, 138, 0), t(self.lang, "ui.history.old"));
                        }
                    });
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if ui.button(t(self.lang, "ui.result.open")).clicked() {
                                let lang = if e.reports.iter().any(|r| r == &format!("report_{}.html", self.lang.code())) { self.lang } else if e.reports.iter().any(|r| r == "report_ko.html") { Lang::Ko } else { Lang::En };
                                action = Some((0, e.dir.clone(), Some(lang)));
                            }
                            if ui.button(t(self.lang, "ui.result.folder")).clicked() {
                                action = Some((2, e.dir.clone(), None));
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button(t(self.lang, "ui.result.regen")).clicked() {
                                let other = if e.reports.iter().any(|r| r == "report_ko.html") && !e.reports.iter().any(|r| r == "report_en.html") { Lang::En } else { Lang::Ko };
                                action = Some((1, e.dir.clone(), Some(other)));
                            }
                            if ui.button(RichText::new(t(self.lang, "ui.delete")).color(Color32::from_rgb(198, 47, 47))).clicked() {
                                action = Some((3, e.dir.clone(), None));
                            }
                        });
                    });
                    ui.end_row();
                }
            });
        });
        match action {
            Some((0, dir, Some(l))) => self.flash = st_engine::open_path(&dir.join(format!("report_{}.html", l.code()))).err(),
            Some((1, dir, Some(l))) => {
                match st_report::regenerate(&dir, l) {
                    Ok(p) => self.flash = st_engine::open_path(&p).err(),
                    Err(e) => self.flash = Some(e),
                }
                self.history = store::list_history(&store::history_dir(&self.data));
            }
            Some((2, dir, _)) => self.flash = st_engine::open_path(&dir).err(),
            Some((3, dir, _)) => self.confirm_delete = Some(dir),
            _ => {}
        }
        if let Some(dir) = self.confirm_delete.clone() {
            egui::Window::new(self.t("ui.delete")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
                ui.label(self.t("ui.history.confirm_delete"));
                ui.label(RichText::new(dir.display().to_string()).weak().small());
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(self.t("ui.delete")).color(Color32::from_rgb(198, 47, 47))).clicked() {
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

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.horizontal(|ui| {
            ui.heading(self.t("ui.settings.title"));
            ui.add_space(20.0);
            for (tab, key) in [(SettingsTab::Basic, "ui.settings.basic"), (SettingsTab::Customers, "ui.settings.customers"), (SettingsTab::Advanced, "ui.settings.advanced")] {
                if ui.selectable_label(self.tab == tab, self.t(key)).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| match self.tab {
            SettingsTab::Basic => self.settings_basic(ui),
            SettingsTab::Customers => self.settings_customers(ui),
            SettingsTab::Advanced => self.settings_advanced(ui),
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_sized([110.0, 36.0], egui::Button::new(RichText::new(self.t("ui.save")).strong())).clicked() {
                let split = |s: &str| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
                self.settings.rules.cloud_domains = split(&self.domains[0]);
                self.settings.rules.webmail_domains = split(&self.domains[1]);
                self.settings.rules.file_share_domains = split(&self.domains[2]);
                self.save_settings();
                self.save_customers();
                ui.ctx().set_zoom_factor(self.settings.ui_scale.clamp(0.8, 2.0));
                self.flash = Some(self.t("ui.settings.saved"));
            }
            if ui.button(self.t("ui.settings.reset")).clicked() {
                let keep_lang = self.settings.ui_lang.clone();
                self.settings = Settings { ui_lang: keep_lang, consent_accepted: true, ..Settings::default() };
                self.domains = [
                    self.settings.rules.cloud_domains.join("\n"),
                    self.settings.rules.webmail_domains.join("\n"),
                    self.settings.rules.file_share_domains.join("\n"),
                ];
            }
            if ui.button(self.t("ui.settings.export")).clicked() {
                let to = self.data.join("securidge-trace-settings.json");
                self.flash = Some(match store::export_bundle(&self.data, &to) {
                    Ok(()) => to.display().to_string(),
                    Err(e) => e,
                });
            }
            if ui.button(self.t("ui.settings.import")).clicked() {
                let from = self.data.join("securidge-trace-settings.json");
                self.flash = Some(match store::import_bundle(&self.data, &from) {
                    Ok(()) => {
                        self.settings = store::load_settings(&self.data);
                        self.customers = store::load_customers(&self.data);
                        self.t("ui.settings.saved")
                    }
                    Err(e) => format!("{e} ({})", from.display()),
                });
            }
            if ui.button(self.t("ui.back")).clicked() {
                self.flash = None;
                self.screen = Screen::Home;
            }
            if let Some(f) = &self.flash {
                ui.label(f);
            }
        });
    }

    fn settings_basic(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("basic").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(self.t("ui.settings.ui_lang"));
            ui.horizontal(|ui| {
                if ui.radio(self.lang == Lang::Ko, "한국어").clicked() {
                    self.lang = Lang::Ko;
                }
                if ui.radio(self.lang == Lang::En, "English").clicked() {
                    self.lang = Lang::En;
                }
            });
            ui.end_row();
            ui.label(self.t("ui.settings.report_lang"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.settings.report_lang, "ko".to_string(), "한국어");
                ui.radio_value(&mut self.settings.report_lang, "en".to_string(), "English");
            });
            ui.end_row();
            ui.label(self.t("ui.settings.tz"));
            ui.text_edit_singleline(&mut self.settings.time_zone);
            ui.end_row();
            ui.label(self.t("ui.settings.company"));
            ui.text_edit_singleline(&mut self.settings.company_name);
            ui.end_row();
            ui.label(self.t("ui.settings.analyst"));
            ui.text_edit_singleline(&mut self.settings.analyst);
            ui.end_row();
            ui.label(self.t("ui.settings.period"));
            ui.add(egui::DragValue::new(&mut self.settings.default_period_days).range(0..=3650));
            ui.end_row();
            ui.label(self.t("ui.settings.scale"));
            ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.8..=1.6));
            ui.end_row();
        });
        if self.settings.time_zone.parse::<chrono_tz::Tz>().is_err() {
            ui.colored_label(Color32::from_rgb(198, 47, 47), "Asia/Seoul, UTC, America/New_York …");
        }
    }

    fn settings_customers(&mut self, ui: &mut egui::Ui) {
        if self.customers.is_empty() {
            self.customers.push(Customer { name: "-".into(), ..Default::default() });
        }
        ui.horizontal(|ui| {
            let names: Vec<String> = self.customers.iter().map(|c| c.name.clone()).collect();
            egui::ComboBox::from_id_salt("edit_cust")
                .selected_text(names.get(self.edit_customer).cloned().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.edit_customer, i, n);
                    }
                });
            if ui.button(self.t("ui.form.new_customer")).clicked() {
                self.customers.push(Customer { name: format!("#{}", self.customers.len() + 1), ..Default::default() });
                self.edit_customer = self.customers.len() - 1;
            }
            if self.customers.len() > 1 && ui.button(self.t("ui.delete")).clicked() {
                self.customers.remove(self.edit_customer.min(self.customers.len() - 1));
                self.edit_customer = 0;
            }
        });
        let lang = self.lang;
        let Some(c) = self.customers.get_mut(self.edit_customer) else { return };
        egui::Grid::new("cust").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(t(lang, "report.customer"));
            ui.text_edit_singleline(&mut c.name);
            ui.end_row();
            ui.label(t(lang, "ui.settings.work_hours"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut c.work_start_hour).range(0..=23));
                ui.label("~");
                ui.add(egui::DragValue::new(&mut c.work_end_hour).range(1..=24));
            });
            ui.end_row();
            ui.label(t(lang, "ui.settings.work_days"));
            ui.horizontal(|ui| {
                for (i, k) in ["weekday.mon", "weekday.tue", "weekday.wed", "weekday.thu", "weekday.fri", "weekday.sat", "weekday.sun"].iter().enumerate() {
                    let bit = 1u8 << i;
                    let mut on = c.work_days_mask & bit != 0;
                    if ui.checkbox(&mut on, t(lang, k)).changed() {
                        if on {
                            c.work_days_mask |= bit;
                        } else {
                            c.work_days_mask &= !bit;
                        }
                    }
                }
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.label(RichText::new(t(lang, "ui.settings.devices")).strong());
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
        ui.colored_label(Color32::from_rgb(201, 138, 0), self.t("ui.settings.advanced_hint"));
        ui.add_space(6.0);
        ui.label(RichText::new(self.t("ui.settings.rules")).strong());
        let lang = self.lang;
        let r = &mut self.settings.rules;
        for (on, id) in [
            (&mut r.new_storage_device, "R1"),
            (&mut r.after_hours_storage_use, "R2"),
            (&mut r.files_opened_from_removable, "R3"),
            (&mut r.mass_delete, "R4"),
            (&mut r.log_cleared, "R5"),
            (&mut r.time_changed, "R6"),
            (&mut r.portable_device, "R7"),
            (&mut r.cloud_or_webmail, "R8"),
        ] {
            ui.checkbox(on, format!("{id}  {}", t(lang, &format!("finding.{id}.title"))));
        }
        ui.horizontal(|ui| {
            ui.label(st_i18n::tf(lang, "ui.settings.mass_delete", &[("minutes", &r.mass_delete_window_minutes.to_string()), ("count", &r.mass_delete_count.to_string())]));
            ui.add(egui::DragValue::new(&mut r.mass_delete_window_minutes).range(1..=1440));
            ui.add(egui::DragValue::new(&mut r.mass_delete_count).range(1..=10000));
        });
        ui.horizontal(|ui| {
            ui.label(t(lang, "ui.settings.time_change"));
            ui.add(egui::DragValue::new(&mut r.time_change_minutes).range(1..=100000));
        });
        ui.horizontal(|ui| {
            ui.label(t(lang, "ui.settings.cloud_count"));
            ui.add(egui::DragValue::new(&mut r.cloud_visit_count).range(1..=100000));
        });
        ui.add_space(8.0);
        ui.label(RichText::new(t(lang, "ui.settings.domains")).strong());
        ui.horizontal(|ui| {
            for (i, k) in ["ui.settings.cloud", "ui.settings.webmail", "ui.settings.share"].iter().enumerate() {
                ui.vertical(|ui| {
                    ui.label(t(lang, k));
                    if let Some(text) = self.domains.get_mut(i) {
                        ui.add(egui::TextEdit::multiline(text).desired_width(220.0).desired_rows(8));
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.checkbox(&mut self.settings.mask_names, t(lang, "ui.settings.mask"));
        ui.checkbox(&mut self.settings.keep_collected_copies, t(lang, "ui.settings.keep_copies"));
        ui.checkbox(&mut self.settings.export_full_browser_history, t(lang, "ui.settings.full_browser"));
    }

    fn ui_help(&mut self, ui: &mut egui::Ui) {
        self.header(ui);
        ui.heading(self.t("ui.help.title"));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self.t("ui.help.steps").lines() {
                ui.label(RichText::new(line).size(16.0));
            }
            ui.add_space(12.0);
            ui.label(RichText::new(self.t("ui.help.grades")).strong());
            for g in [Grade::NothingFound, Grade::Caution, Grade::Review, Grade::Undetermined] {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size([14.0, 14.0].into(), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 7.0, grade_color(g));
                    ui.label(RichText::new(self.t(g.key())).strong());
                });
                let desc = g.key().replace("grade.", "grade.desc.");
                ui.label(self.t(&desc));
            }
            ui.add_space(12.0);
            ui.label(RichText::new(self.t("report.cannot_check")).strong());
            for k in ["cannot.1", "cannot.2", "cannot.3", "cannot.4", "cannot.5"] {
                ui.label(format!("•  {}", self.t(k)));
            }
            ui.add_space(12.0);
            ui.label(self.t("ui.help.smartscreen"));
            ui.label(self.t("ui.help.data"));
            ui.label(RichText::new(self.data.display().to_string()).monospace().weak());
            ui.add_space(8.0);
            ui.label(format!("{}: {}", self.t("ui.help.version"), env!("CARGO_PKG_VERSION")));
            ui.label(RichText::new(self.t("report.reference_only")).weak());
        });
        if ui.button(self.t("ui.back")).clicked() {
            self.screen = Screen::Home;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| match self.screen {
            Screen::Language => self.ui_language(ui),
            Screen::Welcome => self.ui_welcome(ui),
            Screen::Home => self.ui_home(ui),
            Screen::Form { offline } => self.ui_form(ui, offline),
            Screen::Progress => self.ui_progress(ui),
            Screen::Result => self.ui_result(ui),
            Screen::History => self.ui_history(ui),
            Screen::Settings => self.ui_settings(ui),
            Screen::Help => self.ui_help(ui),
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
        // PowerShell 없이 확인: 사용자 기본 UI 언어 ID (0x0412 = 한국어)
        if let Ok(o) = std::process::Command::new("cmd").args(["/c", "reg", "query", "HKCU\\Control Panel\\International", "/v", "LocaleName"]).output() {
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
