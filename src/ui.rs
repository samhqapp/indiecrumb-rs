//! Ready UI for GPUI apps (feature `gpui`): the "Report a problem" form and the sharing switch row
//! for Settings. Each looks native on its platform: an Apple grouped form on macOS, Fluent on
//! Windows, libadwaita on Linux. The app calls `gpui_kit::init` first, as it does for any Kit app.
//! Nothing here sends: the app gets the [`Report`] and sends it off the UI thread.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::gpui::accesskit::Toggled;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, rgba, AnyElement, App, AppContext as _, BoxShadow, ClickEvent, Context, Div, Entity, FocusHandle,
    FontWeight, Hsla, KeyDownEvent,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, WindowAppearance,
};
use std::rc::Rc;

/// Which platform look to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Apple grouped form, SF font.
    Mac,
    /// Fluent (WinUI 3), Segoe UI Variable.
    Windows,
    /// GNOME libadwaita, Cantarell.
    Gnome,
}

impl Look {
    /// The look of the platform this binary is built for.
    pub fn native() -> Look {
        if cfg!(target_os = "macos") {
            Look::Mac
        } else if cfg!(target_os = "windows") {
            Look::Windows
        } else {
            Look::Gnome
        }
    }
}

/// A look plus Light or Dark. `accent` starts as the platform's default blue; GPUI does not read
/// the user's accent color, so pass it in when the app knows it.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub look: Look,
    pub dark: bool,
    pub accent: Hsla,
}

impl Theme {
    pub fn new(look: Look, dark: bool) -> Theme {
        let accent = match (look, dark) {
            (Look::Mac, false) => 0x007AFFFF,
            (Look::Mac, true) => 0x0A84FFFF,
            (Look::Windows, false) => 0x005FB8FF,
            (Look::Windows, true) => 0x60CDFFFF,
            (Look::Gnome, _) => 0x3584E4FF,
        };
        Theme { look, dark, accent: rgba(accent).into() }
    }

    /// The native look in the window's current appearance.
    pub fn of(window: &Window) -> Theme {
        let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
        Theme::new(Look::native(), dark)
    }

