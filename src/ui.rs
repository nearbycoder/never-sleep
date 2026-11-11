use crate::{State, power::PowerSource};
use rune::tok::{TextStyle, motion};
use rune::{Color, Cx, El, Family, Shape, SystemMaterial, canvas, col, el, row, text};

fn color(hex: &str) -> Color {
    rune::color::hex(hex).expect("valid palette color")
}

fn label(value: impl Into<String>, size: f32, weight: f32, ink: Color) -> El {
    text(value)
        .font(TextStyle::new(size, weight).family(Family::Named("System Font")))
        .color(ink)
}

fn switch(
    key: &'static str,
    name: &'static str,
    on: bool,
    ink: Color,
    accent: Color,
    action: fn(&mut State),
) -> El {
    el().key(key)
        .role("switch")
        .label(name)
        .checked(on)
        .focusable(true)
        .size(38.0, 22.0)
        .radius(11.0)
        .fill(if on { accent } else { ink.alpha(0.18) })
        .motion(motion::SNAPPY)
        .cursor(rune::Cursor::Pointer)
        .on_press(action)
        .child(
            el().key(format!("{key}-knob"))
                .size(18.0, 18.0)
                .radius(9.0)
                .fill(Color::WHITE)
                .shadow(1.0, 2.0, Color::BLACK.alpha(0.18))
                .absolute(if on { 18.0 } else { 2.0 }, 2.0),
        )
}

fn setting_icon(kind: u8, ink: Color) -> El {
    let shape = match kind {
        0 => Shape::new()
            .stroke(vec![[7.0, 2.0], [7.0, 7.0]], 1.6, ink)
            .stroke(vec![[13.0, 2.0], [13.0, 7.0]], 1.6, ink)
            .stroke(
                vec![
                    [5.0, 7.0],
                    [15.0, 7.0],
                    [15.0, 10.0],
                    [12.0, 13.0],
                    [8.0, 13.0],
                    [5.0, 10.0],
                    [5.0, 7.0],
                ],
                1.6,
                ink,
            )
            .stroke(vec![[10.0, 13.0], [10.0, 18.0]], 1.6, ink),
        1 => Shape::new()
            .stroke(
                vec![
                    [2.0, 3.0],
                    [18.0, 3.0],
                    [18.0, 14.0],
                    [2.0, 14.0],
                    [2.0, 3.0],
                ],
                1.5,
                ink,
            )
            .stroke(vec![[10.0, 14.0], [10.0, 18.0]], 1.5, ink)
            .stroke(vec![[6.0, 18.0], [14.0, 18.0]], 1.5, ink),
        _ => Shape::new()
            .stroke(
                vec![
                    [2.0, 4.0],
                    [18.0, 4.0],
                    [18.0, 17.0],
                    [2.0, 17.0],
                    [2.0, 4.0],
                ],
                1.5,
                ink,
            )
            .stroke(vec![[6.0, 13.0], [14.0, 13.0]], 3.0, ink),
    };
    el().size(30.0, 30.0)
        .radius(8.0)
        .continuous_corners()
        .fill(ink.alpha(0.06))
        .center()
        .child(canvas(shape).size(20.0, 20.0))
}

fn setting(
    kind: u8,
    title: &'static str,
    description: &'static str,
    control: El,
    ink: Color,
) -> El {
    row()
        .w_full()
        .padding(14.0)
        .gap(12.0)
        .center_y()
        .child(setting_icon(kind, ink.alpha(0.72)))
        .child(
            col()
                .grow(1.0)
                .gap(3.0)
                .child(label(title, 13.0, 500.0, ink))
                .child(label(description, 11.0, 400.0, ink.alpha(0.58)).wrap()),
        )
        .child(control)
}

