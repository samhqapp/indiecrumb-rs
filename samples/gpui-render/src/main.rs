//! Draws the `gpui` UI offscreen and saves PNGs: `cargo run -- <out folder>`. Opens no window.

use gpui_kit::component::theme::{Theme as KitTheme, ThemeMode};
use gpui_kit::{div, px, size, AppContext as _, HeadlessAppContext, IntoElement, ParentElement, Render, Styled};
use indiecrumb::ui::{sharing_group, Look, ProblemReport, ReportProblem, Theme, Tip};
use std::path::PathBuf;
use std::sync::Arc;

/// The sharing row with its tooltip drawn open under the note, as it shows on hover.
struct Settings(Theme, gpui_kit::FocusHandle);

impl Render for Settings {
    fn render(&mut self, _: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        let bg: gpui_kit::Hsla = gpui_kit::rgba(match (self.0.look, self.0.dark) {
            (Look::Mac, false) => 0xECECECFF,
            (Look::Mac, true) => 0x282828FF,
            (Look::Windows, false) => 0xF3F3F3FF,
            (Look::Windows, true) => 0x202020FF,
            (Look::Gnome, false) => 0xFAFAFBFF,
            (Look::Gnome, true) => 0x222226FF,
        })
        .into();
        let tip = cx.new(|_| Tip::new(self.0, indiecrumb::ui::sharing_summary()));
        div()
            .size_full()
            .bg(bg)
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(sharing_group(self.0, &self.1, true, |_, _, _| {}))
            .child(div().pl(px(150.0)).child(tip))
    }
}

fn headless(dark: bool) -> HeadlessAppContext {
    let text = gpui_kit::platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(text, Arc::new(gpui_kit::assets::Assets), gpui_kit::platform::current_headless_renderer);
    cx.update(|cx| {
        gpui_kit::init(cx);
        KitTheme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    });
    cx
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("out folder"));
    std::fs::create_dir_all(&out).unwrap();
    for (look, name) in [(Look::Mac, "mac"), (Look::Windows, "windows"), (Look::Gnome, "gnome")] {
        for dark in [false, true] {
            let theme = Theme::new(look, dark);
            let mode = if dark { "dark" } else { "light" };
            {
                let mut cx = headless(dark);
                let w = cx
                    .open_window(size(px(1000.0), px(740.0)), move |window, cx| {
                        let details = std::fs::read_to_string(env!("CARGO_MANIFEST_DIR").to_string() + "/../crash.json").unwrap_or_default();
                        let r = cx.new(|cx| ProblemReport::new("Hushpen", details, window, cx).theme(theme));
                        cx.new(|cx| gpui_kit::base::Root::new(r, window, cx))
                    })
                    .unwrap();
                cx.run_until_parked();
                cx.capture_screenshot(w.into()).unwrap().save(out.join(format!("gpui-{name}-problem-{mode}.png"))).unwrap();
            }
            for sent in [false, true] {
                let mut cx = headless(dark);
                let h = match look {
                    Look::Mac => 390.0,
                    Look::Windows => 470.0,
                    Look::Gnome => 400.0,
                };
                let w = cx
                    .open_window(size(px(480.0), px(h)), move |window, cx| {
                        let form = cx.new(|cx| {
                            let mut f = ReportProblem::new(true, window, cx).theme(theme);
                            f.set_sent(sent, cx);
                            f
                        });
                        cx.new(|cx| gpui_kit::base::Root::new(form, window, cx))
                    })
                    .unwrap();
                cx.run_until_parked();
                let state = if sent { "sent" } else { "form" };
                cx.capture_screenshot(w.into()).unwrap().save(out.join(format!("gpui-{name}-report-{state}-{mode}.png"))).unwrap();
            }
            for focused in [false, true] {
                let mut cx = headless(dark);
                let w = cx
                    .open_window(size(px(480.0), px(250.0)), move |_, cx| {
                        cx.new(|cx| Settings(theme, cx.focus_handle().tab_stop(true)))
                    })
                    .unwrap();
                if focused {
                    // A key press first, so the focus counts as keyboard focus.
                    let f = w.update(&mut cx, |s, _, _| s.1.clone()).unwrap();
                    cx.update_window(w.into(), |_, window, cx| {
                        window.dispatch_keystroke(gpui_kit::Keystroke::parse("escape").unwrap(), cx);
                        window.focus(&f, cx);
                    })
                    .unwrap();
                }
                cx.run_until_parked();
                let tag = if focused { "sharing-focus" } else { "sharing" };
                cx.capture_screenshot(w.into()).unwrap().save(out.join(format!("gpui-{name}-{tag}-{mode}.png"))).unwrap();
            }
        }
    }
}