    fn palette(&self) -> Palette {
        let c = |v: u32| -> Hsla { rgba(v).into() };
        let d = self.dark;
        let pick = |light: u32, dark: u32| c(if d { dark } else { light });
        match self.look {
            Look::Mac => Palette {
                font: ".SystemUIFont",
                text: 13.0,
                title: 13.0,
                window: pick(0xECECECFF, 0x282828FF),
                group: pick(0xF7F7F7FF, 0x323232FF),
                border: pick(0x0000001A, 0xFFFFFF1A),
                label: pick(0x000000D9, 0xFFFFFFD9),
                secondary: pick(0x00000080, 0xFFFFFF8C),
                field: pick(0xFFFFFFFF, 0x1E1E1EFF),
                button: pick(0xFFFFFFFF, 0x5C5C5CFF),
                switch_off: pick(0x00000017, 0xFFFFFF26),
                knob: c(0xFFFFFFFF),
                on_accent: c(0xFFFFFFFF),
                tip: pick(0xF4F4F4FF, 0x3A3A3AFF),
                tip_text: pick(0x000000D9, 0xFFFFFFD9),
                group_radius: 10.0,
                control_radius: 6.0,
                control_h: 24.0,
                row_h: 40.0,
                switch: (26.0, 15.0, 13.0),
            },
            Look::Windows => Palette {
                font: "Segoe UI Variable Text",
                text: 14.0,
                title: 20.0,
                window: pick(0xF3F3F3FF, 0x202020FF),
                group: pick(0xFBFBFBFF, 0x2B2B2BFF),
                border: pick(0x0000000F, 0xFFFFFF14),
                label: pick(0x000000E4, 0xFFFFFFFF),
                secondary: pick(0x0000009E, 0xFFFFFFC8),
                field: pick(0xFFFFFFFF, 0x2D2D2DFF),
                button: pick(0xFFFFFFFF, 0x2D2D2DFF),
                switch_off: pick(0x00000000, 0x00000000),
                knob: pick(0xFFFFFFFF, 0x000000FF),
                on_accent: pick(0xFFFFFFFF, 0x000000FF),
                tip: pick(0xF9F9F9FF, 0x2C2C2CFF),
                tip_text: pick(0x000000E4, 0xFFFFFFFF),
                group_radius: 8.0,
                control_radius: 4.0,
                control_h: 32.0,
                row_h: 48.0,
                switch: (40.0, 20.0, 12.0),
            },
            Look::Gnome => Palette {
                font: "Cantarell",
                text: 15.0,
                title: 15.0,
                window: pick(0xFAFAFBFF, 0x222226FF),
                group: pick(0xFFFFFFFF, 0x36363AFF),
                border: pick(0x0000060F, 0xFFFFFF14),
                label: pick(0x000006CC, 0xFFFFFFFF),
                secondary: pick(0x00000680, 0xFFFFFF8C),
                field: pick(0x0000060D, 0xFFFFFF14),
                button: pick(0x00000614, 0xFFFFFF1A),
                switch_off: pick(0x00000626, 0xFFFFFF33),
                knob: c(0xFFFFFFFF),
                on_accent: c(0xFFFFFFFF),
                tip: c(0x000000CC),
                tip_text: c(0xFFFFFFFF),
                group_radius: 12.0,
                control_radius: 6.0,
                control_h: 34.0,
                row_h: 50.0,
                switch: (46.0, 26.0, 20.0),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    font: &'static str,
    text: f32,
    title: f32,
    window: Hsla,
    group: Hsla,
    border: Hsla,
    label: Hsla,
    secondary: Hsla,
    field: Hsla,
    button: Hsla,
    switch_off: Hsla,
    knob: Hsla,
    on_accent: Hsla,
    tip: Hsla,
    tip_text: Hsla,
    group_radius: f32,
    control_radius: f32,
    control_h: f32,
    row_h: f32,
    /// Track width, track height, knob size.
    switch: (f32, f32, f32),
}

type Toggle = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// A box of rows: fill, a hairline on all four sides drawn inside, rounded.
fn group(p: &Palette) -> Div {
    div().flex().flex_col().w_full().bg(p.group).border_1().border_color(p.border).rounded(px(p.group_radius))
}

fn hairline(p: &Palette) -> Div {
    div().h(px(1.0)).w_full().bg(p.border)
}

fn row(p: &Palette) -> Div {
    div().flex().items_center().justify_between().gap(px(12.0)).w_full().min_h(px(p.row_h)).px(px(12.0))
}

/// The platform switch. It takes no keyboard focus, so it never draws a focus ring.
fn switch(t: &Theme, id: &'static str, label: &'static str, on: bool, toggle: Toggle) -> impl IntoElement {
    let p = t.palette();
    let (w, h, k) = p.switch;
    let mut knob = div().size(px(k)).rounded_full();
    let mut track = div().w(px(w)).h(px(h)).flex().items_center().px(px((h - k) / 2.0)).rounded_full();
    if t.look == Look::Windows {
        // Fluent: an outlined track with a dark knob when off, a filled track when on.
        let ink: Hsla = rgba(if t.dark { 0xFFFFFF9E } else { 0x0000009E }).into();
        track = track.border_1().border_color(if on { t.accent } else { ink });
        knob = knob.bg(if on { p.knob } else { ink });
    } else {
        knob = knob.bg(p.knob).shadow(vec![BoxShadow {
            color: rgba(0x00000040).into(),
            offset: gpui_kit::point(px(0.0), px(1.0)),
            blur_radius: px(2.0),
            spread_radius: px(0.0),
            inset: false,
        }]);
    }
    track = if on { track.bg(t.accent).justify_end() } else { track.bg(p.switch_off).justify_start() };
    div()
        .id(id)
        .role(Role::Switch)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .aria_label(label)
        .flex_none()
        .cursor_pointer()
        .on_click(move |_: &ClickEvent, window, cx| toggle(!on, window, cx))
        .child(track.child(knob))
}

/// A row with a title and a switch. Tab reaches the row; Space or Return flips the switch. Keyboard
/// focus shows as a soft accent tint on the row, never an outline ring.
fn switch_row(
    t: &Theme,
    focus: &FocusHandle,
    id: &'static str,
    label: &'static str,
    title: impl IntoElement,
    on: bool,
    toggle: Toggle,
) -> gpui_kit::Stateful<Div> {
    let p = t.palette();
    let tint = t.accent.opacity(0.14);
    let key_toggle = toggle.clone();
    row(&p)
        .id(SharedString::from(format!("{id}-row")))
        .track_focus(focus)
        .rounded(px(p.group_radius - 1.0))
        .focus_visible(move |s| s.bg(tint))
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            if matches!(e.keystroke.key.as_str(), "space" | "enter") {
                key_toggle(!on, window, cx);
                cx.stop_propagation();
            }
        })
        .child(title)
        .child(switch(t, id, label, on, toggle))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Primary,
}

fn button(
    t: &Theme,
    id: &'static str,
    text: &'static str,
    kind: Kind,
    enabled: bool,
    click: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<Div> {
    let p = t.palette();
    let primary = kind == Kind::Primary;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(text)
        .flex()
        .items_center()
        .justify_center()
        .h(px(p.control_h))
        .px(px(if t.look == Look::Mac { 14.0 } else { 18.0 }))
        .rounded(px(p.control_radius))
        .border_1()
        .border_color(if primary { t.accent } else { p.border })
        .bg(if primary { t.accent } else { p.button })
        .text_color(if primary { p.on_accent } else { p.label })
        .when(t.look != Look::Mac, |b| b.font_weight(FontWeight::MEDIUM))
        .when(!enabled, |b| b.opacity(0.45))
        .when(enabled, |b| b.cursor_pointer().on_click(move |_, window, cx| click(window, cx)))
        .child(text)
}

/// What the user filled in. The app sends it, such as with `Client::send_feedback`.
#[derive(Clone, Debug)]
pub struct Report {
    /// Empty only in ask mode, when the user typed nothing.
    pub text: String,
    /// Only when the user typed one.
    pub email: Option<String>,
    /// Attach the last crash report.
    pub include_crash: bool,
}

/// The "Report a problem" form. Put it in its own window or in a sheet.
pub struct ReportProblem {
    message: Entity<TextareaState>,
    email: Entity<InputState>,
    crash_focus: FocusHandle,
    include_crash: bool,
    /// Ask mode: the app name, after a crash.
    asking: Option<SharedString>,
    sent: bool,
    theme: Option<Theme>,
    on_send: Option<Rc<dyn Fn(Report, &mut Window, &mut App)>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ReportProblem {
    /// `sharing` is the Settings switch: when it is off, "Include last crash" starts off.
    pub fn new(sharing: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build("What happened, and what did you expect?", sharing, None, window, cx)
    }

    /// Ask mode, after a crash: the heading is "<App> Quit Unexpectedly", the crash is included, and
    /// the message is optional. [`ask_crash`] opens it in its own window.
    pub fn ask(app_name: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build("Optional: what were you doing when it quit?", true, Some(app_name.into()), window, cx)
    }

    fn build(prompt: &'static str, include_crash: bool, asking: Option<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| TextareaState::new(window, cx).placeholder(prompt));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("name@example.com"));
        cx.observe(&message, |_, _, cx| cx.notify()).detach();
        let crash_focus = cx.focus_handle().tab_stop(true);
        ReportProblem { message, email, crash_focus, include_crash, asking, sent: false, theme: None, on_send: None, on_close: None }
    }

    /// Send is ready with text, or in ask mode with the crash included.
    fn ready(&self, cx: &App) -> bool {
        !self.message.read(cx).value().trim().is_empty() || (self.asking.is_some() && self.include_crash)
    }

    /// Called with the report when the user presses Send. The form then shows "Report sent".
    pub fn on_send(mut self, f: impl Fn(Report, &mut Window, &mut App) + 'static) -> Self {
        self.on_send = Some(Rc::new(f));
        self
    }

    /// Called on Cancel and on Done: close the window or the sheet here.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Draws this theme instead of the window's own.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Shows the "Report sent" state, such as for a preview.
    pub fn set_sent(&mut self, sent: bool, cx: &mut Context<Self>) {
        self.sent = sent;
        cx.notify();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let text = self.message.read(cx).value().trim().to_string();
        let email = self.email.read(cx).value().trim().to_string();
        let report = Report { text, email: (!email.is_empty()).then_some(email), include_crash: self.include_crash };
        if let Some(f) = self.on_send.clone() {
            f(report, window, cx);
        }
        self.set_sent(true, cx);
    }

    fn form(&self, t: &Theme, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let me = cx.entity();
        let close = self.on_close.clone();
        let cancel = button(t, "ic-report-cancel", "Cancel", Kind::Plain, true, move |window, cx| {
            if let Some(f) = &close {
                f(window, cx);
            }
        });
        let ready = self.ready(cx);
        let send = button(t, "ic-report-send", "Send", Kind::Primary, ready, {
            let me = me.clone();
            move |window, cx| me.update(cx, |me, cx| me.send(window, cx))
        });
        let crash_toggle: Toggle = {
            let me = me.clone();
            Rc::new(move |on, _, cx| {
                me.update(cx, |me, cx| {
                    me.include_crash = on;
                    cx.notify();
                })
            })
        };
        let field_box = |h: Option<f32>| {
            div()
                .w_full()
                .when_some(h, |d, h| d.h(px(h)))
                .px(px(if t.look == Look::Mac { 4.0 } else { 6.0 }))
                .py(px(2.0))
                .bg(p.field)
                .border_1()
                .border_color(p.border)
                .rounded(px(p.control_radius))
                .text_size(px(p.text))
        };
        // The fields draw no frame of their own, so focus never changes their border.
        let message = field_box(Some(132.0)).child(
            Textarea::new(&self.message).aria_label("What happened").appearance(false).with_size(Size::Small).h(px(126.0)),
        );
        let email = field_box(None)
            .w(px(220.0))
            .child(Input::new(&self.email).aria_label("Email (optional)").appearance(false).with_size(Size::Small));
        let options = group(p)
            .child(
                switch_row(
                    t,
                    &self.crash_focus,
                    "ic-report-crash",
                    "Include last crash",
                    div().text_color(p.label).child("Include last crash"),
                    self.include_crash,
                    crash_toggle,
                ),
            )
            .child(hairline(p))
            .child(
                row(p)
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child(div().text_color(p.label).child("Email"))
                            .child(div().text_color(p.secondary).child("(optional)")),
                    )
                    .child(email),
            );
        let heading = |d: Div| d.text_size(px(p.title)).font_weight(FontWeight::SEMIBOLD).text_color(p.label);
        let title = |plain: &'static str| -> SharedString {
            self.asking.as_ref().map_or(plain.into(), |app| format!("{app} Quit Unexpectedly").into())
        };
        let lead = div().text_color(p.secondary).child("Tell us what went wrong. We read every report.");
        let asking = self.asking.is_some();
        match t.look {
            // libadwaita: a header bar with Cancel left, the title centered, Send right.
            Look::Gnome => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(48.0))
                        .px(px(6.0))
                        .child(cancel)
                        .child(heading(div()).font_weight(FontWeight::BOLD).child(title("Report a Problem")))
                        .child(send),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .p(px(18.0))
                        .when(!asking, |d| d.child(lead))
                        .child(message)
                        .child(options),
                )
                .into_any_element(),
            // WinUI ContentDialog: the title, the content, then a footer band with equal buttons.
            Look::Windows => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.0))
                        .p(px(24.0))
                        .bg(p.group)
                        .child(heading(div()).child(title("Report a problem")))
                        .when(!asking, |d| d.child(lead))
                        .child(message)
                        .child(options.bg(p.window)),
                )
                .child(hairline(p))
                .child(div().flex().gap(px(8.0)).p(px(24.0)).child(send.flex_1()).child(cancel.flex_1()))
                .into_any_element(),
            // macOS sheet: bold title, a grouped form, Cancel and Send at the bottom right.
            Look::Mac => div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .p(px(20.0))
                .child(div().flex().flex_col().gap(px(4.0)).child(heading(div()).font_weight(FontWeight::BOLD).child(title("Report a Problem"))).when(!asking, |d| d.child(lead)))
                .child(message)
                .child(options)
                .child(div().flex().justify_end().gap(px(10.0)).pt(px(4.0)).child(cancel).child(send))
                .into_any_element(),
        }
    }

    fn done(&self, t: &Theme, p: &Palette) -> AnyElement {
        let close = self.on_close.clone();
        let done = button(t, "ic-report-done", if t.look == Look::Windows { "Close" } else { "Done" }, Kind::Primary, true, move |window, cx| {
            if let Some(f) = &close {
                f(window, cx);
            }
        });
        let mark_size = if t.look == Look::Gnome { 64.0 } else { 44.0 };
        div()
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .justify_center()
            .gap(px(10.0))
            .px(px(24.0))
            .child(
                div()
                    .size(px(mark_size))
                    .rounded_full()
                    .bg(t.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(p.on_accent)
                    .text_size(px(mark_size * 0.5))
                    .font_weight(FontWeight::BOLD)
                    .child("\u{2713}"),
            )
            .child(div().text_size(px(p.title.max(15.0))).font_weight(FontWeight::BOLD).text_color(p.label).child("Report sent"))
            .child(div().text_color(p.secondary).child("Thanks for telling us. It helps us fix things faster."))
            .child(div().pt(px(10.0)).child(done))
            .into_any_element()
    }
}