pub(crate) fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let ink = if dark {
        color("#F5F5F7")
    } else {
        color("#1D1D1F")
    };
    let muted = ink.alpha(0.58);
    let bg = if dark {
        color("#29292C")
    } else {
        color("#F1F1F3")
    };
    let accent = cx.accent();
    let c = &s.controller;
    let source = match c.source {
        PowerSource::Ac => "Power adapter connected",
        PowerSource::Battery => "Running on battery",
        PowerSource::Unknown => "Power source unavailable",
    };
    let status_ink = if c.active() {
        color(if dark { "#62D887" } else { "#248342" })
    } else {
        muted
    };
    let divider = || el().w_full().h(0.5).fill(ink.alpha(0.1));
    let settings = col()
        .w_full()
        .radius(12.0)
        .continuous_corners()
        .fill(if dark {
            Color::WHITE.alpha(0.055)
        } else {
            Color::WHITE.alpha(0.48)
        })
        .border(0.5, ink.alpha(0.09))
        .child(setting(
            0,
            "Keep awake on power",
            "Prevent idle sleep while plugged in.",
            switch(
                "enable-switch",
                "Keep awake on power",
                c.enabled,
                ink,
                accent,
                State::toggle_enabled,
            ),
            ink,
        ))
        .child(divider())
        .child(setting(
            1,
            "Keep display awake",
            "Keep the screen on while protection is active.",
            switch(
                "display-switch",
                "Keep display awake",
                c.display,
                ink,
                accent,
                State::toggle_display,
            ),
            ink,
        ))
        .child(divider())
        .child(setting(
            2,
            "Show in Dock",
            "Turn off to use only the menu bar.",
            switch(
                "dock-switch",
                "Show in Dock",
                c.show_in_dock,
                ink,
                accent,
                State::toggle_show_in_dock,
            ),
            ink,
        ));
    let icon = match &s.icon {
        Some(icon) => el().key("app-icon").size(86.0, 86.0).pixels(icon.pixels()),
        None => el().size(86.0, 86.0),
    };
    let mut body = col().w_full().gap(18.0)
        .child(col().w_full().center().gap(8.0)
            .child(icon)
            .child(label("Never Sleep", 22.0, 650.0, ink))
            .child(row().center_y().gap(6.0)
                .child(el().size(6.0, 6.0).radius(3.0).fill(status_ink))
                .child(label(c.status(), 12.0, 500.0, status_ink).key("status-label")))
            .child(label(source, 11.0, 400.0, muted)))
        .child(settings)
        .child(col().w_full().gap(4.0).px(5.0)
            .child(label("Automatically pauses on battery", 11.0, 500.0, ink.alpha(0.72)))
            .child(label("Resumes when you plug back in. Closing the lid or choosing Sleep still works.", 11.0, 400.0, muted).wrap()));
    if let Some(error) = c
        .error
        .as_ref()
        .or(c.save_error.as_ref())
        .or(s.menu_error.as_ref())
    {
        body = body.child(
            label(
                error,
                11.0,
                500.0,
                color(if dark { "#FFB69A" } else { "#A13F26" }),
            )
            .key("error")
            .wrap(),
        );
    }
    // Rune suppresses this fill on a native transparent window; AppKit supplies
    // genuine Liquid Glass. The opaque fill is the offscreen/older-host fallback.
    let mut window = col().key("window").w_full().h_full().fill(bg);
    if !s.reduce_transparency {
        window = window.system_material(SystemMaterial::Glass);
    }
    window.child(
        col()
            .w_full()
            .h_full()
            .fill(bg.alpha(if s.reduce_transparency { 0.0 } else { 0.28 }))
            .child(row().w_full().h(44.0).center().window_drag().child(label(
                "Never Sleep",
                13.0,
                600.0,
                ink.alpha(0.75),
            )))
            .child(
                col()
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .scroll(rune::ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(body.px(24.0).pt(6.0).pb(18.0)),
            )
            .child(
                row()
                    .w_full()
                    .px(24.0)
                    .py(16.0)
                    .gap(12.0)
                    .center_y()
                    .justify_between()
                    .child(label(
                        "Always available in the menu bar",
                        11.0,
                        400.0,
                        muted,
                    ))
                    .child(
                        row()
                            .key("hide-window")
                            .role("button")
                            .label("Close window and keep running")
                            .focusable(true)
                            .h(28.0)
                            .px(19.0)
                            .radius(14.0)
                            .continuous_corners()
                            .center()
                            .fill(ink.alpha(0.08))
                            .border(0.5, ink.alpha(0.08))
                            .hover(ink.alpha(0.08))
                            .cursor(rune::Cursor::Pointer)
                            .on_press_tx(|_: &mut State, tx| tx.close_window())
                            .child(label("Done", 12.0, 500.0, ink)),
                    ),
            ),
    )
}