impl Render for ReportProblem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.unwrap_or_else(|| Theme::of(window));
        let p = t.palette();
        div()
            .size_full()
            .bg(if t.look == Look::Windows { p.group } else { p.window })
            .font_family(p.font)
            .text_size(px(p.text))
            .text_color(p.label)
            .child(if self.sent { self.done(&t, &p) } else { self.form(&t, &p, cx) })
    }
}

/// Apple's caution sign: a yellow rounded triangle with a white rim and a black "!".
const CAUTION_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFE14D"/><stop offset="1" stop-color="#F5B800"/></linearGradient></defs><path d="M27.6 7.6a5 5 0 0 1 8.8 0l24 43.4A5 5 0 0 1 56 58.5H8a5 5 0 0 1-4.4-7.5z" fill="#fff" stroke="#00000026" stroke-width="1"/><path d="M28.9 10.6a3.5 3.5 0 0 1 6.2 0l22.6 40.9a3.5 3.5 0 0 1-3.1 5.2H9.4a3.5 3.5 0 0 1-3.1-5.2z" fill="url(#g)"/><rect x="29.2" y="22" width="5.6" height="20" rx="2.8" fill="#1a1a1a"/><circle cx="32" cy="48.5" r="3.2" fill="#1a1a1a"/></svg>"##;

/// The crash's "Problem Report" window, a copy of the macOS one: a comment field, the exact text that
/// is sent, and "Don't Send" and "Send to Developer". [`ask_crash`] opens it after "Report…".
pub struct ProblemReport {
    app: SharedString,
    details: SharedString,
    comment: Entity<TextareaState>,
    theme: Option<Theme>,
    on_send: Option<Rc<dyn Fn(String, &mut Window, &mut App)>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ProblemReport {
    /// `details` is the read-only text that is sent, such as the crash JSON.
    pub fn new(app: impl Into<SharedString>, details: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let comment = cx.new(|cx| TextareaState::new(window, cx).placeholder("Provide any steps necessary to reproduce the problem."));
        ProblemReport { app: app.into(), details: details.into(), comment, theme: None, on_send: None, on_close: None }
    }

    /// Called with the comment (maybe empty) on "Send to Developer".
    pub fn on_send(mut self, f: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_send = Some(Rc::new(f));
        self
    }

    /// Called on "Don't Send" and after Send: close the window here.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Draws this theme instead of the window's own.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Render for ProblemReport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::scroll::ScrollableElement as _;
        use gpui_kit::component::{Icon, IconName};
        let t = self.theme.unwrap_or_else(|| Theme::of(window));
        let p = t.palette();
        let mac = t.look == Look::Mac;
        let mono = match t.look {
            Look::Mac => "Menlo",
            Look::Windows => "Consolas",
            Look::Gnome => "Monospace",
        };
        let me = cx.entity();
        let (close, close2, send_cb) = (self.on_close.clone(), self.on_close.clone(), self.on_send.clone());
        let dont = button(&t, "ic-crash-dont-send", "Don\u{2019}t Send", Kind::Plain, true, move |window, cx| {
            if let Some(f) = &close {
                f(window, cx);
            }
        });
        let send = button(&t, "ic-crash-send", "Send to Developer", Kind::Primary, true, move |window, cx| {
            let text = me.read(cx).comment.read(cx).value().trim().to_string();
            if let Some(f) = &send_cb {
                f(text, window, cx);
            }
            if let Some(f) = &close2 {
                f(window, cx);
            }
        });
        let boxed = |d: Div| d.w_full().bg(p.field).border_1().border_color(p.border).rounded(px(if mac { 0.0 } else { p.control_radius }));
        let icon = gpui_kit::img(std::sync::Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Svg, CAUTION_SVG.as_bytes().to_vec()))).size(px(64.0)).flex_none();
        let header = div()
            .flex()
            .gap(px(15.0))
            .child(icon)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pt(px(4.0))
                    .gap(px(13.0))
                    .child(div().font_weight(FontWeight::BOLD).text_size(px(p.title.max(p.text))).child(SharedString::from(format!("{} quit unexpectedly.", self.app))))
                    .child(
                        div()
                            .text_size(px(p.text - 2.0))
                            .text_color(p.label)
                            .child("Click \u{201C}Send to Developer\u{201D} to submit the report to the developer. This information is collected anonymously."),
                    ),
            );
        let lines = self.details.lines().map(|l| div().whitespace_nowrap().child(SharedString::from(l.to_string())));
        let body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .pl(px(79.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .mt(px(12.0))
                    .child(Icon::new(IconName::ChevronDown).size(px(11.0)).text_color(p.secondary))
                    .child("Comments"),
            )
            .child(
                boxed(div()).mt(px(5.0)).h(px(73.0)).px(px(4.0)).py(px(2.0)).child(
                    Textarea::new(&self.comment).aria_label("Comments").appearance(false).with_size(Size::Small).h(px(67.0)),
                ),
            )
            .child(div().mt(px(9.0)).pl(px(2.0)).child("Problem Details and System Configuration"))
            .child(
                boxed(div()).mt(px(3.0)).flex_1().min_h_0().child(
                    div()
                        .id("ic-crash-details")
                        .size_full()
                        .px(px(6.0))
                        .py(px(5.0))
                        .font_family(mono)
                        .text_size(px(11.0))
                        .line_height(px(13.0))
                        .children(lines)
                        .overflow_y_scrollbar(),
                ),
            )
            .child(div().flex().justify_end().gap(px(if mac { 12.0 } else { 8.0 })).mt(px(14.0)).child(dont.min_w(px(109.0))).child(send.min_w(px(109.0))));
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(px(20.0))
            .pl(px(20.0))
            .pr(px(21.0))
            .pb(px(20.0))
            .bg(p.window)
            .font_family(p.font)
            .text_size(px(p.text))
            .text_color(p.label)
            .child(header)
            .child(body)
    }
}

/// The JSON that is sent, keys sorted, one frame per line.
#[cfg(feature = "crash")]
fn crash_text(crash: &crate::crash::CrashReport) -> String {
    let mut v = serde_json::to_value(crash).unwrap_or_default();
    let frames = v.as_object_mut().and_then(|o| o.remove("frames"));
    let mut s = serde_json::to_string_pretty(&v).unwrap_or_default();
    if let (Some(serde_json::Value::Array(frames)), Some(end)) = (frames, s.rfind("\n}")) {
        let list: Vec<String> = frames.iter().map(|f| format!("    {f}")).collect();
        s.replace_range(end.., &format!(",\n  \"frames\": [\n{}\n  ]\n}}", list.join(",\n")));
    }
    s
}

/// Ask mode (features `crash` and `feedback`), like macOS: when a crash waits on disk, shows the
/// platform alert "<App> quit unexpectedly." with "Report…" and "Ignore", without activating the app.
/// Ignore deletes the crash; "Report…" opens the [`ProblemReport`] window, whose "Send to Developer"
/// sends the newest crash (no install id) and the comment as feedback. Closing the window in any way
/// deletes every waiting crash, so it asks once. Call it once at launch, after `gpui_kit::init`.
#[cfg(all(feature = "crash", feature = "feedback"))]
pub fn ask_crash(client: std::sync::Arc<crate::Client>, app_name: &str, cx: &mut App) {
    let Some(crash) = client.pending_crashes().pop() else { return };
    let name = app_name.to_string();
    #[cfg(not(target_os = "macos"))]
    open_report(client, name, crash, true, cx);
    // GPUI's prompt is a sheet on a window, so macOS gets a real app-modal alert with no window
    // around it, and the report window opens only after "Report…". It runs in a task, outside any
    // App borrow, so GPUI work in the modal loop does not clash with it.
    #[cfg(target_os = "macos")]
    cx.spawn(async move |cx| {
        let report = mac_alert(&name);
        let _ = cx.update(|cx| {
            if report {
                open_report(client, name, crash, false, cx);
            } else {
                client.pending_crashes().iter().for_each(|r| client.discard_crash(&r.id));
            }
        });
    })
    .detach();
}

/// The macOS crash alert, app-modal: true for "Report…", false for "Ignore" or Escape.
#[cfg(all(feature = "crash", feature = "feedback", target_os = "macos"))]
fn mac_alert(name: &str) -> bool {
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSImage, NSImageNameCaution};
    use objc2_foundation::NSString;
    let Some(mtm) = objc2::MainThreadMarker::new() else { return false };
    let alert = NSAlert::new(mtm);
    // The plain caution sign, like the system alert; the Critical style would add an app-icon badge.
    unsafe { alert.setIcon(NSImage::imageNamed(NSImageNameCaution).as_deref()) }; // main thread, a system image
    alert.setMessageText(&NSString::from_str(&format!("{name} quit unexpectedly.")));
    alert.setInformativeText(&NSString::from_str(PROMPT_DETAIL));
    alert.addButtonWithTitle(&NSString::from_str("Report\u{2026}"));
    alert.addButtonWithTitle(&NSString::from_str("Ignore")).setKeyEquivalent(&NSString::from_str("\u{1b}"));
    alert.runModal() == NSAlertFirstButtonReturn
}

#[cfg(all(feature = "crash", feature = "feedback"))]
const PROMPT_DETAIL: &str = "Click Report to see what would be sent. Nothing is sent unless you press Send.";

#[cfg(all(feature = "crash", feature = "feedback"))]
fn crash_prompt(name: &str, window: &mut Window, cx: &mut App) -> impl std::future::Future<Output = bool> + use<> {
    use gpui_kit::{PromptButton, PromptLevel};
    let rx = window.prompt(
        PromptLevel::Critical,
        &format!("{name} quit unexpectedly."),
        Some(PROMPT_DETAIL),
        &[PromptButton::ok("Report\u{2026}"), PromptButton::cancel("Ignore")],
        cx,
    );
    async move { rx.await == Ok(0) }
}

/// The Problem Report window; with `ask`, the platform alert shows on it first (Windows, Linux).
#[cfg(all(feature = "crash", feature = "feedback"))]
fn open_report(client: std::sync::Arc<crate::Client>, name: String, crash: crate::crash::CrashReport, ask: bool, cx: &mut App) {
    use gpui_kit::{size, TitlebarOptions, WindowBounds, WindowOptions};
    let details = crash_text(&crash);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1000.0), px(740.0)), cx)),
        titlebar: Some(TitlebarOptions { title: Some(format!("Problem Report for {name}").into()), ..Default::default() }),
        focus: false, // never activates the app
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    let _ = gpui_kit::open_window(options, cx, move |window, cx| {
        let view = cx.new(|cx| {
            let sent = Rc::new(std::cell::Cell::new(false));
            let (c, s) = (client.clone(), sent.clone());
            cx.on_release(move |_, _| {
                if !s.get() {
                    c.pending_crashes().iter().for_each(|r| c.discard_crash(&r.id));
                }
            })
            .detach();
            ProblemReport::new(name.clone(), details, window, cx)
                .on_send(move |text, _, _| {
                    sent.set(true);
                    let (c, id) = (client.clone(), crash.id.clone());
                    std::thread::spawn(move || {
                        c.send_crash(&id);
                        if !text.is_empty() {
                            let f = crate::feedback::Feedback { kind: crate::feedback::Kind::Bug, text, email: None, screenshot: None, pro: false };
                            let _ = c.send_feedback(f);
                        }
                        c.pending_crashes().iter().for_each(|r| c.discard_crash(&r.id));
                    });
                })
                .on_close(|window, _| window.remove_window())
        });
        if ask {
            let answer = crash_prompt(&name, window, cx);
            window
                .spawn(cx, async move |cx| {
                    if !answer.await {
                        let _ = cx.update(|window, _| window.remove_window()); // Ignore: the release hook deletes the crash
                    }
                })
                .detach();
        }
        view
    });
}

/// What the sharing switch sends, from the features this build has.
pub fn sharing_summary() -> String {
    let mut lines = vec!["When on, the app sends:"];
    if cfg!(feature = "usage") {
        lines.push("\u{2022} a daily count of uses, the app version, OS and CPU type");
    }
    if cfg!(feature = "crash") {
        lines.push("\u{2022} crash reports, with your home folder removed from paths");
    }
    if cfg!(feature = "feedback") {
        lines.push("\u{2022} the problem reports you choose to send");
    }
    if lines.len() == 1 {
        return "This build sends nothing.".into();
    }
    lines.push("Never your name, files or account.");
    lines.join("\n")
}

/// The tooltip of the "1" note, drawn in the platform's tooltip style.
pub struct Tip {
    theme: Theme,
    text: SharedString,
}

impl Tip {
    pub fn new(theme: Theme, text: impl Into<SharedString>) -> Self {
        Tip { theme, text: text.into() }
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let p = t.palette();
        let (size, radius) = match t.look {
            Look::Mac => (11.5, 6.0),
            Look::Windows => (12.0, 4.0),
            Look::Gnome => (13.0, 6.0),
        };
        div()
            .max_w(px(290.0))
            .px(px(8.0))
            .py(px(5.0))
            .bg(p.tip)
            .border_1()
            .border_color(if t.look == Look::Gnome { p.tip } else { p.border })
            .rounded(px(radius))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000026).into(),
                offset: gpui_kit::point(px(0.0), px(2.0)),
                blur_radius: px(6.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .font_family(p.font)
            .text_size(px(size))
            .text_color(p.tip_text)
            .flex()
            .flex_col()
            .children(self.text.lines().map(|l| div().child(SharedString::from(l.to_string()))))
    }
}

/// The small "1" after the label: softer color, dotted underline, the summary on hover.
fn note(t: &Theme) -> impl IntoElement {
    let p = t.palette();
    let theme = *t;
    let size = (p.text * 0.7).round();
    let dots = div().flex().gap(px(1.0)).children((0..4).map(|_| div().size(px(1.0)).bg(p.secondary)));
    div()
        .id("ic-sharing-note")
        .relative()
        .top(px(-p.text * 0.2))
        .flex()
        .flex_col()
        .items_center()
        .text_size(px(size))
        .line_height(px(size + 1.0))
        .text_color(p.secondary)
        .child("1")
        .child(dots)
        .tooltip(move |_, cx| cx.new(|_| Tip::new(theme, sharing_summary())).into())
}

/// The Settings row: "Share anonymous usage" with the note and a switch. Nothing shows under it.
/// Wire `toggle` to `Client::set_enabled` and save the choice; the app's default is on. `focus` is a
/// handle the app keeps, made with `cx.focus_handle().tab_stop(true)`, so Tab reaches the row.
pub fn sharing_row(
    theme: Theme,
    focus: &FocusHandle,
    on: bool,
    toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = theme.palette();
    let title = div().flex().items_start().gap(px(2.0)).text_color(p.label).child("Share anonymous usage").child(note(&theme));
    switch_row(&theme, focus, "ic-sharing", "Share anonymous usage", title, on, Rc::new(toggle))
        .font_family(p.font)
        .text_size(px(p.text))
}

/// [`sharing_row`] inside its own group box, for an app without groups of its own.
pub fn sharing_group(
    theme: Theme,
    focus: &FocusHandle,
    on: bool,
    toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    group(&theme.palette()).child(sharing_row(theme, focus, on, toggle))
}
